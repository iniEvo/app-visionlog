//! 从 RAW 文件读相机型号
//!
//! 三类容器：
//! 1. **DNG** —— TIFF 容器，直接读 tag 0xC614 `UniqueCameraModel`（最准确）
//! 2. **TIFF 系 RAW**（ARW / NEF / RAF / ORF / RW2 / CR2）—— 读 IFD0 的
//!    `Make`(0x010F) / `Model`(0x0110)
//! 3. **ISO BMFF 系**（CR3 / Canon HIF）—— `ftyp` 开头，相机型号在
//!    `moov > CMT1`（内含 TIFF IFD0），XMP uuid 作兜底
//!
//! 注意 CR3 不是 TIFF，不能靠 `II`/`MM` 魔数判断。

use std::path::Path;

#[derive(Debug, thiserror::Error)]
pub enum RawMetaError {
    #[error("unsupported format: {0}")]
    Unsupported(String),
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("parse: {0}")]
    Parse(String),
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct RawCameraInfo {
    pub make: String,
    pub model: String,
    /// Adobe UniqueCameraModel 字符串（DCP 匹配键）
    pub unique_camera_model: String,
    pub is_dng: bool,
    /// 容器类型：dng / tiff / iso-bmff
    pub container: String,
}

/// Canon XMP uuid box（`be7acfcb-97a9-42e8-9c71-999491e3afac`）
const XMP_UUID: [u8; 16] = [
    0xbe, 0x7a, 0xcf, 0xcb, 0x97, 0xa9, 0x42, 0xe8,
    0x9c, 0x71, 0x99, 0x94, 0x91, 0xe3, 0xaf, 0xac,
];

const MAX_FILE: u64 = 512 * 1024 * 1024; // 512 MB 上限，防误选巨型文件

/// 从任意 RAW / DNG / CR3 读出相机型号
pub fn read_camera_info(path: &Path) -> Result<RawCameraInfo, RawMetaError> {
    let meta = std::fs::metadata(path)?;
    if meta.len() > MAX_FILE {
        return Err(RawMetaError::Unsupported(format!(
            "文件过大（{} MB），上限 {} MB",
            meta.len() / 1024 / 1024,
            MAX_FILE / 1024 / 1024
        )));
    }
    let bytes = std::fs::read(path)?;
    if bytes.len() < 16 {
        return Err(RawMetaError::Parse("file too small".into()));
    }

    // ① ISO BMFF（CR3）—— 必须先判，因为 CR3 里也可能出现 II/MM 字节
    if is_iso_bmff(&bytes) {
        if let Ok(info) = iso_bmff_camera_info(&bytes) {
            return Ok(info);
        }
        // CR3 解析失败时继续往下试（可能是别的 BMFF 容器）
    }

    // ② DNG —— 有 DNGVersion tag
    if let Ok(d) = crate::dcp::Dcp::parse(&bytes) {
        if d.entries.contains_key(&0xC612) {
            if let Some(ucm) = d.get_str(crate::dcp::tags::UNIQUE_CAMERA_MODEL) {
                let (make, model) = split_model(&ucm);
                return Ok(RawCameraInfo {
                    make,
                    model,
                    unique_camera_model: ucm,
                    is_dng: true,
                    container: "dng".into(),
                });
            }
        }
    }

    // ③ TIFF 系 RAW
    let (make, model) = read_tiff_make_model(&bytes)?;
    let ucm = compose_unique_model(&make, &model);
    Ok(RawCameraInfo {
        make,
        model,
        unique_camera_model: ucm,
        is_dng: false,
        container: "tiff".into(),
    })
}

fn is_iso_bmff(bytes: &[u8]) -> bool {
    bytes.len() >= 12 && &bytes[4..8] == b"ftyp"
}

// ============================================================
// ISO BMFF (CR3)
// ============================================================

/// 遍历 ISO BMFF box 树，收集 CMT1..CMT4 载荷区间与 XMP 载荷区间
fn walk_boxes(
    data: &[u8],
    start: usize,
    end: usize,
    depth: u8,
    cmts: &mut Vec<(usize, usize)>,
    xmp: &mut Option<(usize, usize)>,
) {
    if depth > 8 || start + 8 > end {
        return;
    }
    let mut p = start;
    while p + 8 <= end {
        let size = u32::from_be_bytes([data[p], data[p + 1], data[p + 2], data[p + 3]]) as u64;
        let kind = &data[p + 4..p + 8];
        let (box_size, hdr) = if size == 1 {
            // 64-bit largesize
            if p + 16 > end {
                break;
            }
            let ls = u64::from_be_bytes([
                data[p + 8], data[p + 9], data[p + 10], data[p + 11],
                data[p + 12], data[p + 13], data[p + 14], data[p + 15],
            ]);
            (ls, 16usize)
        } else if size == 0 {
            ((end - p) as u64, 8usize)
        } else {
            (size, 8usize)
        };
        if box_size < hdr as u64 || (p as u64).saturating_add(box_size) > end as u64 {
            break;
        }
        let pay = p + hdr;
        let pay_end = p + box_size as usize;

        match kind {
            b"moov" | b"trak" | b"mdia" | b"minf" | b"stbl" | b"udta" | b"iprp" | b"moof"
            | b"dinf" | b"edts" | b"mvex" => {
                walk_boxes(data, pay, pay_end, depth + 1, cmts, xmp);
            }
            b"meta" => {
                // ISO FullBox：前 4 字节为 version+flags；QuickTime 则直接是子 box
                if pay + 4 <= pay_end {
                    walk_boxes(data, pay + 4, pay_end, depth + 1, cmts, xmp);
                }
            }
            b"CMT1" | b"CMT2" | b"CMT3" | b"CMT4" => {
                cmts.push((pay, pay_end));
            }
            b"uuid" => {
                if pay + 16 <= pay_end && data[pay..pay + 16] == XMP_UUID {
                    *xmp = Some((pay + 16, pay_end));
                }
            }
            _ => {}
        }
        p += box_size as usize;
    }
}

fn iso_bmff_camera_info(bytes: &[u8]) -> Result<RawCameraInfo, RawMetaError> {
    let mut cmts: Vec<(usize, usize)> = Vec::new();
    let mut xmp: Option<(usize, usize)> = None;
    walk_boxes(bytes, 0, bytes.len(), 0, &mut cmts, &mut xmp);

    // ① CMT1..4（CMT1 是 IFD0，优先）
    for (s, e) in &cmts {
        let payload = &bytes[*s..*e];
        if let Some((make, model)) = parse_cmt_payload(payload) {
            let ucm = compose_unique_model(&make, &model);
            return Ok(RawCameraInfo {
                make,
                model,
                unique_camera_model: ucm,
                is_dng: false,
                container: "iso-bmff".into(),
            });
        }
    }

    // ② XMP 兜底
    if let Some((s, e)) = xmp {
        let xml = String::from_utf8_lossy(&bytes[s..e]);
        let make = xmp_field(&xml, "Make");
        let model = xmp_field(&xml, "Model");
        if let (Some(mk), Some(md)) = (make, model) {
            let ucm = compose_unique_model(&mk, &md);
            return Ok(RawCameraInfo {
                make: mk,
                model: md,
                unique_camera_model: ucm,
                is_dng: false,
                container: "iso-bmff".into(),
            });
        }
    }

    Err(RawMetaError::Unsupported(
        "CR3 里没找到 CMT1 元数据或 XMP 型号信息".into(),
    ))
}

/// CMT 载荷可能是：完整 TIFF 流，或裸 IFD（无 TIFF 头）
fn parse_cmt_payload(payload: &[u8]) -> Option<(String, String)> {
    // 完整 TIFF
    if let Some(r) = parse_tiff_stream(payload) {
        return Some(r);
    }
    // 裸 IFD：两种字节序都试，做合理性校验
    for little in [true, false] {
        if let Some(r) = parse_raw_ifd(payload, little) {
            return Some(r);
        }
    }
    None
}

fn xmp_field(xml: &str, field: &str) -> Option<String> {
    // 元素形式：<tiff:Make>Canon</tiff:Make>
    let open = format!("<tiff:{field}>");
    let close = format!("</tiff:{field}>");
    if let Some(i) = xml.find(&open) {
        let rest = &xml[i + open.len()..];
        if let Some(j) = rest.find(&close) {
            let v = rest[..j].trim();
            if !v.is_empty() {
                return Some(v.to_string());
            }
        }
    }
    // 属性形式：tiff:Make="Canon"
    let attr = format!("tiff:{field}=\"");
    if let Some(i) = xml.find(&attr) {
        let rest = &xml[i + attr.len()..];
        if let Some(j) = rest.find('"') {
            let v = rest[..j].trim();
            if !v.is_empty() {
                return Some(v.to_string());
            }
        }
    }
    None
}

// ============================================================
// TIFF
// ============================================================

fn split_model(ucm: &str) -> (String, String) {
    let mut parts = ucm.splitn(2, ' ');
    let make = parts.next().unwrap_or("").to_string();
    let model = parts.next().unwrap_or("").to_string();
    (make, model)
}

/// 拼 Adobe 的 UniqueCameraModel 字符串
fn compose_unique_model(make: &str, model: &str) -> String {
    let m = make.trim();
    let md = model.trim();
    if md.to_lowercase().starts_with(&m.to_lowercase()) {
        md.to_string()
    } else {
        format!("{m} {md}")
    }
}

/// 整份文件是 TIFF（ARW/NEF/RAF/CR2 等）
fn read_tiff_make_model(bytes: &[u8]) -> Result<(String, String), RawMetaError> {
    if let Some(r) = parse_tiff_stream(bytes) {
        return Ok(r);
    }
    Err(RawMetaError::Unsupported(
        "无法读出 Make/Model：既不是 DNG，也不是 TIFF/CR3 容器".into(),
    ))
}

/// 解析以 TIFF 头（II/MM + magic 42）开始的字节流，读 IFD0 的 Make/Model
fn parse_tiff_stream(data: &[u8]) -> Option<(String, String)> {
    if data.len() < 8 {
        return None;
    }
    let little = match &data[0..2] {
        b"II" => true,
        b"MM" => false,
        _ => return None,
    };
    let magic = if little {
        u16::from_le_bytes([data[2], data[3]])
    } else {
        u16::from_be_bytes([data[2], data[3]])
    };
    if magic != 42 {
        return None;
    }
    let ifd_off = if little {
        u32::from_le_bytes([data[4], data[5], data[6], data[7]]) as usize
    } else {
        u32::from_be_bytes([data[4], data[5], data[6], data[7]]) as usize
    };
    parse_ifd(data, ifd_off, little, 0)
}

/// 裸 IFD（无 TIFF 头，偏移相对 payload 起点）
fn parse_raw_ifd(payload: &[u8], little: bool) -> Option<(String, String)> {
    parse_ifd(payload, 0, little, 0)
}

fn parse_ifd(data: &[u8], ifd_off: usize, little: bool, base: usize) -> Option<(String, String)> {
    let u16_at = |b: &[u8]| -> u16 {
        if little { u16::from_le_bytes([b[0], b[1]]) } else { u16::from_be_bytes([b[0], b[1]]) }
    };
    let u32_at = |b: &[u8]| -> u32 {
        if little {
            u32::from_le_bytes([b[0], b[1], b[2], b[3]])
        } else {
            u32::from_be_bytes([b[0], b[1], b[2], b[3]])
        }
    };

    if ifd_off + 2 > data.len() {
        return None;
    }
    let n = u16_at(&data[ifd_off..ifd_off + 2]) as usize;
    if n == 0 || n > 512 {
        return None;
    }
    let mut make = String::new();
    let mut model = String::new();
    let mut p = ifd_off + 2;
    for _ in 0..n {
        if p + 12 > data.len() {
            break;
        }
        let tag = u16_at(&data[p..p + 2]);
        let typ = u16_at(&data[p + 2..p + 4]);
        let count = u32_at(&data[p + 4..p + 8]) as usize;
        if (tag == 0x010F || tag == 0x0110) && typ == 2 && count > 0 && count < 4096 {
            let value_bytes: &[u8] = if count <= 4 {
                &data[p + 8..p + 8 + count.min(4)]
            } else {
                let off = u32_at(&data[p + 8..p + 12]) as usize;
                let start = off.checked_add(base)?;
                let end = start.checked_add(count)?;
                if end > data.len() {
                    p += 12;
                    continue;
                }
                &data[start..end]
            };
            let s = String::from_utf8_lossy(value_bytes)
                .trim_end_matches('\0')
                .trim()
                .to_string();
            // 合理性：可打印、非空
            if !s.is_empty() && s.chars().all(|c| !c.is_control()) {
                if tag == 0x010F {
                    make = s;
                } else {
                    model = s;
                }
            }
        }
        p += 12;
    }
    if make.is_empty() && model.is_empty() {
        return None;
    }
    Some((make, model))
}

#[cfg(test)]
mod tests {
    use super::*;

    // ---------- 测试样本构造 ----------

    fn boxed(kind: &[u8; 4], payload: &[u8]) -> Vec<u8> {
        let mut v = Vec::with_capacity(8 + payload.len());
        v.extend_from_slice(&((8 + payload.len()) as u32).to_be_bytes());
        v.extend_from_slice(kind);
        v.extend_from_slice(payload);
        v
    }

    /// 构造最小 TIFF 流（IFD0 只有 Make + Model 两个 ASCII tag）
    fn make_tiff(make: &str, model: &str, little: bool) -> Vec<u8> {
        let mut w = |v: &mut Vec<u8>, x: u16| {
            if little { v.extend_from_slice(&x.to_le_bytes()) } else { v.extend_from_slice(&x.to_be_bytes()) }
        };
        let mut l = |v: &mut Vec<u8>, x: u32| {
            if little { v.extend_from_slice(&x.to_le_bytes()) } else { v.extend_from_slice(&x.to_be_bytes()) }
        };
        let mut out = Vec::new();
        out.extend_from_slice(if little { b"II" } else { b"MM" });
        w(&mut out, 42);
        l(&mut out, 8); // IFD 偏移

        // IFD: 2 entries + next=0
        let ifd_start = out.len();
        w(&mut out, 2);
        let data_start = ifd_start + 2 + 2 * 12 + 4;

        // tag 0x010F Make
        w(&mut out, 0x010F);
        w(&mut out, 2); // ASCII
        l(&mut out, (make.len() + 1) as u32);
        l(&mut out, data_start as u32);

        // tag 0x0110 Model
        w(&mut out, 0x0110);
        w(&mut out, 2);
        l(&mut out, (model.len() + 1) as u32);
        l(&mut out, (data_start + make.len() + 1) as u32);

        l(&mut out, 0); // next IFD

        out.extend_from_slice(make.as_bytes());
        out.push(0);
        out.extend_from_slice(model.as_bytes());
        out.push(0);
        out
    }

    fn make_cr3(make: &str, model: &str, little: bool) -> Vec<u8> {
        let ftyp = boxed(b"ftyp", b"crx \x00\x00\x00\x00crx crx ");
        let tiff = make_tiff(make, model, little);
        let cmt1 = boxed(b"CMT1", &tiff);
        let moov = boxed(b"moov", &cmt1);
        let mut out = ftyp;
        out.extend_from_slice(&moov);
        out
    }

    fn make_cr3_xmp_only(make: &str, model: &str) -> Vec<u8> {
        let ftyp = boxed(b"ftyp", b"crx \x00\x00\x00\x00crx ");
        let xml = format!(
            r#"<x:xmpmeta xmlns:x="adobe:ns:meta/"><rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#"><rdf:Description xmlns:tiff="http://ns.adobe.com/tiff/1.0/" tiff:Make="{make}" tiff:Model="{model}"/></rdf:RDF></x:xmpmeta>"#
        );
        let mut uuid_payload = XMP_UUID.to_vec();
        uuid_payload.extend_from_slice(xml.as_bytes());
        let uuid_box = boxed(b"uuid", &uuid_payload);
        let moov = boxed(b"moov", &uuid_box);
        let mut out = ftyp;
        out.extend_from_slice(&moov);
        out
    }

    // ---------- 测试 ----------

    #[test]
    fn cr3_little_endian_cmt1() {
        let bytes = make_cr3("Canon", "Canon EOS R5", true);
        assert!(is_iso_bmff(&bytes));
        let info = iso_bmff_camera_info(&bytes).expect("must parse");
        assert_eq!(info.make, "Canon");
        assert_eq!(info.model, "Canon EOS R5");
        assert_eq!(info.unique_camera_model, "Canon EOS R5");
        assert_eq!(info.container, "iso-bmff");
    }

    #[test]
    fn cr3_big_endian_cmt1() {
        let bytes = make_cr3("Canon", "Canon EOS R6 Mark II", false);
        let info = iso_bmff_camera_info(&bytes).expect("must parse");
        assert_eq!(info.unique_camera_model, "Canon EOS R6 Mark II");
    }

    #[test]
    fn cr3_sony_model_already_prefixed() {
        let bytes = make_cr3("SONY", "SONY ILCE-7M5", true);
        let info = iso_bmff_camera_info(&bytes).unwrap();
        assert_eq!(info.unique_camera_model, "SONY ILCE-7M5");
    }

    #[test]
    fn cr3_xmp_fallback() {
        let bytes = make_cr3_xmp_only("Canon", "Canon EOS R3");
        let info = iso_bmff_camera_info(&bytes).expect("XMP fallback");
        assert_eq!(info.unique_camera_model, "Canon EOS R3");
    }

    #[test]
    fn cr3_without_metadata_errors_cleanly() {
        let mut bytes = boxed(b"ftyp", b"crx \x00\x00\x00\x00crx ");
        bytes.extend_from_slice(&boxed(b"moov", &boxed(b"mdat", b"\x00\x01\x02\x03")));
        assert!(iso_bmff_camera_info(&bytes).is_err());
    }

    #[test]
    fn cr3_64bit_largesize_box() {
        // 用 size==1 + largesize 的大 box
        let tiff = make_tiff("Canon", "Canon EOS R1", true);
        let cmt1_payload = &tiff;
        let mut cmt1 = Vec::new();
        cmt1.extend_from_slice(&1u32.to_be_bytes());
        cmt1.extend_from_slice(b"CMT1");
        cmt1.extend_from_slice(&((16 + cmt1_payload.len()) as u64).to_be_bytes());
        cmt1.extend_from_slice(cmt1_payload);

        let mut moov = Vec::new();
        moov.extend_from_slice(&1u32.to_be_bytes());
        moov.extend_from_slice(b"moov");
        moov.extend_from_slice(&((16 + cmt1.len()) as u64).to_be_bytes());
        moov.extend_from_slice(&cmt1);

        let mut bytes = boxed(b"ftyp", b"crx \x00\x00\x00\x00crx ");
        bytes.extend_from_slice(&moov);
        let info = iso_bmff_camera_info(&bytes).expect("largesize box");
        assert_eq!(info.unique_camera_model, "Canon EOS R1");
    }

    #[test]
    fn dcp_reads_unique_camera_model() {
        let p = std::path::Path::new("../Sony ILCE-7M5 Camera VisionLOG.dcp");
        let bytes = std::fs::read(p).unwrap();
        let d = crate::dcp::Dcp::parse(&bytes).unwrap();
        assert_eq!(
            d.get_str(crate::dcp::tags::UNIQUE_CAMERA_MODEL).unwrap(),
            "Sony ILCE-7M5"
        );
    }

    #[test]
    fn compose_model_avoids_duplicate_make() {
        assert_eq!(compose_unique_model("SONY", "ILCE-7M5"), "SONY ILCE-7M5");
        assert_eq!(compose_unique_model("SONY", "SONY ILCE-7M5"), "SONY ILCE-7M5");
        assert_eq!(compose_unique_model("Canon", "EOS R5"), "Canon EOS R5");
        assert_eq!(compose_unique_model("Canon", "Canon EOS R5"), "Canon EOS R5");
    }

    #[test]
    fn raw_ifd_without_tiff_header() {
        // 裸 IFD（无 II/MM 头）也要能解析 —— off 相对 payload 起点
        let mut out = Vec::new();
        out.extend_from_slice(&2u16.to_le_bytes()); // count
        let data_start = 2 + 2 * 12 + 4;
        out.extend_from_slice(&0x010Fu16.to_le_bytes());
        out.extend_from_slice(&2u16.to_le_bytes());
        out.extend_from_slice(&6u32.to_le_bytes());
        out.extend_from_slice(&(data_start as u32).to_le_bytes());
        out.extend_from_slice(&0x0110u16.to_le_bytes());
        out.extend_from_slice(&2u16.to_le_bytes());
        out.extend_from_slice(&7u32.to_le_bytes()); // "EOS R5\0"
        out.extend_from_slice(&((data_start + 6) as u32).to_le_bytes());
        out.extend_from_slice(&0u32.to_le_bytes());
        out.extend_from_slice(b"Canon\0");
        out.extend_from_slice(b"EOS R5\0");
        let r = parse_cmt_payload(&out).expect("raw IFD");
        assert_eq!(r.0, "Canon");
        assert_eq!(r.1, "EOS R5");
    }
}
