import { invoke } from "@tauri-apps/api/core";

export interface InspectResult {
  path: string;
  unique_camera_model: string | null;
  profile_name: string | null;
  profile_copyright: string | null;
  has_look_table: boolean;
  look_dims: [number, number, number] | null;
  hue_sat_dims: [number, number, number] | null;
  tone_curve_points: number;
  color_matrix1: number[] | null;
  forward_matrix1: number[] | null;
  calibration_illuminant1: number | null;
  tag_count: number;
}

export type MatrixSourceName = "dng" | "adobe_standard" | "lib_raw" | "rawspeed_xml";

export interface CameraEntry {
  model: string;
  make: string;
  short: string;
  aliases: string[];
  std_dcp: string;
}

export interface CameraMatrices {
  unique_camera_model: string;
  cm1: number[];
  cm2: number[] | null;
  fm1: number[] | null;
  fm2: number[] | null;
  ill1: number | null;
  ill2: number | null;
  source: MatrixSourceName;
  std_dcp_path: string | null;
}

export interface InstallResult {
  installed_path: string;
  group_dir: string;
  backup: string | null;
  hint: string;
}

export type EngineEvent =
  | { event: "progress"; stage: string; pct: number }
  | { event: "done"; avg_de: number; max_de: number; unreached: number; out_dcp: string }
  | { event: "error"; msg: string };

export interface TransferRequest {
  src_dcp: string;
  std_dcp: string;
  out_dcp: string;
  src_name: string | null;
}

export interface TransferResult {
  avg_de: number;
  max_de: number;
  unreached: number;
  out_dcp: string;
}

export interface RawCameraInfo {
  make: string;
  model: string;
  unique_camera_model: string;
  is_dng: boolean;
  /** 容器类型：dng / tiff / iso-bmff（CR3） */
  container: string;
}

export const api = {
  inspectDcp: (path: string) => invoke<InspectResult>("inspect_dcp", { path }),
  resolveCameraMatrix: (model: string) =>
    invoke<CameraMatrices>("resolve_camera_matrix", { model }),
  listInstalledCameras: () => invoke<string[]>("list_installed_cameras"),
  renameOnly: (srcDcp: string, targetModel: string, outPath: string) =>
    invoke<string>("rename_only", { srcDcp, targetModel, outPath }),
  installDcp: (dcpPath: string, group: string) =>
    invoke<InstallResult>("install_dcp", { dcpPath, group }),
  openProfileFolder: () => invoke<string>("open_profile_folder"),
  runTransfer: (req: TransferRequest) => invoke<TransferResult>("run_transfer", { req }),
  readRawCamera: (path: string) => invoke<RawCameraInfo>("read_raw_camera", { path }),
  listCameras: () => invoke<CameraEntry[]>("list_cameras"),
  searchCameras: (query: string) => invoke<CameraEntry[]>("search_cameras", { query }),
  defaultLookSource: () => invoke<string | null>("default_look_source"),
  exportDir: () => invoke<string>("export_dir"),
};

/** 当前平台名，用于 UI 文案 */
export const isMac = navigator.userAgent.includes("Mac");
