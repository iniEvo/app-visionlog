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
/// - 内容相同则直接跳过，不产生备份
/// - 内容不同则备份到 `<group>/.backup/` 后再覆盖
pub fn install(src_dcp: &Path, group: &str) -> Result<InstallResult, InstallError> {
    let root = user_profile_root()?;
    install_into(&root, src_dcp, group)
}

fn install_into(root: &Path, src_dcp: &Path, group: &str) -> Result<InstallResult, InstallError> {
    let group_dir = root.join(group);
    std::fs::create_dir_all(&group_dir)?;
    cleanup_legacy_backups(&group_dir);

    let file_name = src_dcp
        .file_name()
        .ok_or_else(|| InstallError::Io(std::io::Error::new(
            std::io::ErrorKind::InvalidInput, "src has no file name")))?;
    let dest = group_dir.join(file_name);

    let mut backup = None;
    if dest.exists() {
        if files_equal(&dest, src_dcp)? {
            return Ok(InstallResult {
                installed_path: dest,
                group_dir,
                backup: None,
                hint: INSTALL_HINT.into(),
            });
        }
        let backup_dir = group_dir.join(".backup");
        std::fs::create_dir_all(&backup_dir)?;
        let bak = backup_dir.join(format!("{}.bak", file_name.to_string_lossy()));
        let _ = std::fs::remove_file(&bak);
        std::fs::rename(&dest, &bak)?;
        backup = Some(bak);
    }
    std::fs::copy(src_dcp, &dest)?;

    Ok(InstallResult {
        installed_path: dest,
        group_dir,
        backup,
        hint: INSTALL_HINT.into(),
    })
}

const INSTALL_HINT: &str =
    "重启 Photoshop / Lightroom 后，在 Camera Raw 的「配置文件」列表里选择新生成的 profile。";

fn files_equal(a: &Path, b: &Path) -> Result<bool, InstallError> {
    if std::fs::metadata(a)?.len() != std::fs::metadata(b)?.len() {
        return Ok(false);
    }
    Ok(std::fs::read(a)? == std::fs::read(b)?)
}

/// 旧版本会把 `.bak` 直接放在 profile 目录里；统一收进 `.backup/`。
fn cleanup_legacy_backups(group_dir: &Path) {
    let Ok(entries) = std::fs::read_dir(group_dir) else { return };
    let backup_dir = group_dir.join(".backup");
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_file() { continue; }
        if !entry.file_name().to_string_lossy().ends_with(".bak") { continue; }
        if std::fs::create_dir_all(&backup_dir).is_err() { continue; }
        let dest = backup_dir.join(entry.file_name());
        let _ = std::fs::rename(&path, &dest);
    }
}

/// 启动时把已安装目录里的旧 `.bak` 文件收进 `.backup/`
pub fn cleanup_legacy_backups_for(group: &str) {
    if let Ok(root) = user_profile_root() {
        cleanup_legacy_backups(&root.join(group));
    }
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
    fn installs_and_dedupes_with_backups_in_hidden_dir() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("CameraProfiles");
        let src = tmp.path().join("Fujifilm X-T30 II VisionLOG.dcp");
        std::fs::write(&src, b"IIRCv1").unwrap();

        let r1 = install_into(&root, &src, "VisionLOG").unwrap();
        assert!(r1.installed_path.exists());
        assert!(r1.backup.is_none());

        // 相同内容再装一次：不产生备份
        let r2 = install_into(&root, &src, "VisionLOG").unwrap();
        assert!(r2.backup.is_none());

        // 内容变化：旧文件进 .backup
        std::fs::write(&src, b"IIRCv2").unwrap();
        let r3 = install_into(&root, &src, "VisionLOG").unwrap();
        let bak = r3.backup.expect("backup");
        assert_eq!(bak.parent().unwrap().file_name().unwrap().to_string_lossy(), ".backup");
        assert_eq!(std::fs::read(&r3.installed_path).unwrap(), b"IIRCv2");
        assert_eq!(std::fs::read(&bak).unwrap(), b"IIRCv1");
    }

    #[test]
    fn user_root_is_non_empty_on_this_platform() {
        let r = user_profile_root();
        assert!(r.is_ok(), "should resolve a user profile root");
    }
}
