//! Python 移植引擎 sidecar 进程管理
//!
//! 协议：Rust 启动 python engine/main.py，stdin 写 JSON 请求，
//! stdout 逐行读 JSON 事件（progress / done / error）。
//! M1 阶段先用系统 python3 跑通，M4 打包 python-build-standalone。

use serde::{Deserialize, Serialize};
use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use tauri::Emitter;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransferRequest {
    pub src_dcp: PathBuf,
    pub std_dcp: PathBuf, // 目标机型的 Adobe Standard DCP
    pub out_dcp: PathBuf,
    pub src_name: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum EngineEvent {
    Progress { stage: String, pct: u32 },
    Done {
        avg_de: f64,
        max_de: f64,
        unreached: u32,
        out_dcp: PathBuf,
    },
    Error { msg: String },
}

#[derive(Debug, thiserror::Error)]
pub enum SidecarError {
    #[error("python3 not found: {0}")]
    PythonNotFound(String),
    #[error("engine script not found: {0}")]
    ScriptNotFound(PathBuf),
    #[error("engine failed: {0}")]
    EngineFailed(String),
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
}

fn engine_root() -> PathBuf {
    // dev: <repo>/engine；打包后：resource dir/engine
    let dev = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../engine");
    if dev.exists() {
        return dev;
    }
    PathBuf::from("engine")
}

/// 查找打包好的 visionlog-engine 二进制 sidecar
///
/// dev: src-tauri/binaries/visionlog-engine
/// 打包: 同可执行目录或 Resources/
fn packaged_engine_binary() -> Option<PathBuf> {
    let names = if cfg!(target_os = "windows") {
        &["visionlog-engine.exe"][..]
    } else {
        &["visionlog-engine"][..]
    };

    let mut candidates: Vec<PathBuf> = Vec::new();

    // dev: src-tauri/binaries/<name>
    let dev = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("binaries");
    for n in names {
        candidates.push(dev.join(n));
    }

    // 打包后：当前 exe 同目录
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            for n in names {
                candidates.push(dir.join(n));
            }
            // macOS: Contents/MacOS/visionlog → Contents/Resources/binaries/<name>
            if let Some(resources) = dir.parent().map(|c| c.join("Resources")) {
                for n in names {
                    candidates.push(resources.join(n));
                    candidates.push(resources.join("binaries").join(n));
                }
            }
        }
    }

    // macOS: <Bundle>/Contents/Resources/visionlog-engine
    // Linux/Windows: 已覆盖（exe 同目录或 Resources）
    candidates.into_iter().find(|p| p.exists())
}

/// 跑一次移植（同进程异步包装），由 commands.rs 用 #[tauri::command] 再包装
pub async fn run_transfer(
    _app: tauri::AppHandle,
    window: tauri::Window,
    req: TransferRequest,
) -> Result<serde_json::Value, String> {
    run_transfer_blocking(window, req).await.map_err(|e| e.to_string())
}

async fn run_transfer_blocking(
    window: tauri::Window,
    req: TransferRequest,
) -> Result<serde_json::Value, SidecarError> {
    // 交给阻塞线程跑，事件即时 emit
    let (tx, rx) = mpsc::channel::<EngineEvent>();
    let req_clone = req.clone();
    let handle = std::thread::spawn(move || -> Result<EngineEvent, SidecarError> {
        run_engine(&req_clone, move |ev| { let _ = tx.send(ev.clone()); Ok(()) })
    });

    // 主线程转发事件到前端
    while let Ok(ev) = rx.recv() {
        let _ = window.emit("engine-event", &ev);
        if let EngineEvent::Done { .. } | EngineEvent::Error { .. } = &ev {
            break;
        }
    }

    let result = handle.join().map_err(|_| SidecarError::EngineFailed("thread panicked".into()))??;
    match result {
        EngineEvent::Done { avg_de, max_de, unreached, out_dcp } => Ok(serde_json::json!({
            "avg_de": avg_de, "max_de": max_de, "unreached": unreached, "out_dcp": out_dcp
        })),
        EngineEvent::Error { msg } => Err(SidecarError::EngineFailed(msg)),
        _ => unreachable!(),
    }
}

/// 同步跑引擎，stdout 每行一个 JSON 事件
///
/// 优先用打包的 visionlog-engine 二进制（安装即用，无 Python 依赖）；
/// 回退到源码运行（dev 环境，需要 python3 + numpy）。
pub fn run_engine<F>(req: &TransferRequest, mut on_event: F) -> Result<EngineEvent, SidecarError>
where
    F: FnMut(EngineEvent) -> Result<(), SidecarError> + Send + 'static,
{
    let payload = serde_json::to_string(req)
        .map_err(|e| SidecarError::EngineFailed(e.to_string()))?;

    // 优先打包二进制
    let mut cmd: Command = if let Some(bin) = packaged_engine_binary() {
        Command::new(bin)
    } else {
        // 回退：源码 + python
        let script = engine_root().join("main.py");
        if !script.exists() {
            return Err(SidecarError::ScriptNotFound(script));
        }
        let venv = engine_root().join(if cfg!(target_os = "windows") {
            ".venv/Scripts/python.exe"
        } else {
            ".venv/bin/python"
        });
        let py = if venv.exists() {
            venv.to_string_lossy().to_string()
        } else if cfg!(target_os = "windows") {
            "python".to_string()
        } else {
            "python3".to_string()
        };
        let mut c = Command::new(py);
        c.arg(&script);
        c
    };

    let mut child: Child = cmd
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| SidecarError::PythonNotFound(e.to_string()))?;

    // 写请求
    if let Some(stdin) = child.stdin.as_mut() {
        let _ = stdin.write_all(payload.as_bytes());
        let _ = stdin.write_all(b"\n");
        let _ = stdin.flush();
    }
    drop(child.stdin.take());

    // 读事件
    let stdout = child.stdout.take()
        .ok_or_else(|| SidecarError::EngineFailed("no stdout".into()))?;
    let reader = BufReader::new(stdout);
    let mut last = None;
    for line in reader.lines() {
        let line = line?;
        let line = line.trim();
        if line.is_empty() { continue; }
        if let Ok(ev) = serde_json::from_str::<EngineEventSerde>(line) {
            let ev: EngineEvent = ev.into();
            on_event(ev.clone())?;
            let stop = matches!(ev, EngineEvent::Done { .. } | EngineEvent::Error { .. });
            last = Some(ev);
            if stop { break; }
        }
    }
    let stderr_out = child
        .stderr
        .take()
        .map(|mut s| {
            let mut buf = String::new();
            let _ = std::io::Read::read_to_string(&mut s, &mut buf);
            buf
        })
        .unwrap_or_default();
    let _ = child.wait();

    last.ok_or_else(|| {
        SidecarError::EngineFailed(format!("engine exited without terminal event; stderr: {stderr_out}"))
    })
}

// serde 直接 derive 在带 non-string 字段的 enum 上，需要 tag; 用中间结构
#[derive(Debug, Deserialize)]
struct EngineEventSerde {
    event: String,
    #[serde(default)]
    stage: Option<String>,
    #[serde(default)]
    pct: Option<u32>,
    #[serde(default)]
    avg_de: Option<f64>,
    #[serde(default)]
    max_de: Option<f64>,
    #[serde(default)]
    unreached: Option<u32>,
    #[serde(default)]
    out_dcp: Option<PathBuf>,
    #[serde(default)]
    msg: Option<String>,
}

impl From<EngineEventSerde> for EngineEvent {
    fn from(s: EngineEventSerde) -> Self {
        match s.event.as_str() {
            "progress" => EngineEvent::Progress {
                stage: s.stage.unwrap_or_default(),
                pct: s.pct.unwrap_or(0),
            },
            "done" => EngineEvent::Done {
                avg_de: s.avg_de.unwrap_or_default(),
                max_de: s.max_de.unwrap_or_default(),
                unreached: s.unreached.unwrap_or_default(),
                out_dcp: s.out_dcp.unwrap_or_default(),
            },
            _ => EngineEvent::Error { msg: s.msg.unwrap_or_else(|| "unknown engine error".into()) },
        }
    }
}
