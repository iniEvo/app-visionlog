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

export default function App() {
  const [s, setS] = useState<State>(initial);
  const update = useCallback((p: Partial<State>) => setS((x) => ({ ...x, ...p })), []);

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

  return (
    <div className="flex h-screen flex-col overflow-hidden p-4 gap-3">
      {/* 顶栏 */}
      <header className="flex items-center justify-between shrink-0">
        <div className="flex items-baseline gap-3">
          <h1 className="text-lg font-medium tracking-tight">visionlog</h1>
          <span className="text-xs text-dim">DCP Look 跨机型真色彩移植</span>
        </div>
        <div className="flex items-center gap-2 text-xs text-dim">
          <span className="badge">{s.cameras.length} 机型</span>
          <span>v0.1.0</span>
        </div>
      </header>

      {s.error && (
        <div
          className="shrink-0 p-2 text-xs"
          style={{
            background: "rgba(224, 91, 91, 0.12)",
            border: "1px solid rgba(224, 91, 91, 0.3)",
            borderRadius: 3,
            color: "#E05B5B",
          }}
        >
          {s.error}
        </div>
      )}

      {/* 主体两栏 */}
      <div className="flex min-h-0 flex-1 gap-3">
        {/* 左栏 · Look 源 + 输出信息 */}
        <div className="flex w-72 shrink-0 flex-col gap-3">
          <section className="panel flex min-h-0 flex-1 flex-col gap-3">
            <h2 className="section-title">Look 源</h2>
            <div className="min-h-0 flex-1">
              {!s.srcInfo ? (
                <div className="dropzone h-full" onClick={onPickSrc}>
                  <div className="text-2xl opacity-40">📂</div>
                  <div className="text-sm">选择 .dcp 文件</div>
                </div>
              ) : (
                <div className="flex h-full flex-col justify-between">
                  <div className="space-y-2 overflow-hidden">
                    {isDefaultVisionLog ? (
                      <>
                        <div className="truncate text-base font-medium">VisionLOG</div>
                        <div className="text-xs text-dim">已加载默认 VisionLOG 配置</div>
                        <div className="text-xs text-dim opacity-70">如需更换点击下方按钮</div>
                      </>
                    ) : (
                      <>
                        <div className="truncate text-base font-medium">{s.srcInfo.profile_name}</div>
                        <div className="text-xs text-dim">原机型 · {s.srcInfo.unique_camera_model}</div>
                        <div className="flex flex-wrap gap-1">
                          {s.srcInfo.look_dims && (
                            <span className="badge">LookTable {s.srcInfo.look_dims.join("×")}</span>
                          )}
                          <span className="badge">{s.srcInfo.tone_curve_points} tone pts</span>
                        </div>
                      </>
                    )}
                  </div>
                  <button className="btn-ghost w-full" onClick={onPickSrc}>
                    {isDefaultVisionLog ? "更换 Look 源" : "更换文件"}
                  </button>
                </div>
              )}
            </div>
          </section>

          {/* 输出信息 · 生成文件 + 安装路径 */}
          <section className="panel flex shrink-0 flex-col gap-2 p-4">
            <h2 className="section-title">输出</h2>
            {s.result ? (
              <div className="space-y-2 text-xs">
                <div>
                  <div className="text-dim mb-0.5">生成文件</div>
                  <div className="truncate font-mono text-[11px]" title={s.result.out_dcp}>
                    {s.result.out_dcp.split("/").pop()}
                  </div>
                  <div className="truncate text-[10px] text-dim opacity-70" title={s.result.out_dcp}>
                    {s.result.out_dcp}
                  </div>
                </div>
                {s.installed ? (
                  <div>
                    <div className="text-dim mb-0.5">Photoshop 安装位置</div>
                    <div className="break-all font-mono text-[10px] text-ok">
                      {s.installed.path}
                    </div>
                  </div>
                ) : (
                  <div className="text-dim opacity-70">尚未安装到 Photoshop</div>
                )}
              </div>
            ) : (
              <div className="text-xs text-dim opacity-70">
                移植完成后在此显示生成文件和安装路径
              </div>
            )}
            {s.installed && (
              <button className="btn-ghost w-full mt-1" onClick={() => void api.openProfileFolder()}>
                打开文件夹
              </button>
            )}
          </section>
        </div>

        {/* 右栏 · 目标机型 */}
        <section className="panel flex min-w-0 flex-1 flex-col gap-2">
          <div className="flex items-center justify-between">
            <h2 className="section-title">目标机型</h2>
            <button className="btn-ghost" onClick={onPickRaw}>
              从 RAW 识别
            </button>
          </div>

          <input
            type="text"
            placeholder="搜索：索尼a7m4 / a7m4 / ILCE-7M4 / EOS R6 …"
            className="w-full px-3 py-2 text-sm outline-none"
            style={{
              background: "rgba(255, 255, 255, 0.02)",
              border: "1px solid var(--border)",
              borderRadius: 3,
              color: "var(--text)",
            }}
            value={s.cameraQuery}
            onChange={(e) => update({ cameraQuery: e.target.value })}
          />

          {s.targetModel && (
            <div className="shrink-0 text-xs">
              <span className="text-dim">已选</span>{" "}
              <span className="text-accent">{s.targetModel}</span>
            </div>
          )}

          <div
            className="min-h-0 flex-1 overflow-y-auto"
            style={{
              border: "1px solid var(--border)",
              borderRadius: 3,
              background: "rgba(0, 0, 0, 0.2)",
            }}
          >
            {s.filtered.map((c) => (
              <button
                key={c.model}
                className={`camera-row ${c.model === s.targetModel ? "camera-row-selected" : ""}`}
                onClick={() => onSelectEntry(c.model)}
              >
                <span>{c.model}</span>
                <span className="text-xs opacity-50">
                  {c.aliases.slice(0, 3).join(" · ")}
                </span>
              </button>
            ))}
            {s.filtered.length === 0 && (
              <div className="p-4 text-center text-sm text-dim">无匹配机型</div>
            )}
          </div>
        </section>
      </div>

      {/* 底部操作栏 */}
      <footer className="flex shrink-0 items-center gap-3">
        {!s.progress && !s.result && (
          <>
            <button className="btn-primary flex-1 py-2.5" disabled={!canGenerate} onClick={onGenerate}>
              生成移植 DCP{s.targetModel ? ` · ${s.targetModel}` : ""}
            </button>
            <button className="btn-ghost" onClick={() => void api.openProfileFolder()}>
              打开 PS 配置目录
            </button>
          </>
        )}

        {s.progress && (
          <div className="flex-1">
            <div className="mb-1 flex justify-between text-xs text-dim">
              <span>{s.progress.stage}</span>
              <span>{s.progress.pct}%</span>
            </div>
            <div className="progress-track">
              <div className="progress-fill" style={{ width: `${s.progress.pct}%` }} />
            </div>
          </div>
        )}

        {s.result && (
          <div className="flex w-full items-center gap-3">
            <div className="flex gap-3 text-center text-xs">
              <div className="px-4 py-2" style={{ background: "#1F1F22", borderRadius: 3 }}>
                <div className="text-dim">平均 ΔE</div>
                <div className="text-lg font-medium text-ok">{s.result.avg_de.toFixed(2)}</div>
              </div>
              <div className="px-4 py-2" style={{ background: "#1F1F22", borderRadius: 3 }}>
                <div className="text-dim">p95 ΔE</div>
                <div className="text-lg font-medium">{s.result.max_de.toFixed(2)}</div>
              </div>
            </div>
            {!s.installed ? (
              <>
                <button className="btn-primary flex-1 py-2.5" onClick={onInstall}>安装到 Photoshop</button>
                <button className="btn-ghost" onClick={resetSession}>再来一份</button>
              </>
            ) : (
              <>
                <div
                  className="min-w-0 flex-1 p-2 text-xs"
                  style={{
                    background: "rgba(107, 201, 160, 0.1)",
                    border: "1px solid rgba(107, 201, 160, 0.3)",
                    borderRadius: 3,
                  }}
                >
                  <span className="font-medium text-ok">已安装</span>
                  <span className="ml-2 text-dim">{s.installed.hint}</span>
                </div>
                <button className="btn-ghost" onClick={resetSession}>再来一份</button>
              </>
            )}
          </div>
        )}
      </footer>
    </div>
  );
}
