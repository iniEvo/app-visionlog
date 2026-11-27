# -*- mode: python ; coding: utf-8 -*-
"""PyInstaller spec for visionlog engine.

Bundles engine/*.py + numpy into a single-file executable that reads JSON on
stdin and writes JSON events on stdout. Output: visionlog-engine
"""
import os

block_cipher = None

a = Analysis(
    ["engine/main.py"],
    pathex=[os.path.abspath("engine"), os.path.abspath(".")],
    binaries=[],
    datas=[
        ("engine/acr3_curve.txt", "."),
        ("engine/dcp.py", "."),
        ("engine/pipeline.py", "."),
        ("engine/transfer.py", "."),
        ("engine/converter.py", "."),
        ("engine/accuracy.py", "."),
    ],
    hiddenimports=[
        "numpy",
        "numpy.core._methods",
        "numpy.core._dtype_ctypes",
        "engine.dcp",
        "engine.pipeline",
        "engine.transfer",
        "engine.converter",
        "engine.accuracy",
    ],
    hookspath=[],
    hooksconfig={},
    runtime_hooks=[],
    excludes=["tkinter", "matplotlib", "pandas", "scipy", "PIL", "pytest"],
    win_no_prefer_console=True,
    cipher=block_cipher,
    noarchive=False,
)
pyz = PYZ(a.pure, a.zipped_data, cipher=block_cipher)

exe = EXE(
    pyz,
    a.scripts,
    a.binaries,
    a.zipfiles,
    a.datas,
    [],
    name="visionlog-engine",
    debug=False,
    bootloader_ignore_signals=False,
    strip=False,
    upx=True,
    upx_exclude=[],
    runtime_tmpdir=None,
    console=True,
    disable_windowed_traceback=False,
    argv_emulation=False,
    target_arch=None,
    codesign_identity=None,
    entitlements_file=None,
)
