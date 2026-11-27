import { useCallback, useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { api, type EngineEvent, type InspectResult, type CameraEntry } from "./ipc";

type Step = "ready" | "running" | "done";

interface State {
  step: Step;
  srcInfo: InspectResult | null;
  cameras: CameraEntry[];
  filtered: CameraEntry[];
  cameraQuery: string;
  targetModel: string;
  targetStd: string | null;
  progress: { stage: string; pct: number } | null;
  result: { avg_de: number; max_de: number; unreached: number; out_dcp: string } | null;
  installed: { path: string; hint: string } | null;
  error: string | null;
}

const initial: State = {
  step: "ready",
  srcInfo: null,
  cameras: [],
  filtered: [],
  cameraQuery: "",
  targetModel: "",
  targetStd: null,
  progress: null,
  result: null,
  installed: null,
  error: null,
};

function baseName(path: string) {
  return path.split(/[\\/]/).pop() || path;
}

function dirName(path: string) {
  const i = Math.max(path.lastIndexOf("/"), path.lastIndexOf("\\"));
  return i > 0 ? path.slice(0, i) : path;
}

const STEPS = ["Look 源", "目标机型", "生成安装"];

const STAGE_TEXT: Record<string, string> = {
  prepare: "解析源配置 · 阶段 1 / 3",
  transfer: "拟合 LookTable · 阶段 2 / 3",
  verify: "ΔE 质量自检 · 阶段 3 / 3",
};

/* ============================ 图标 ============================ */
function IconAperture() {
  return (
    <svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="#fff" strokeWidth="1.7" strokeLinecap="round">
      <circle cx="12" cy="12" r="8.2" />
      <path d="M12 3.8v5.1M20.2 12h-5.1M12 20.2v-5.1M3.8 12h5.1" />
      <circle cx="12" cy="12" r="2.1" fill="#fff" stroke="none" />
    </svg>
  );
}

function IconCheck({ size = 10 }: { size?: number }) {
  return (
    <svg width={size} height={size} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="3.4" strokeLinecap="round">
      <path d="M20 6 9 17l-5-5" />
    </svg>
  );
}

function IconSearch() {
  return (
    <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.2" strokeLinecap="round">
      <circle cx="11" cy="11" r="7" />
      <path d="m20 20-3.2-3.2" />
    </svg>
  );
}

function IconCamera() {
  return (
    <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round">
      <path d="M4 8h3l1.5-2h7L17 8h3a1 1 0 0 1 1 1v9a1 1 0 0 1-1 1H4a1 1 0 0 1-1-1V9a1 1 0 0 1 1-1Z" />
      <circle cx="12" cy="13" r="3.4" />
    </svg>
  );
}

function IconArrow() {
  return (
    <svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.4" strokeLinecap="round" strokeLinejoin="round">
      <path d="M5 12h14M13 6l6 6-6 6" />
    </svg>
  );
}

function IconFile() {
  return (
    <svg width="22" height="22" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.7" strokeLinecap="round" strokeLinejoin="round">
      <path d="M14 3H7a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h10a2 2 0 0 0 2-2V8Z" />
      <path d="M14 3v5h5" />
    </svg>
  );
}

/* ============================ 应用 ============================ */
export default function App() {
  const [s, setS] = useState<State>(initial);
  const [copied, setCopied] = useState<string | null>(null);
  const update = useCallback((p: Partial<State>) => setS((x) => ({ ...x, ...p })), []);

  const copyPath = useCallback(async (path: string) => {
    try {
      await navigator.clipboard.writeText(path);
      setCopied(path);
      setTimeout(() => setCopied(null), 1500);
    } catch {
      // 剪贴板权限被拒时静默失败
    }
  }, []);

  const loadDefaultLook = useCallback(async () => {
    try {
      const p = await api.defaultLookSource();
      if (!p) return;
      const info = await api.inspectDcp(p);
      update({ srcInfo: info });
    } catch (e) {
      console.warn("default look load failed", e);
    }
  }, [update]);

  useEffect(() => {
    void loadDefaultLook();
    api.listCameras().then((list) => update({ cameras: list, filtered: list.slice(0, 500) })).catch(() => {});
  }, [update, loadDefaultLook]);

  const resetSession = useCallback(() => {
    setS((prev) => ({
      ...prev,
      step: "ready",
      progress: null,
      result: null,
      installed: null,
      error: null,
      targetModel: "",
      targetStd: null,
      cameraQuery: "",
      filtered: prev.cameras.slice(0, 500),
    }));
    if (!s.srcInfo) void loadDefaultLook();
  }, [loadDefaultLook, s.srcInfo]);

  useEffect(() => {
    const q = s.cameraQuery.trim();
    if (!q) {
      update({ filtered: s.cameras.slice(0, 500) });
      return;
    }
    const timer = setTimeout(() => {
      api.searchCameras(q).then((list) => update({ filtered: list })).catch(() => {});
    }, 200);
    return () => clearTimeout(timer);
  }, [s.cameraQuery, s.cameras, update]);

  useEffect(() => {
    const un = listen<EngineEvent>("engine-event", (e) => {
      const ev = e.payload;
      if (ev.event === "progress") update({ progress: { stage: ev.stage, pct: ev.pct } });
      else if (ev.event === "done")
        update({
          step: "done",
          progress: null,
          result: { avg_de: ev.avg_de, max_de: ev.max_de, unreached: ev.unreached, out_dcp: ev.out_dcp },
        });
      else if (ev.event === "error") update({ step: "ready", progress: null, error: ev.msg });
    });
    return () => { void un.then((f) => f()); };
  }, [update]);

  /* 相机搜索防抖，交给 ipc 端做模糊匹配 */

  async function pickFile(extensions: string[], name: string) {
    const mod = await import("@tauri-apps/plugin-dialog");
    const picked = await mod.open({ multiple: false, filters: [{ name, extensions }] });
    if (!picked || typeof picked !== "string") return null;
    return picked;
  }

  async function onPickSrc() {
    const p = await pickFile(["dcp"], "DCP 相机配置文件");
    if (!p) return;
    try {
      const info = await api.inspectDcp(p);
      update({ srcInfo: info, error: null });
    } catch (e) { update({ error: String(e) }); }
  }

  async function onPickRaw() {
    const p = await pickFile(["dng", "arw", "cr3", "nef", "raf", "orf", "rw2"], "RAW / DNG 样片");
    if (!p) return;
    try {
      const info = await api.readRawCamera(p);
      await onSelectEntry(info.unique_camera_model);
      update({ error: null });
    } catch (e) { update({ error: `读 RAW 型号失败：${e}` }); }
  }

  async function onSelectEntry(model: string) {
    const entry = s.cameras.find((c) => c.model === model);
    try {
      if (entry) {
        await api.resolveCameraMatrix(model);
        update({ targetModel: model, targetStd: entry.std_dcp });
      } else {
        update({ targetModel: model });
      }
    } catch (e) { update({ targetModel: model, error: String(e) }); }
  }

  async function onGenerate() {
    if (!s.srcInfo || !s.targetModel || !s.targetStd) return;
    update({ step: "running", progress: { stage: "init", pct: 0 }, error: null, result: null, installed: null });
    try {
      const dir = await api.exportDir();
      const safe = s.targetModel.replace(/[/\\:]/g, "_");
      await api.runTransfer({
        src_dcp: s.srcInfo.path,
        std_dcp: s.targetStd,
        out_dcp: `${dir}/${safe} VisionLOG.dcp`,
        src_name: s.srcInfo.profile_name,
      });
    } catch (e) { update({ step: "ready", error: String(e) }); }
  }

  async function onInstall() {
    if (!s.result) return;
    try {
      const r = await api.installDcp(s.result.out_dcp, "VisionLOG");
      update({ installed: { path: r.installed_path, hint: r.hint } });
    } catch (e) { update({ error: String(e) }); }
  }

  const canGenerate = !!s.srcInfo && !!s.targetModel && !!s.targetStd && s.step === "ready";
  const isDefaultVisionLog = !!s.srcInfo?.profile_name && /visionlog/i.test(s.srcInfo.profile_name);

  /* 步骤轨：已完成的步骤打勾，当前步骤高亮 */
  const stepDone = [!!s.srcInfo, !!s.srcInfo && !!s.targetStd, !!s.result];
  const activeIndex = stepDone.findIndex((v) => !v);

  return (
    <div className="app">
      {/* 顶栏 */}
      <header className="app-head">
        <div className="brand">
          <div className="logo"><IconAperture /></div>
          <div className="brand-txt">
            <div className="brand-name">
              visionlog <span className="beta">BETA</span>
            </div>
            <div className="brand-sub">DCP Look 跨机型真色彩移植</div>
          </div>
        </div>
        <div className="head-right">
          <span className="pill"><i />{s.cameras.length.toLocaleString()} 台机型已索引</span>
          <span className="pill pink">离线运行</span>
          <span className="ver">v0.1.0</span>
        </div>
      </header>

      {/* 步骤轨 */}
      <div className="steps">
        {STEPS.map((label, i) => (
          <div key={label} style={{ display: "contents" }}>
            <div
              className={`step${stepDone[i] ? " done" : ""}${activeIndex === i ? " on" : ""}`}
            >
              <span className="step-no">{stepDone[i] ? <IconCheck /> : i + 1}</span>
              <span className="step-label">{label}</span>
            </div>
            {i < STEPS.length - 1 && (
              <div className={`step-line${stepDone[i] ? " filled" : ""}`} />
            )}
          </div>
        ))}
      </div>

      {s.error && <div className="error-bar">{s.error}</div>}

      {/* 主体 */}
      <main className="main">
        {/* 左栏 */}
        <aside className="col-left">
          <section className="card look-card">
            <div className="card-head">
              <div className="card-title"><span className="bar" />Look 源</div>
              <div className="head-actions">
                {isDefaultVisionLog && <span className="pill pink"><i />默认配置</span>}
                {s.srcInfo && (
                  <button type="button" className="btn btn-ghost btn-xs" onClick={onPickSrc}>
                    {isDefaultVisionLog ? "更换 Look 源" : "更换文件"}
                  </button>
                )}
              </div>
            </div>
            <div className="card-body">
              {!s.srcInfo ? (
                <button type="button" className="dropzone" onClick={onPickSrc}>
                  <span className="ic"><IconFile /></span>
                  <span className="t1">选择 .dcp 文件</span>
                  <span className="t2">拖入或点击导入 Look 配置文件</span>
                </button>
              ) : (
                <div className="look-file">
                  <div className="file-badge">
                    <IconCheck size={10} />
                    {isDefaultVisionLog ? "VISIONLOG" : "CUSTOM LOOK"}
                  </div>
                  <div>
                    <div className="file-name">
                      {isDefaultVisionLog ? "VisionLOG" : s.srcInfo.profile_name || "未命名 Look"}
                    </div>
                    <div className="file-meta">
                      {isDefaultVisionLog
                        ? "已加载默认 VisionLOG 配置"
                        : `原机型 · ${s.srcInfo.unique_camera_model ?? "未知"}`}
                    </div>
                    {isDefaultVisionLog && !s.installed && (
                      <div className="file-hint">如需更换点击右上角「更换 Look 源」</div>
                    )}
                  </div>
                  <div className="look-spec">
                    {isDefaultVisionLog && (
                      <div className="look-spec-row">
                        <span className="k">原机型</span>
                        <span className="v">{s.srcInfo.unique_camera_model ?? "未知"}</span>
                      </div>
                    )}
                    <div className="look-spec-row">
                      <span className="k">配置文件</span>
                      <span className="v">{baseName(s.srcInfo.path)}</span>
                    </div>
                    <div className="look-spec-row">
                      <span className="k">DCP 标签</span>
                      <span className="v">{s.srcInfo.tag_count} tags</span>
                    </div>
                  </div>
                  <div className="look-foot">
                    {!s.installed && (
                      <div className="look-note">
                        <span className="look-note-dot" />
                        <span>源文件只读解析，移植结果写入新 .dcp</span>
                      </div>
                    )}
                    <div className="chips">
                      {s.srcInfo.look_dims && (
                        <span className="chip blue">LookTable {s.srcInfo.look_dims.join("×")}</span>
                      )}
                      <span className="chip">{s.srcInfo.tone_curve_points} tone pts</span>
                    </div>
                  </div>
                </div>
              )}
            </div>
          </section>

          {/* 输出 */}
          <section className="card out-card">
            <div className="card-head">
              <div className="card-title"><span className="bar" />输出</div>
              {s.installed && (
                <button
                  type="button"
                  className="btn btn-ghost btn-sm"
                  style={{ height: 24, padding: "0 9px", fontSize: 10 }}
                  onClick={() => void api.openProfileFolder()}
                >
                  打开文件夹
                </button>
              )}
            </div>
            <div className="card-body" style={{ paddingTop: 8 }}>
              {s.result ? (
                <div className="space-y-1">
                  <button type="button" className="out-row" onClick={() => void copyPath(s.result!.out_dcp)}>
                    <span className="out-k">
                      <span>生成文件</span>
                      <span className="copy">{copied === s.result!.out_dcp ? "已复制" : "点击复制"}</span>
                    </span>
                    <span className="out-v">{baseName(s.result.out_dcp)}</span>
                  </button>
                  <button
                    type="button"
                    className="out-row"
                    onClick={() => void copyPath(dirName(s.result!.out_dcp))}
                  >
                    <span className="out-k">
                      <span>生成位置</span>
                      <span className="copy">
                        {copied === dirName(s.result!.out_dcp) ? "已复制" : "点击复制"}
                      </span>
                    </span>
                    <span className="out-v wrap">{dirName(s.result.out_dcp)}</span>
                  </button>
                  {s.installed ? (
                    <>
                      <button type="button" className="out-row" onClick={() => void copyPath(s.installed!.path)}>
                        <span className="out-k">
                          <span>安装位置</span>
                          <span className="copy">{copied === s.installed!.path ? "已复制" : "点击复制"}</span>
                        </span>
                        <span className="out-v ok wrap">{s.installed.path}</span>
                      </button>
                    </>
                  ) : (
                    <div className="out-empty" style={{ padding: "4px 10px" }}>尚未安装到 Photoshop</div>
                  )}
                </div>
              ) : (
                <div className="out-empty">
                  移植完成后，这里会显示生成文件、生成位置与 Photoshop 安装位置。
                </div>
              )}
            </div>
          </section>
        </aside>

        {/* 右栏 · 目标机型 */}
        <section className="card camera-card">
          <div className="card-head">
            <div className="card-title"><span className="bar" />目标机型</div>
            <button type="button" className="btn-raw" onClick={onPickRaw}>
              <IconCamera />从 RAW 识别
            </button>
          </div>
          <div className="card-body">
            <div className="search">
              <IconSearch />
              <input
                type="text"
                placeholder="搜索：索尼a7m5 / a7m5 / ILCE-7M5 / EOS R5 …"
                value={s.cameraQuery}
                onChange={(e) => update({ cameraQuery: e.target.value })}
              />
            </div>

            <div className="selected-line">
              <span className="lab">已选</span>
              <span className={`val${s.targetModel ? "" : " empty"}`}>
                {s.targetModel || "尚未选择机型"}
              </span>
              {s.targetModel && (
                <span className="src">
                  {s.targetStd ? "矩阵来源 · Adobe Standard" : "矩阵尚未解析"}
                </span>
              )}
            </div>

            <div className="camera-list">
              {s.filtered.map((c, i) => (
                <button
                  type="button"
                  key={c.model}
                  className={`cam${c.model === s.targetModel ? " sel" : ""}`}
                  style={{ animationDelay: `${Math.min(i * 0.015, 0.3)}s` }}
                  onClick={() => onSelectEntry(c.model)}
                >
                  <span className="cam-body">
                    <span className="m">{c.model}</span>
                    <span className="a">{c.aliases.slice(0, 3).join(" · ")}</span>
                  </span>
                  {c.std_dcp && <span className="src-tag">Adobe Std</span>}
                  <span className="tick"><IconCheck /></span>
                </button>
              ))}
              {s.filtered.length === 0 && (
                <div className="out-empty" style={{ padding: "24px 0", textAlign: "center" }}>
                  无匹配机型
                </div>
              )}
            </div>

            <div className="matrix-note">
              <span>矩阵 fallback</span>
              <span className="chain">
                <span className="node">DNG</span>
                <span>→</span>
                <span className={`node${s.targetStd ? " hit" : ""}`}>Adobe Std</span>
                <span>→</span>
                <span className="node">LibRaw</span>
                <span>→</span>
                <span className="node">Rawspeed</span>
              </span>
            </div>
          </div>
        </section>
      </main>

      {/* 底栏 */}
      {s.progress ? (
        <footer className="foot">
          <div className="progress-wrap">
            <div className="progress-top">
              <span className="stage">
                <span className="spin" />
                {STAGE_TEXT[s.progress.stage] ?? s.progress.stage}
              </span>
              <span className="pct">{s.progress.pct}%</span>
            </div>
            <div className="progress-track">
              <div className="progress-fill" style={{ width: `${s.progress.pct}%` }} />
            </div>
          </div>
        </footer>
      ) : s.result ? (
        <footer className="foot">
          <div className="metrics">
            <div className="metric blue">
              <div className="k">平均 ΔE</div>
              <div className="v">{s.result.avg_de.toFixed(2)}</div>
            </div>
            <div className="metric pink">
              <div className="k">P95 ΔE</div>
              <div className="v">{s.result.max_de.toFixed(2)}</div>
            </div>
            <div className="metric warn">
              <div className="k">未达节点</div>
              <div className="v">{s.result.unreached}<span className="u">个</span></div>
            </div>
          </div>

          {s.installed ? (
            <>
              <div className="success">
                <span className="ico"><IconCheck size={13} /></span>
                <div style={{ minWidth: 0 }}>
                  <div className="tt">已安装到 Photoshop</div>
                  <div className="ss">{s.installed.hint}</div>
                </div>
              </div>
              <button type="button" className="btn btn-ghost" onClick={() => void api.openProfileFolder()}>
                打开文件夹
              </button>
              <button type="button" className="btn btn-ghost" onClick={resetSession}>再来一份</button>
            </>
          ) : (
            <>
              <button type="button" className="btn btn-primary" onClick={onInstall}>
                安装到 Photoshop
              </button>
              <button type="button" className="btn btn-ghost" onClick={resetSession}>再来一份</button>
            </>
          )}
        </footer>
      ) : (
        <footer className="foot">
          <button type="button" className="btn btn-primary" disabled={!canGenerate} onClick={onGenerate}>
            生成移植 DCP
            {s.targetModel && <span className="model">· {s.targetModel}</span>}
            <IconArrow />
          </button>
          <button type="button" className="btn btn-ghost" onClick={() => void api.openProfileFolder()}>
            打开 PS 配置目录
          </button>
        </footer>
      )}
    </div>
  );
}
