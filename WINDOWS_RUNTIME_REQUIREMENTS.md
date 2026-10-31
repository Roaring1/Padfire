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
The exact wheel is published on PyPI with a verified Publish Attestation:
- Wheel: `numpy-2.5.4-cp312-cp312-win_amd64.whl`
- PyPI-published SHA-256: `a7b1b6353e36a7e50de2973a38d705c88ee93adcf120673cee7f45a4a3fa223a`
- Publisher repository/workflow: `numpy/numpy-release`, Windows CPython 3.12 x64 job in run
  [38074635821](https://github.com/numpy/numpy-release/actions/runs/38074635821)
  (job 114278931605); its logs record source ref `maintenance/2.5.x`.
- The Padfire build pins NumPy 2.5.4, captures pip's `--report`, and fails unless
  the resolved wheel filename, `files.pythonhosted.org` URL and SHA-256 match
  that PyPI-attested file. The generated manifest records those values.

The attestation establishes the publisher/workflow identity for this exact
wheel; it is not, by itself, a complete reproducible-build record for every
compiler flag or environment input. The NumPy PE import rewrite is a
downstream Padfire build modification and is separately recorded by the
before/after importer hashes below.
The patch is a build modification and must be re-audited when NumPy or
delvewheel changes. This is a packaging technique, not legal advice or a
Microsoft licensing determination.
