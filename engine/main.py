#!/usr/bin/env python3
"""visionlog engine — DCP look 移植引擎（fork of extra-sauce, MIT）

协议：stdin 一行 JSON 请求，stdout 逐行 JSON 事件。

请求: {"src_dcp": "...", "std_dcp": "...", "out_dcp": "...", "src_name": null}
事件:
  {"event": "progress", "stage": "...", "pct": N}
  {"event": "done", "avg_de": F, "max_de": F, "unreached": N, "out_dcp": "..."}
  {"event": "error", "msg": "..."}
"""

import json
import sys
import time
import types
import importlib.util
from pathlib import Path

PKG = Path(__file__).resolve().parent

# extra-sauce 模块内部用包相对导入（from .dcp import ...），
# 注册为 "engine" 包再逐个加载。
_pkg = types.ModuleType("engine")
_pkg.__path__ = [str(PKG)]
sys.modules.setdefault("engine", _pkg)


def _load(name):
    spec = importlib.util.spec_from_file_location(f"engine.{name}", PKG / f"{name}.py")
    mod = importlib.util.module_from_spec(spec)
    sys.modules[f"engine.{name}"] = mod
    spec.loader.exec_module(mod)
    return mod


dcp = _load("dcp")
converter = _load("converter")  # converter 内部会 import pipeline/transfer


def emit(obj):
    sys.stdout.write(json.dumps(obj) + "\n")
    sys.stdout.flush()


def std_name(path: Path) -> str:
    """'Sony ILCE-7M5 Adobe Standard.dcp' -> 'Sony ILCE-7M5'"""
    import re
    m = re.match(r"^(.+?) Adobe Standard( v2)?\.dcp$", path.name, re.I)
    return m.group(1) if m else path.stem


def main():
    line = sys.stdin.readline()
    if not line.strip():
        emit({"event": "error", "msg": "no request on stdin"})
        return
    req = json.loads(line)
    src_dcp = Path(req["src_dcp"])
    std_dcp = Path(req["std_dcp"])
    out_dcp = Path(req["out_dcp"])
    src_name = req.get("src_name")

    t0 = time.time()

    try:
        # 源 look 自己机型的 Adobe Standard（prepare 的测量基准）
        src_info = dcp.read_dcp(src_dcp)
        src_camera = src_info.get("UniqueCameraModel")
        if not src_camera:
            raise RuntimeError(f"{src_dcp.name}: DCP 里没有 UniqueCameraModel，不是相机 look 配置文件")

        all_stds = converter.adobe_standard_cameras()
        src_std = all_stds.get(src_camera)
        if src_std is None:
            raise RuntimeError(
                f"找不到 {src_camera} 的 Adobe Standard DCP（本机 Photoshop/Lightroom 未覆盖该机型），"
                f"无法建立移植基准"
            )

        emit({"event": "progress", "stage": "prepare", "pct": 5})
        # prepare: 解析源 look + 基于其原生机型抽取 LookTable（build_look + fit_look + 自检）
        src = converter.prepare(src_dcp, cameras=all_stds)

        emit({"event": "progress", "stage": "transfer", "pct": 60})
        # write_profile: 目标机型的 Adobe Standard 矩阵为底 + 新 LookTable/ToneCurve/Encoding=1
        converter.write_profile(src, std_dcp, out_dcp, profile_name=src_name or src.name)

        emit({"event": "progress", "stage": "verify", "pct": 92})
        emit({
            "event": "done",
            "avg_de": round(src.accuracy_mean, 3),
            "max_de": round(src.accuracy_p95, 3),  # p95，比单点 max 更有统计意义
            "unreached": int(src.filled_pct),
            "out_dcp": str(out_dcp),
            "elapsed_s": round(time.time() - t0, 1),
        })
    except Exception as e:
        emit({"event": "error", "msg": f"{type(e).__name__}: {e}"})


if __name__ == "__main__":
    main()
