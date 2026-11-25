//! Tauri IPC 命令集合。lib.rs 只负责组装，命令逻辑都在这里。

use serde::Serialize;
use std::path::PathBuf;

use crate::dcp;
use crate::install;
use crate::matrix;
use crate::{cameras, rawmeta, sidecar};

#[derive(Debug, Serialize)]
pub struct InspectResult {
    pub path: PathBuf,
    pub unique_camera_model: Option<String>,
    pub profile_name: Option<String>,
    pub profile_copyright: Option<String>,
    pub has_look_table: bool,
    pub look_dims: Option<[u32; 3]>,
    pub hue_sat_dims: Option<[u32; 3]>,
    pub tone_curve_points: usize,
    pub color_matrix1: Option<[f64; 9]>,
    pub forward_matrix1: Option<[f64; 9]>,
    pub calibration_illuminant1: Option<u16>,
    pub tag_count: usize,
}

#[tauri::command]
pub fn inspect_dcp(path: String) -> Result<InspectResult, String> {
    let bytes = std::fs::read(&path).map_err(|e| format!("读文件失败: {e}"))?;
    let d = dcp::Dcp::parse(&bytes).map_err(|e| format!("不是有效的 DCP: {e}"))?;
    let tone = d
        .get_floats(dcp::tags::PROFILE_TONE_CURVE)
        .map(|v| v.len() / 2)
        .unwrap_or(0);
    Ok(InspectResult {
        path: PathBuf::from(&path),
        unique_camera_model: d.get_str(dcp::tags::UNIQUE_CAMERA_MODEL),
        profile_name: d.get_str(dcp::tags::PROFILE_NAME),
        profile_copyright: d.get_str(dcp::tags::PROFILE_COPYRIGHT),
        has_look_table: d.entries.contains_key(&dcp::tags::LOOK_TABLE_DATA),
        look_dims: d.get_long3(dcp::tags::LOOK_TABLE_DIMS),
        hue_sat_dims: d.get_long3(dcp::tags::HUE_SAT_MAP_DIMS),
        tone_curve_points: tone,
        color_matrix1: d.get_srational9(dcp::tags::COLOR_MATRIX_1),
        forward_matrix1: d.get_srational9(dcp::tags::FORWARD_MATRIX_1),
        calibration_illuminant1: d.get_short(dcp::tags::CALIBRATION_ILLUMINANT_1),
        tag_count: d.entries.len(),
    })
}

#[tauri::command]
pub fn resolve_camera_matrix(model: String) -> Result<matrix::CameraMatrices, String> {
    matrix::resolve_camera(&model).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn list_installed_cameras() -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for root in matrix::adobe_standard_search_roots() {
        let std_dir = root.join("Adobe Standard");
        let Ok(entries) = std::fs::read_dir(&std_dir) else { continue };
        for e in entries.flatten() {
            let name = e.file_name().to_string_lossy().to_string();
            if let Some(model) = name
                .strip_suffix(" Adobe Standard.dcp")
                .or_else(|| name.strip_suffix(" Adobe Standard v2.dcp"))
            {
                if !out.iter().any(|m| m == model) {
                    out.push(model.to_string());
                }
            }
        }
    }
    out.sort();
    out.dedup();
    out
}

#[tauri::command]
pub fn rename_only(
    src_dcp: String,
    target_model: String,
    out_path: String,
) -> Result<String, String> {
    let bytes = std::fs::read(&src_dcp).map_err(|e| format!("读源文件失败: {e}"))?;
    let mut d = dcp::Dcp::parse(&bytes).map_err(|e| format!("解析失败: {e}"))?;
    d.set(
        dcp::tags::UNIQUE_CAMERA_MODEL,
        dcp::TagEntry::ascii_or_byte(&target_model, d.order),
    );
    std::fs::write(&out_path, d.serialize()).map_err(|e| format!("写入失败: {e}"))?;
    Ok(out_path)
}

#[tauri::command]
pub fn install_dcp(dcp_path: String, group: String) -> Result<install::InstallResult, String> {
    install::install(std::path::Path::new(&dcp_path), &group).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn open_profile_folder() -> Result<String, String> {
    let root = install::user_profile_root().map_err(|e| e.to_string())?;
    let _ = std::fs::create_dir_all(&root);
    #[cfg(target_os = "macos")]
    let status = std::process::Command::new("open").arg(&root).status();
    #[cfg(target_os = "windows")]
    let status = std::process::Command::new("explorer").arg(&root).status();
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    let status = Err(std::io::Error::new(std::io::ErrorKind::Unsupported, "unsupported"));

    status.map_err(|e| e.to_string())?;
    Ok(root.to_string_lossy().to_string())
}

/// 主流厂商机型列表（带中文/简称别名，供前端搜索）
#[tauri::command]
pub fn list_cameras() -> Vec<cameras::CameraEntry> {
    cameras::list_cameras()
}

/// 按查询过滤机型（'索尼a7m4' / 'a7m4' / 'A7 IV' 都能命中）
#[tauri::command]
pub fn search_cameras(query: String) -> Vec<cameras::CameraEntry> {
    let tokens = cameras::normalize_query(&query);
    if tokens.iter().all(|t| t.is_empty()) {
        return cameras::list_cameras();
    }
    cameras::list_cameras()
        .into_iter()
        .filter(|e| cameras::matches(e, &tokens))
        .collect()
}

/// 默认 look 源（工作目录里的 VisionLOG DCP，A7M5 优先）
#[tauri::command]
pub fn default_look_source() -> Option<String> {
    cameras::default_look_source().map(|p| p.to_string_lossy().to_string())
}

/// 读 RAW/DNG 样片的相机型号
#[tauri::command]
pub fn read_raw_camera(path: String) -> Result<rawmeta::RawCameraInfo, String> {
    rawmeta::read_camera_info(std::path::Path::new(&path)).map_err(|e| e.to_string())
}

/// 移植输出的默认目录（导出前给用户选保存位置时的起点）
#[tauri::command]
pub fn export_dir() -> Result<String, String> {
    let root = install::user_profile_root().map_err(|e| e.to_string())?;
    let tmp = root.join("visionlog-out");
    std::fs::create_dir_all(&tmp).map_err(|e| e.to_string())?;
    Ok(tmp.to_string_lossy().to_string())
}

/// 前端可调用：跑一次移植，事件实时推给前端
#[tauri::command]
pub async fn run_transfer(
    app: tauri::AppHandle,
    window: tauri::Window,
    req: sidecar::TransferRequest,
) -> Result<serde_json::Value, String> {
    sidecar::run_transfer(app, window, req).await
}
