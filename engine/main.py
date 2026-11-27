#!/usr/bin/env python3
"""visionlog engine — DCP look 移植引擎（fork of extra-sauce, MIT）

PyInstaller 打包说明：
- spec 的 hiddenimports 写 "engine.dcp" / "engine.pipeline" 等
- spec 的 datas 把 engine/*.py 放在解包根目录
- 运行时 sys._MEIPASS 指向解包目录，engine/ 目录也在此
"""

import importlib.util
import json
import sys
import time
import types
from pathlib import Path

# PyInstaller 打包后 _MEIPASS = 解包目录；源码运行时 = engine/ 所在目录
if hasattr(sys, "_MEIPASS"):
    _BASE = Path(sys._MEIPASS)
else:
    _BASE = Path(__file__).resolve().parent

# 注册 "engine" 包让 extra-sauce 的相对导入 `from .dcp import ...` 生效
_pkg = types.ModuleType("engine")
_pkg.__path__ = [str(_BASE)]
sys.modules["engine"] = _pkg


def _load(name):
    path = _BASE / f"{name}.py"
    spec = importlib.util.spec_from_file_location(f"engine.{name}", path)
    mod = importlib.util.module_from_spec(spec)
    sys.modules[f"engine.{name}"] = mod
    spec.loader.exec_module(mod)
    return mod


dcp = _load("dcp")
converter = _load("converter")  # 内部链式加载 pipeline/transfer/accuracy


def emit(obj):
    sys.stdout.write(json.dumps(obj) + "\n")
    sys.stdout.flush()


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
        src_info = dcp.read_dcp(src_dcp)
        src_camera = src_info.get("UniqueCameraModel")
        if not src_camera:
            raise RuntimeError(f"{src_dcp.name}: DCP 里没有 UniqueCameraModel")

        all_stds = converter.adobe_standard_cameras()
        src_std = all_stds.get(src_camera)
        if src_std is None:
            raise RuntimeError(
                f"找不到 {src_camera} 的 Adobe Standard DCP（本机 Photoshop/Lightroom 未覆盖该机型）"
            )

        emit({"event": "progress", "stage": "prepare", "pct": 5})
        src = converter.prepare(src_dcp, cameras=all_stds)

        emit({"event": "progress", "stage": "transfer", "pct": 60})
        converter.write_profile(src, std_dcp, out_dcp, profile_name=src_name or src.name)

        emit({"event": "progress", "stage": "verify", "pct": 92})
        emit({
            "event": "done",
            "avg_de": round(src.accuracy_mean, 3),
            "max_de": round(src.accuracy_p95, 3),
            "unreached": int(src.filled_pct),
            "out_dcp": str(out_dcp),
            "elapsed_s": round(time.time() - t0, 1),
        })
    except Exception as e:
        emit({"event": "error", "msg": f"{type(e).__name__}: {e}"})


if __name__ == "__main__":
    main()
