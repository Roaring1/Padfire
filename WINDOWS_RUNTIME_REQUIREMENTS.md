# Windows runtime prerequisite

The native Windows x64 build requires the Microsoft Visual C++ x64
Redistributable version 14.44.35211.0 or newer. Install the latest supported
x64 package from Microsoft's official page before launching Padfire:

https://learn.microsoft.com/en-us/cpp/windows/latest-supported-vc-redist?view=msvc-170

The Inno Setup installer checks the 64-bit Windows runtime registration under
HKLM\SOFTWARE\Microsoft\VisualStudio\14.0\VC\Runtimes\x64. If the required
runtime is missing or too old, setup stops and offers to open Microsoft's
download page. The installer remains per-user; it does not silently install or
update a system-wide runtime.

The portable ZIP has no setup-time prerequisite check. Users of the portable
build must install the same Microsoft runtime before starting Padfire.exe.

## Why the runtime is external

Qt's Qt 6.12 Windows deployment guide says loose compiler runtime DLLs picked
up from a development machine are not intended or licensed for redistribution,
and directs distributors to use Microsoft's official Visual C++ Redistributable.
Padfire's post-build normalizer removes ordinary MSVCP140, VCRUNTIME140, and
ucrtbase.dll copies from both GUI and CLI bundles. The build checks for the
official x64 system redistributable and the installer checks for version
14.44.35211.0 or later.

This is backed by a disposable-copy test on a Windows 11 laptop with system
VC++ runtime 14.51.36247.00: all 33 ordinary app-local MSVC/UCRT DLL copies
were removed from temporary GUI/CLI copies, Padfire launched offscreen and
published working Windows IPC status, and the temporary copies were deleted.
The existing installation and its profile were not touched.

## NumPy wheel runtime import patch

The upstream NumPy Windows wheel used by this build includes a private,
hash-renamed copy of msvcp140.dll in numpy.libs. Two unsigned NumPy extension
modules import that private filename. The build now uses the pinned
delvewheel 1.13.2 PE import-rewrite command to change those two import names
to the standard system name msvcp140.dll, verifies that all PE import tables
refer to the standard name, then removes the hash-renamed wheel DLL before
PyInstaller packaging.

The script records the source DLL SHA-256, importer file paths and before/after
SHA-256 values, and the exact rewrite tool version in
ThirdPartyLicenses/WINDOWS_RUNTIME_PATCHES.md. It also performs fresh-process
NumPy and SciPy numerical smoke tests after deleting the original wheel DLL.
The hash-renamed DLL is not shipped in either Padfire distribution.

This is a deterministic PE import-name patch to NumPy's binary extension
modules; it does not modify NumPy source code or the Microsoft runtime DLL.
The original PyPI wheel has no build provenance attestation, so the project
pins and records its resolved distribution version, keeps the official wheel
checksum in build logs, and retains the script/manifest as evidence.
The patch is a build modification and must be re-audited when NumPy or
delvewheel changes. This is a packaging technique, not legal advice or a
Microsoft licensing determination.
