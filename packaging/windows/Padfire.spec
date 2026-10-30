# PyInstaller one-folder build. Keeping Qt shared libraries alongside the
# executable makes the bundled LGPL libraries inspectable and replaceable.
from pathlib import Path

root = Path.cwd()
a = Analysis(
    ["padfire_windows.py"],
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
    name="Padfire",
    debug=False,
    bootloader_ignore_signals=False,
    strip=False,
    upx=False,
    console=False,
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
    name="Padfire",
)
