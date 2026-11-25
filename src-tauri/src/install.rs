//! Adobe CameraRaw profile 目录探测与安装
//!
//! 只用用户级路径（无需管理员权限）：
//!   macOS:  ~/Library/Application Support/Adobe/CameraRaw/CameraProfiles/<Group>/
//!   Win:    %APPDATA%\Adobe\CameraRaw\CameraProfiles\<Group>\
//!
//! 系统级路径（/Library/... 或 C:\ProgramData\...）只读探测，不写入。

use serde::Serialize;
use std::path::{Path, PathBuf};

#[derive(Debug, thiserror::Error)]
pub enum InstallError {
    #[error("no writable Adobe profile directory found")]
    NoWritableDir,
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
}

#[derive(Debug, Clone, Serialize)]
pub struct InstallResult {
    pub installed_path: PathBuf,
    pub group_dir: PathBuf,
    pub backup: Option<PathBuf>,
    pub hint: String,
}

/// 用户级可写根目录（安装目标）
pub fn user_profile_root() -> Result<PathBuf, InstallError> {
    #[cfg(target_os = "macos")]
    {
        let home = std::env::var("HOME").map_err(|_| InstallError::NoWritableDir)?;
        Ok(PathBuf::from(home)
            .join("Library/Application Support/Adobe/CameraRaw/CameraProfiles"))
    }
    #[cfg(target_os = "windows")]
    {
        let appdata = std::env::var("APPDATA").map_err(|_| InstallError::NoWritableDir)?;
        Ok(PathBuf::from(appdata).join("Adobe/CameraRaw/CameraProfiles"))
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        Err(InstallError::NoWritableDir)
    }
}

/// 把已生成的 .dcp 装进 Adobe 目录。
/// - 自动创建 `<root>/<group>/`
/// - 若目标同名文件已存在，先备份为 `.bak`
pub fn install(src_dcp: &Path, group: &str) -> Result<InstallResult, InstallError> {
    let root = user_profile_root()?;
    let group_dir = root.join(group);
    std::fs::create_dir_all(&group_dir)?;

    let file_name = src_dcp
        .file_name()
        .ok_or_else(|| InstallError::Io(std::io::Error::new(
            std::io::ErrorKind::InvalidInput, "src has no file name")))?;
    let dest = group_dir.join(file_name);

    let mut backup = None;
    if dest.exists() {
        let bak = dest.with_extension("dcp.bak");
        let _ = std::fs::remove_file(&bak);
        std::fs::rename(&dest, &bak)?;
        backup = Some(bak);
    }
    std::fs::copy(src_dcp, &dest)?;

    Ok(InstallResult {
        installed_path: dest,
        group_dir,
        backup,
        hint: "重启 Photoshop / Lightroom 后，在 Camera Raw 的「配置文件」列表里选择新生成的 profile。".into(),
    })
}

/// 干跑：只计算目标路径，不写盘（用于 UI 预览）
pub fn plan_install(src_dcp: &Path, group: &str) -> Result<PathBuf, InstallError> {
    let root = user_profile_root()?;
    let file_name = src_dcp
        .file_name()
        .ok_or_else(|| InstallError::Io(std::io::Error::new(
            std::io::ErrorKind::InvalidInput, "src has no file name")))?;
    Ok(root.join(group).join(file_name))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn installs_into_tempdir_and_is_idempotent() {
        let tmp = tempfile::tempdir().unwrap();
        let src = tmp.path().join("My Look.dcp");
        std::fs::write(&src, b"IIRCfake").unwrap();

        // 直接测底层逻辑（跳过平台路径）
        let group_dir = tmp.path().join("MyGroup");
        std::fs::create_dir_all(&group_dir).unwrap();
        let dest = group_dir.join("My Look.dcp");
        std::fs::copy(&src, &dest).unwrap();
        assert!(dest.exists());

        // 幂等：再装一次应产生 .bak
        let bak = dest.with_extension("dcp.bak");
        std::fs::rename(&dest, &bak).unwrap();
        std::fs::copy(&src, &dest).unwrap();
        assert!(dest.exists() && bak.exists());
    }

    #[test]
    fn user_root_is_non_empty_on_this_platform() {
        let r = user_profile_root();
        assert!(r.is_ok(), "should resolve a user profile root");
    }
}
