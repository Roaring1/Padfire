# PyInstaller one-folder build. Keeping Qt shared libraries alongside the
# executable makes the bundled LGPL libraries inspectable and replaceable.
from pathlib import Path
from PyInstaller.utils.hooks import collect_data_files, collect_dynamic_libs

root = Path.cwd()

# PortAudio and libsndfile ship in platform-specific private data packages.
# These are loaded via CFFI/ctypes, so make the DLL resources explicit.
binaries = collect_dynamic_libs("_sounddevice_data") + collect_dynamic_libs("_soundfile_data")
datas = collect_data_files("_sounddevice_data", excludes=["**/*.dll"])
datas += collect_data_files("_soundfile_data", excludes=["**/*.dll"])
datas += [(str(root / "LICENSE"), "."), (str(root / "THIRD_PARTY_NOTICES.md"), ".")]
a = Analysis(
    [str(root / "padfire_windows.py")],
    pathex=[str(root)],
    binaries=binaries,
    datas=datas,
    hiddenimports=[
        "mido.backends.rtmidi",
        "rtmidi",
        "sounddevice",
        "soundfile",
        "_sounddevice_data",
        "_soundfile_data",
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
