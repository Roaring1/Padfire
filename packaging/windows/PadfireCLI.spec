# Console companion, shipped as a replaceable one-folder distribution.
from pathlib import Path

root = Path.cwd()
a = Analysis(
    ["padfire_windows_cli.py"],
    pathex=[str(root)],
    binaries=[],
    datas=[],
    hiddenimports=[
        "mido.backends.rtmidi",
        "rtmidi",
        "sounddevice",
        "soundfile",
        "pyloudnorm",
        "PySide6.QtCore",
        "PySide6.QtGui",
        "PySide6.QtWidgets",
        "PySide6.QtNetwork",
    ],
    hookspath=[],
    hooksconfig={},
    runtime_hooks=[],
    excludes=["tkinter", "matplotlib", "pytest"],
    noarchive=False,
    optimize=1,
)
pyz = PYZ(a.pure)
exe = EXE(
    pyz,
    a.scripts,
    [],
    exclude_binaries=True,
    name="PadfireCLI",
    debug=False,
    bootloader_ignore_signals=False,
    strip=False,
    upx=False,
    console=True,
    disable_windowed_traceback=False,
    argv_emulation=False,
    target_arch=None,
    codesign_identity=None,
    entitlements_file=None,
)
coll = COLLECT(
    exe,
    a.binaries,
    a.datas,
    strip=False,
    upx=False,
    upx_exclude=[],
    name="PadfireCLI",
)
