//! visionlog — DCP look 移植工具
//! 把任意相机 look（.dcp）真色彩移植到任意目标机型，并一键装进 Photoshop ACR。

pub mod cameras;
pub mod commands;
pub mod dcp;
pub mod install;
pub mod matrix;
pub mod rawmeta;
pub mod sidecar;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            // 清理旧版留在 CameraProfiles 里的生成目录与 .bak 文件
            commands::migrate_output_dir(app.handle());
            install::cleanup_legacy_backups_for("VisionLOG");
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::inspect_dcp,
            commands::resolve_camera_matrix,
            commands::list_installed_cameras,
            commands::rename_only,
            commands::install_dcp,
            commands::open_profile_folder,
            commands::list_cameras,
            commands::search_cameras,
            commands::default_look_source,
            commands::read_raw_camera,
            commands::export_dir,
            commands::run_transfer,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
