//! 4 层相机矩阵 fallback 链
//! ① 用户 .dng → 直接读 DNG tag
//! ② 本机已装 Adobe Standard DCP → 运行时读，不分发
//! ③ libraw colordata 表（打包为常量，CDDL-1.0）
//! ④ rawspeed cameras.xml（打包资源，CC-BY-SA 3.0，About 署名）

use serde::Serialize;
use std::path::{Path, PathBuf};

use crate::dcp::{tags, Dcp};

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum MatrixSource {
    Dng,
    AdobeStandard,
    LibRaw,
    RawspeedXml,
}

#[derive(Debug, Clone, Serialize)]
pub struct CameraMatrices {
    pub unique_camera_model: String,
    pub cm1: [f64; 9],
    pub cm2: Option<[f64; 9]>,
    pub fm1: Option<[f64; 9]>,
    pub fm2: Option<[f64; 9]>,
    pub ill1: Option<u16>,
    pub ill2: Option<u16>,
    pub source: MatrixSource,
    /// 命中的 Adobe Standard DCP 路径（第②层时）
    pub std_dcp_path: Option<PathBuf>,
}

#[derive(Debug, thiserror::Error)]
pub enum MatrixError {
    #[error("camera '{0}' not found in any matrix source")]
    NotFound(String),
    #[error("failed to parse {0}: {1}")]
    ParseFailed(PathBuf, String),
    #[error("Adobe profile directory not found on this machine")]
    NoAdobeDir,
}

/// 在所有候选路径里找 Adobe Standard DCP
pub fn adobe_standard_search_roots() -> Vec<PathBuf> {
    let mut roots = Vec::new();
    #[cfg(target_os = "macos")]
    {
        if let Ok(home) = std::env::var("HOME") {
            roots.push(PathBuf::from(&home)
                .join("Library/Application Support/Adobe/CameraRaw/CameraProfiles"));
        }
        roots.push(PathBuf::from(
            "/Library/Application Support/Adobe/CameraRaw/CameraProfiles",
        ));
        // Photoshop 内置的 profile 目录
        if let Ok(entries) = std::fs::read_dir("/Applications") {
            for e in entries.flatten() {
                let name = e.file_name().to_string_lossy().to_string();
                if name.starts_with("Adobe Photoshop") {
                    let p = e.path().join("Adobe Photoshop.app/Contents/Resources/CameraProfiles");
                    roots.push(p);
                }
            }
        }
    }
    #[cfg(target_os = "windows")]
    {
        if let Ok(appdata) = std::env::var("APPDATA") {
            roots.push(PathBuf::from(&appdata).join("Adobe/CameraRaw/CameraProfiles"));
        }
        if let Ok(pd) = std::env::var("ProgramData") {
            roots.push(PathBuf::from(&pd).join("Adobe/CameraRaw/CameraProfiles"));
        }
        if let Ok(pf) = std::env::var("ProgramFiles") {
            if let Ok(entries) = std::fs::read_dir(&pf) {
                for e in entries.flatten() {
                    let name = e.file_name().to_string_lossy().to_string();
                    if name.starts_with("Adobe Photoshop") {
                        roots.push(e.path().join("Required/CameraProfiles"));
                    }
                }
            }
        }
    }
    roots
}

/// 在候选根目录下找 "<model> Adobe Standard.dcp"
fn find_adobe_standard_dcp(model: &str) -> Option<PathBuf> {
    let needle = format!("{model} Adobe Standard");
    for root in adobe_standard_search_roots() {
        let std_dir = root.join("Adobe Standard");
        let Ok(entries) = std::fs::read_dir(&std_dir) else { continue };
        let mut best: Option<(PathBuf, u32)> = None;
        for e in entries.flatten() {
            let stem = e.path().extension().map(|x| x == "dcp");
            if stem.is_none() { continue; }
            let name = e.file_name().to_string_lossy().to_string();
            if name.to_lowercase().contains(&needle.to_lowercase()) {
                // v2 优先
                let v2 = if name.to_lowercase().contains(" v2.dcp") { 1u32 } else { 0 };
                if best.as_ref().map(|(_, b)| v2 >= *b).unwrap_or(true) {
                    best = Some((e.path(), v2));
                }
            }
        }
        if let Some((p, _)) = best {
            return Some(p);
        }
    }
    None
}

/// 从 Adobe Standard DCP 抽取矩阵
fn matrices_from_dcp(d: &Dcp) -> Option<CameraMatrices> {
    let model = d.get_str(tags::UNIQUE_CAMERA_MODEL)?;
    let cm1 = d.get_srational9(tags::COLOR_MATRIX_1)?;
    Some(CameraMatrices {
        unique_camera_model: model,
        cm1,
        cm2: d.get_srational9(tags::COLOR_MATRIX_2),
        fm1: d.get_srational9(tags::FORWARD_MATRIX_1),
        fm2: d.get_srational9(tags::FORWARD_MATRIX_2),
        ill1: d.get_short(tags::CALIBRATION_ILLUMINANT_1),
        ill2: d.get_short(tags::CALIBRATION_ILLUMINANT_2),
        source: MatrixSource::AdobeStandard,
        std_dcp_path: None,
    })
}

/// ① DNG 输入直接读矩阵
pub fn matrices_from_dng(path: &Path) -> Result<CameraMatrices, MatrixError> {
    let bytes = std::fs::read(path)
        .map_err(|e| MatrixError::ParseFailed(path.to_owned(), e.to_string()))?;
    let d = Dcp::parse(&bytes)
        .map_err(|e| MatrixError::ParseFailed(path.to_owned(), e.to_string()))?;
    let mut m = matrices_from_dcp(&d)
        .ok_or_else(|| MatrixError::ParseFailed(path.to_owned(), "no matrices".into()))?;
    m.source = MatrixSource::Dng;
    Ok(m)
}

/// 4 层主入口
pub fn resolve_camera(model: &str) -> Result<CameraMatrices, MatrixError> {
    // ② 本机 Adobe Standard
    if let Some(p) = find_adobe_standard_dcp(model) {
        let bytes = std::fs::read(&p)
            .map_err(|e| MatrixError::ParseFailed(p.clone(), e.to_string()))?;
        if let Ok(d) = Dcp::parse(&bytes) {
            if let Some(mut m) = matrices_from_dcp(&d) {
                m.std_dcp_path = Some(p);
                return Ok(m);
            }
        }
    }
    // ③ libraw / ④ rawspeed —— M1 里打包数据后接入
    Err(MatrixError::NotFound(model.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a7m5_resolves_via_adobe_standard() {
        let m = resolve_camera("Sony ILCE-7M5").expect("A7M5 must resolve via layer 2");
        assert_eq!(m.source, MatrixSource::AdobeStandard);
        assert!((m.cm1[0] - 1.1168).abs() < 1e-3); // Adobe 实测值
        assert!(m.fm1.is_some());
        assert_eq!(m.ill1, Some(17)); // D65
        assert!(m.std_dcp_path.is_some());
    }

    #[test]
    fn unknown_model_fails_with_not_found() {
        let r = resolve_camera("NotA RealCamera X1");
        assert!(matches!(r, Err(MatrixError::NotFound(_))));
    }
}
