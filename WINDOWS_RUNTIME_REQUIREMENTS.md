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
Padfire's post-build normalizer therefore removes ordinary MSVCP140,
VCRUNTIME140, and ucrtbase.dll copies from both GUI and CLI bundles. Windows
10 1809 and later supply the Universal CRT as a Windows component; the Visual
C++ libraries are provided by the installed Microsoft Redistributable.

This is backed by a disposable-copy test on a Windows 11 laptop with system
VC++ runtime 14.51.36247.00: all 33 ordinary app-local MSVC/UCRT DLL copies
were removed from temporary GUI/CLI copies, Padfire launched offscreen and
published working Windows IPC status, and the temporary copies were deleted.
The existing installation and its profile were not touched.

## Remaining wheel-specific Microsoft runtime item

NumPy's Windows wheel contains a private, hash-renamed
numpy.libs/msvcp140-<hash>.dll dependency. The corresponding NumPy extension
modules import that exact hashed filename, so the current build retains the
upstream wheel file rather than renaming it or modifying NumPy's PE import
tables. In the inspected package it is file version 14.40.33810.0 and has a
valid Microsoft Authenticode signature. This is a separate Microsoft runtime
redistribution/provenance item: it is not the standard system prerequisite,
and the project has not yet established the precise license basis for
redistributing that wheel-bundled copy. It remains an open release-review gate.
