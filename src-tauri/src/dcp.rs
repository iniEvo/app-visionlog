//! Adobe DNG Camera Profile (.dcp) 解析与写入
//!
//! 格式：TIFF（`II`/`MM` + magic `0x4352` 'CR'）+ 单 IFD + DNG profile tags
//! 不依赖外部库，直接读写 IFD。未识别的 tag 走 passthrough，
//! 保证 RGBTables / BigTable* / GainTableMap 等 DNG 1.6 tag 在 round-trip 中幸存。

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::io::Read;

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ByteOrder {
    Little, // II
    Big,    // MM
}

impl ByteOrder {
    #[inline]
    fn u16(&self, b: &[u8]) -> u16 {
        match self {
            ByteOrder::Little => u16::from_le_bytes([b[0], b[1]]),
            ByteOrder::Big => u16::from_be_bytes([b[0], b[1]]),
        }
    }
    #[inline]
    fn u32(&self, b: &[u8]) -> u32 {
        match self {
            ByteOrder::Little => u32::from_le_bytes([b[0], b[1], b[2], b[3]]),
            ByteOrder::Big => u32::from_be_bytes([b[0], b[1], b[2], b[3]]),
        }
    }
    #[inline]
    fn i32(&self, b: &[u8]) -> i32 {
        match self {
            ByteOrder::Little => i32::from_le_bytes([b[0], b[1], b[2], b[3]]),
            ByteOrder::Big => i32::from_be_bytes([b[0], b[1], b[2], b[3]]),
        }
    }
    #[inline]
    fn f32(&self, b: &[u8]) -> f32 {
        match self {
            ByteOrder::Little => f32::from_le_bytes([b[0], b[1], b[2], b[3]]),
            ByteOrder::Big => f32::from_be_bytes([b[0], b[1], b[2], b[3]]),
        }
    }
}

pub const TYPE_BYTE: u16 = 1;
pub const TYPE_ASCII: u16 = 2;
pub const TYPE_SHORT: u16 = 3;
pub const TYPE_LONG: u16 = 4;
pub const TYPE_RATIONAL: u16 = 5;
pub const TYPE_SBYTE: u16 = 6;
pub const TYPE_UNDEFINED: u16 = 7;
pub const TYPE_SSHORT: u16 = 8;
pub const TYPE_SLONG: u16 = 9;
pub const TYPE_SRATIONAL: u16 = 10;
pub const TYPE_FLOAT: u16 = 11;
pub const TYPE_DOUBLE: u16 = 12;

#[inline]
fn type_size(t: u16) -> u32 {
    match t {
        TYPE_BYTE | TYPE_ASCII | TYPE_SBYTE | TYPE_UNDEFINED => 1,
        TYPE_SHORT | TYPE_SSHORT => 2,
        TYPE_LONG | TYPE_SLONG | TYPE_FLOAT => 4,
        TYPE_RATIONAL | TYPE_SRATIONAL | TYPE_DOUBLE => 8,
        _ => 0,
    }
}

/// 单个 tag 条目：raw 字节原样保存，便于 passthrough。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TagEntry {
    pub typ: u16,
    pub count: u32,
    #[serde(with = "serde_bytes_vec")]
    pub data: Vec<u8>, // 已解析出 data offset 的最终数据段
}

mod serde_bytes_vec {
    use serde::{Deserialize, Deserializer, Serializer};
    pub fn serialize<S: Serializer>(v: &Vec<u8>, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_bytes(v)
    }
    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<u8>, D::Error> {
        Vec::<u8>::deserialize(d)
    }
}

impl TagEntry {
    pub fn new(typ: u16, count: u32, data: Vec<u8>) -> Self {
        Self { typ, count, data }
    }

    /// 用 SRATIONAL×n 写矩阵/曝光偏移，n 必须 == data 长度 / 8
    pub fn srational(values: &[f64], order: ByteOrder) -> Self {
        let mut data = Vec::with_capacity(values.len() * 8);
        for &v in values {
            let den: i64 = 10_000;
            let num: i64 = (v * den as f64).round() as i64;
            match order {
                ByteOrder::Little => data.extend_from_slice(&(num as i32).to_le_bytes()),
                ByteOrder::Big => data.extend_from_slice(&(num as i32).to_be_bytes()),
            }
            match order {
                ByteOrder::Little => data.extend_from_slice(&(den as i32).to_le_bytes()),
                ByteOrder::Big => data.extend_from_slice(&(den as i32).to_be_bytes()),
            }
        }
        Self { typ: TYPE_SRATIONAL, count: values.len() as u32, data }
    }

    /// FLOAT×n（LookTable / HSM / ToneCurve）
    pub fn float(values: &[f32], order: ByteOrder) -> Self {
        let mut data = Vec::with_capacity(values.len() * 4);
        for &v in values {
            match order {
                ByteOrder::Little => data.extend_from_slice(&v.to_le_bytes()),
                ByteOrder::Big => data.extend_from_slice(&v.to_be_bytes()),
            }
        }
        Self { typ: TYPE_FLOAT, count: values.len() as u32, data }
    }

    pub fn long(values: &[u32], order: ByteOrder) -> Self {
        let mut data = Vec::with_capacity(values.len() * 4);
        for &v in values {
            match order {
                ByteOrder::Little => data.extend_from_slice(&v.to_le_bytes()),
                ByteOrder::Big => data.extend_from_slice(&v.to_be_bytes()),
            }
        }
        Self { typ: TYPE_LONG, count: values.len() as u32, data }
    }

    pub fn short(value: u16, order: ByteOrder) -> Self {
        let data = match order {
            ByteOrder::Little => value.to_le_bytes().to_vec(),
            ByteOrder::Big => value.to_be_bytes().to_vec(),
        };
        Self { typ: TYPE_SHORT, count: 1, data }
    }

    /// ASCII 字符串（自动加 \0；非 ASCII 字符退化为 BYTE+UTF-8，与 Adobe 行为一致）
    pub fn ascii_or_byte(s: &str, order: ByteOrder) -> Self {
        if s.is_ascii() {
            let mut data = s.as_bytes().to_vec();
            data.push(0);
            Self { typ: TYPE_ASCII, count: data.len() as u32, data }
        } else {
            let _ = order;
            let data = s.as_bytes().to_vec();
            Self { typ: TYPE_BYTE, count: data.len() as u32, data }
        }
    }

    /// LONG×3（LookTable/HueSatMapDims）
    pub fn long3(a: u32, b: u32, c: u32, order: ByteOrder) -> Self {
        Self::long(&[a, b, c], order)
    }
}

// ============================================================
// 已知 tag 号（与 Adobe DNG SDK / ExifTool 交叉验证）
// ============================================================
pub mod tags {
    pub const UNIQUE_CAMERA_MODEL: u16 = 50708; // 0xC614
    pub const COLOR_MATRIX_1: u16 = 50721; // 0xC621
    pub const COLOR_MATRIX_2: u16 = 50722; // 0xC622
    pub const CAMERA_CALIBRATION_1: u16 = 50723; // 0xC623
    pub const CAMERA_CALIBRATION_2: u16 = 50724; // 0xC624
    pub const REDUCTION_MATRIX_1: u16 = 50725; // 0xC625
    pub const REDUCTION_MATRIX_2: u16 = 50726; // 0xC626
    pub const AS_SHOT_NEUTRAL: u16 = 50728; // 0xC628
    pub const CALIBRATION_ILLUMINANT_1: u16 = 50778; // 0xC65A
    pub const CALIBRATION_ILLUMINANT_2: u16 = 50779; // 0xC65B
    pub const PROFILE_CALIBRATION_SIGNATURE: u16 = 50932; // 0xC6F4
    pub const PROFILE_NAME: u16 = 50936; // 0xC6F8
    pub const HUE_SAT_MAP_DIMS: u16 = 50937; // 0xC6F9
    pub const HUE_SAT_MAP_DATA_1: u16 = 50938; // 0xC6FA
    pub const HUE_SAT_MAP_DATA_2: u16 = 50939; // 0xC6FB
    pub const PROFILE_TONE_CURVE: u16 = 50940; // 0xC6FC
    pub const PROFILE_EMBED_POLICY: u16 = 50941; // 0xC6FD
    pub const PROFILE_COPYRIGHT: u16 = 50942; // 0xC6FE
    pub const FORWARD_MATRIX_1: u16 = 50964; // 0xC714
    pub const FORWARD_MATRIX_2: u16 = 50965; // 0xC715
    pub const LOOK_TABLE_DIMS: u16 = 50981; // 0xC725
    pub const LOOK_TABLE_DATA: u16 = 50982; // 0xC726
    pub const PROFILE_HUE_SAT_MAP_ENCODING: u16 = 51107; // 0xC7A3 (DNG 1.4)
    pub const PROFILE_LOOK_TABLE_ENCODING: u16 = 51108; // 0xC7A4 (DNG 1.4)
    pub const BASELINE_EXPOSURE_OFFSET: u16 = 51109; // 0xC7A5
    pub const DEFAULT_BLACK_RENDER: u16 = 51110; // 0xC7A6
    pub const PROFILE_GROUP_NAME: u16 = 52552; // 0xCD28
}

// ============================================================
// Dcp 容器
// ============================================================
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Dcp {
    pub order: ByteOrder,
    pub entries: BTreeMap<u16, TagEntry>,
}

#[derive(Debug, thiserror::Error)]
pub enum DcpError {
    #[error("not a DCP: bad magic {0:?}")]
    BadMagic([u8; 4]),
    #[error("truncated file: need {0} bytes at offset {1}, have {2}")]
    Truncated(usize, usize, usize),
    #[error("IFD overflow")]
    IfdOverflow,
}

impl Dcp {
    /// 从字节流解析
    pub fn parse(bytes: &[u8]) -> Result<Self, DcpError> {
        if bytes.len() < 8 {
            return Err(DcpError::Truncated(8, 0, bytes.len()));
        }
        let order = match &bytes[0..2] {
            b"II" => ByteOrder::Little,
            b"MM" => ByteOrder::Big,
            _ => return Err(DcpError::BadMagic([bytes[0], bytes[1], bytes[2], bytes[3]])),
        };
        let magic = order.u16(&bytes[2..4]);
        if magic != 0x4352 {
            return Err(DcpError::BadMagic([bytes[0], bytes[1], bytes[2], bytes[3]]));
        }
        let ifd_off = order.u32(&bytes[4..8]) as usize;
        if ifd_off + 2 > bytes.len() {
            return Err(DcpError::Truncated(ifd_off + 2, ifd_off, bytes.len()));
        }
        let n = order.u16(&bytes[ifd_off..ifd_off + 2]) as usize;
        let mut entries = BTreeMap::new();
        let mut p = ifd_off + 2;
        for _ in 0..n {
            if p + 12 > bytes.len() {
                return Err(DcpError::Truncated(p + 12, p, bytes.len()));
            }
            let tag = order.u16(&bytes[p..p + 2]);
            let typ = order.u16(&bytes[p + 2..p + 4]);
            let count = order.u32(&bytes[p + 4..p + 8]);
            let val_field_bytes = &bytes[p + 8..p + 12];
            let size = type_size(typ).checked_mul(count).ok_or(DcpError::IfdOverflow)?;
            let data: Vec<u8> = if size <= 4 {
                // 数据就在 value 字段内
                val_field_bytes.to_vec()
            } else {
                let off = order.u32(val_field_bytes) as usize;
                let end = off.checked_add(size as usize).ok_or(DcpError::IfdOverflow)?;
                if end > bytes.len() {
                    return Err(DcpError::Truncated(end, off, bytes.len()));
                }
                bytes[off..end].to_vec()
            };
            entries.insert(tag, TagEntry::new(typ, count, data));
            p += 12;
        }
        Ok(Self { order, entries })
    }

    pub fn read<R: Read>(mut r: R) -> Result<Self, DcpError> {
        let mut buf = Vec::new();
        r.read_to_end(&mut buf).map_err(|_| DcpError::IfdOverflow)?;
        Self::parse(&buf)
    }

    // -------- 取值辅助 --------

    pub fn get_str(&self, tag: u16) -> Option<String> {
        let e = self.entries.get(&tag)?;
        let s = match e.typ {
            TYPE_ASCII => String::from_utf8_lossy(&e.data).trim_end_matches('\0').to_string(),
            TYPE_BYTE => String::from_utf8_lossy(&e.data).trim_end_matches('\0').to_string(),
            _ => return None,
        };
        Some(s)
    }

    pub fn get_srational9(&self, tag: u16) -> Option<[f64; 9]> {
        let e = self.entries.get(&tag)?;
        if e.typ != TYPE_SRATIONAL || e.count != 9 || e.data.len() < 72 {
            return None;
        }
        let mut out = [0f64; 9];
        for i in 0..9 {
            let b = &e.data[i * 8..i * 8 + 8];
            let num = self.order.i32(&b[0..4]);
            let den = self.order.i32(&b[4..8]);
            out[i] = if den == 0 { 0.0 } else { num as f64 / den as f64 };
        }
        Some(out)
    }

    pub fn get_floats(&self, tag: u16) -> Option<Vec<f32>> {
        let e = self.entries.get(&tag)?;
        if e.typ != TYPE_FLOAT || e.data.len() % 4 != 0 {
            return None;
        }
        let n = e.data.len() / 4;
        let mut out = Vec::with_capacity(n);
        for i in 0..n {
            out.push(self.order.f32(&e.data[i * 4..i * 4 + 4]));
        }
        Some(out)
    }

    pub fn get_long(&self, tag: u16) -> Option<u32> {
        let e = self.entries.get(&tag)?;
        if e.typ != TYPE_LONG || e.count < 1 || e.data.len() < 4 {
            return None;
        }
        Some(self.order.u32(&e.data[0..4]))
    }

    pub fn get_long3(&self, tag: u16) -> Option<[u32; 3]> {
        let e = self.entries.get(&tag)?;
        if e.typ != TYPE_LONG || e.count != 3 || e.data.len() < 12 {
            return None;
        }
        Some([
            self.order.u32(&e.data[0..4]),
            self.order.u32(&e.data[4..8]),
            self.order.u32(&e.data[8..12]),
        ])
    }

    pub fn get_short(&self, tag: u16) -> Option<u16> {
        let e = self.entries.get(&tag)?;
        if e.typ != TYPE_SHORT || e.count < 1 || e.data.len() < 2 {
            return None;
        }
        Some(self.order.u16(&e.data[0..2]))
    }

    // -------- 写入 --------

    pub fn set(&mut self, tag: u16, entry: TagEntry) {
        self.entries.insert(tag, entry);
    }

    /// 序列化：重排 IFD + 重算所有数据偏移。
    /// tag 按升序输出（TIFF 要求）。
    pub fn serialize(&self) -> Vec<u8> {
        // 计算 IFD 大小：2 (n) + 12*n + 4 (next IFD ptr)
        let n = self.entries.len();
        let ifd_size = 2 + 12 * n + 4;
        let header_size = 8; // TIFF header
        let mut out = Vec::with_capacity(header_size + ifd_size + 8);

        // TIFF header
        match self.order {
            ByteOrder::Little => out.extend_from_slice(b"II"),
            ByteOrder::Big => out.extend_from_slice(b"MM"),
        }
        let write_u16 = |out: &mut Vec<u8>, v: u16| match self.order {
            ByteOrder::Little => out.extend_from_slice(&v.to_le_bytes()),
            ByteOrder::Big => out.extend_from_slice(&v.to_be_bytes()),
        };
        let write_u32 = |out: &mut Vec<u8>, v: u32| match self.order {
            ByteOrder::Little => out.extend_from_slice(&v.to_le_bytes()),
            ByteOrder::Big => out.extend_from_slice(&v.to_be_bytes()),
        };
        write_u16(&mut out, 0x4352);
        write_u32(&mut out, 8); // IFD offset

        // 先按 tag 升序排好
        let tags: Vec<(u16, &TagEntry)> = self.entries.iter().map(|(k, v)| (*k, v)).collect();

        // 计算每个 tag 的数据是内嵌还是外置
        let mut external_offset: usize = header_size + ifd_size;
        let mut plan: Vec<(u16, &TagEntry, Option<usize>)> = Vec::with_capacity(n); // (tag, entry, external_offset)
        for (tag, e) in &tags {
            let size = type_size(e.typ).saturating_mul(e.count);
            if size <= 4 {
                plan.push((*tag, e, None));
            } else {
                // 4 字节对齐
                let aligned = (external_offset + 3) & !3usize;
                plan.push((*tag, e, Some(aligned)));
                external_offset = aligned + size as usize;
            }
        }

        // IFD
        write_u16(&mut out, n as u16);
        for (tag, e, ext_off) in &plan {
            write_u16(&mut out, *tag);
            write_u16(&mut out, e.typ);
            write_u32(&mut out, e.count);
            let size = type_size(e.typ).saturating_mul(e.count);
            if size <= 4 {
                // 内嵌：数据左对齐填 0
                let mut b = [0u8; 4];
                let keep = std::cmp::min(4, e.data.len());
                b[..keep].copy_from_slice(&e.data[..keep]);
                out.extend_from_slice(&b);
            } else {
                write_u32(&mut out, ext_off.unwrap() as u32);
            }
        }
        write_u32(&mut out, 0); // next IFD = 0

        // 外置数据段
        for (_tag, e, ext_off) in &plan {
            if let Some(off) = ext_off {
                let off = *off;
                while out.len() < off {
                    out.push(0);
                }
                out.extend_from_slice(&e.data);
                // 4 字节对齐 padding
                while out.len() % 4 != 0 {
                    out.push(0);
                }
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "Sony ILCE-7M5 Camera VisionLOG.dcp";

    #[test]
    fn parse_real_dcp() {
        let bytes = std::fs::read(format!("../{SAMPLE}")).expect("sample DCP not found");
        let d = Dcp::parse(&bytes).expect("parse");
        assert_eq!(d.order, ByteOrder::Little);

        let model = d.get_str(tags::UNIQUE_CAMERA_MODEL).expect("model");
        assert_eq!(model, "Sony ILCE-7M5");

        let name = d.get_str(tags::PROFILE_NAME).expect("name");
        assert_eq!(name, "VisionLOG Sony A7M5");

        let cm1 = d.get_srational9(tags::COLOR_MATRIX_1).expect("cm1");
        assert!((cm1[0] - 0.6688).abs() < 1e-4);

        let look_dims = d.get_long3(tags::LOOK_TABLE_DIMS).expect("look_dims");
        assert_eq!(look_dims, [36, 16, 16]);

        let look_data = d.get_floats(tags::LOOK_TABLE_DATA).expect("look_data");
        assert_eq!(look_data.len(), 36 * 16 * 16 * 3);

        let hsm_dims = d.get_long3(tags::HUE_SAT_MAP_DIMS).expect("hsm_dims");
        assert_eq!(hsm_dims, [90, 25, 1]);

        let tc = d.get_floats(tags::PROFILE_TONE_CURVE).expect("tc");
        assert_eq!(tc.len(), 192);
    }

    #[test]
    fn round_trip_preserves_unmodeled_tags() {
        let bytes = std::fs::read(format!("../{SAMPLE}")).expect("sample DCP");
        let d = Dcp::parse(&bytes).expect("parse");
        let serialized = d.serialize();
        // 重新解析应成功且 tag 集合一致
        let d2 = Dcp::parse(&serialized).expect("re-parse");
        assert_eq!(d.entries.len(), d2.entries.len());
        for (tag, e1) in &d.entries {
            let e2 = d2.entries.get(tag).expect("tag missing after round-trip");
            assert_eq!(e1.typ, e2.typ, "type mismatch for tag {tag:#x}");
            assert_eq!(e1.count, e2.count, "count mismatch for tag {tag:#x}");
            assert_eq!(e1.data, e2.data, "data mismatch for tag {tag:#x}");
        }
    }

    #[test]
    fn modify_unique_camera_model() {
        let bytes = std::fs::read(format!("../{SAMPLE}")).expect("sample DCP");
        let mut d = Dcp::parse(&bytes).expect("parse");
        d.set(
            tags::UNIQUE_CAMERA_MODEL,
            TagEntry::ascii_or_byte("Sony ILCE-7RM5", ByteOrder::Little),
        );
        let out = d.serialize();
        let d2 = Dcp::parse(&out).unwrap();
        assert_eq!(d2.get_str(tags::UNIQUE_CAMERA_MODEL).unwrap(), "Sony ILCE-7RM5");
        // 其他 tag 应保持
        assert_eq!(d2.get_str(tags::PROFILE_NAME).unwrap(), "VisionLOG Sony A7M5");
    }
}
