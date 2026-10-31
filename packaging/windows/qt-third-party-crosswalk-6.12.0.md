# Qt 6.12.0 third-party source and license crosswalk — Windows bundle

This document narrows the third-party review to components that Qt lists for
modules/plugins represented in the Padfire Windows x64 package. It is a
**source-based candidate map**, not a binary-specific SBOM. It must not be used
to claim that every listed component is compiled into the PyPI wheel, or that
the chosen license variant for every dual-licensed dependency is known.

## Evidence and scope

- Binary inventory: `qt-binary-components-6.12.0.md`
- Qt sources: upstream repositories tagged `v6.12.0`
- Qt module/plugin implementation licensing: source file SPDX headers and
  Qt's source REUSE metadata.
- Third-party component list: Qt's Qt 6.12.0 official third-party page:
  https://doc.qt.io/qt-6/licenses-used-in-qt.html
- Qt's licensing page says only third-party components actually shipped with
  an application need to be acknowledged:
  https://doc.qt.io/qt-6/licensing.html
- Qt's SBOM page says installation SBOMs identify module/plugin packages,
  third-party components, versions, copyrights, licenses and system
  dependencies; a source build can generate SBOMs using `configure -sbom`:
  https://doc.qt.io/qt-6.12/sbom.html

The official PySide6 6.12.0 wheel does not include a Qt SBOM or the build
configuration needed to determine which conditional third-party source paths
were selected. Therefore the rows below are candidates for these binaries,
not final binary attributions.

Inspection of the exact `v6.12.0` PySide source tag shows that
`create_wheels.py` stages `LicenseRef-Qt-Commercial.txt`, LGPL-3.0, GPL-2.0,
GPL-3.0 and the Qt GPL exception as base files during wheel creation. The
same tag's wheel metadata declares the LGPL/GPL alternatives, and its
PySide6-Essentials README says the PyPI wheel contents are valid for either
community open-source or commercial licensing. The short commercial file is
therefore a reference notice, not a complete declaration of the chosen path.

The Padfire inventory collector previously looked only inside `.dist-info`
`licenses/` folders and for conventional filenames, which could overlook
wheel files stored under a top-level `LICENSES/` directory. It now scans any
`LICENSES/` directory reported by wheel metadata and fails the build if the
PySide/Shiboken license expression changes unexpectedly. The bundle still
carries complete LGPL/GPL texts independently of this wheel-file collection.

## PySide/Shiboken wheel notice versus selected license

The PySide6-Essentials and Shiboken6 6.12.0 wheel metadata both declare
`LGPL-3.0-only OR GPL-2.0-only OR GPL-3.0-only`. Each wheel also includes a
short `LicenseRef-Qt-Commercial.txt` file that points licensees with separate
commercial agreements to those agreements. That file is not the LGPL text and
does not show that commercial terms were selected for this build.

Padfire records the LGPL-3.0-only option as the selected basis for its Windows
community build; complete LGPLv3 and GPLv3 texts are distributed separately.
The generated component inventory calls out this distinction. The project
selection is a documented packaging decision, not an upstream build-specific
SBOM or a legal determination that all LGPL conditions have been met. The
corresponding-source, user replacement/relink, installation-information and
module/plugin audit gates remain separate.

## Qt module and plugin implementation licensing

The following sample implementation headers in the exact v6.12.0 source
repositories declare:

`LicenseRef-Qt-Commercial OR LGPL-3.0-only OR GPL-2.0-only OR GPL-3.0-only`

This provides an LGPLv3 licensing path for these Qt-authored files, provided
the complete LGPL conditions are met. It is not the GPL-only module expression
`LicenseRef-Qt-Commercial OR GPL-3.0-only`.

| Bundled binary area | Versioned source repository/path used for spot-check |
|---|---|
| GIF/JPEG/ICO image plugins | qt/qtbase: `src/plugins/imageformats/{gif,jpeg,ico}/` |
| Windows, Direct2D, minimal, offscreen platform plugins | qt/qtbase: `src/plugins/platforms/{windows,direct2d,minimal,offscreen}/` |
| Modern Windows style plugin | qt/qtbase: `src/plugins/styles/modernwindows/` |
| Network List Manager plugin | qt/qtbase: `src/plugins/networkinformation/networklistmanager/` |
| OpenSSL, Schannel and cert-only TLS plugins | qt/qtbase: `src/plugins/tls/{openssl,schannel,certonly}/` |
| TUIO touch plugin | qt/qtbase: `src/plugins/generic/tuiotouch/` |
| SVG image and icon plugins | qt/qtsvg: `src/plugins/{imageformats/svg,iconengines/svgiconengine}/` |
| ICNS, TGA, TIFF, WBMP and WebP image plugins | qt/qtimageformats: `src/plugins/imageformats/{icns,tga,tiff,wbmp,webp}/` |

Spot-checked implementation headers include `qgifhandler.cpp`,
`qjpeghandler.cpp`, `qicohandler.cpp`, `qwindowsintegration.cpp`,
`qwindowsdirect2dplatformplugin.cpp`, `qminimalintegration.cpp`,
`qoffscreenintegration.cpp`, `qwindowsvistastyle.cpp`,
`qnetworklistmanagernetworkinformationbackend.cpp`,
`qtlsbackend_openssl.cpp`, `qtls_schannel.cpp`,
`qtlsbackend_cert.cpp`, `qsvgiconengine.cpp`, `qsvgiohandler.cpp`,
and the image-format plugin entry points. The corresponding Qt source
repositories include REUSE metadata for third-party subcomponents.

Qt 6.12's published GPL-only module list includes modules such as Qt Graphs,
Qt MQTT, Qt Quick 3D, Qt Virtual Keyboard and Qt Wayland Compositor. None of
those module DLL names appears in the audited Padfire module inventory. This
is a filename/module check, not a complete license audit of every Qt source
file or plugin.

## Candidate third-party components mapped to shipped modules

The versions and license families below come from Qt's Qt 6.12.0
third-party-code page and the matching source REUSE metadata.

| Qt module/plugin area | Candidate component | Qt 6.12.0 version | License family / uncertainty |
|---|---|---:|---|
| Qt Core | zlib | 1.3.2 | Zlib |
| Qt Core | PCRE2 | 10.48 | BSD-3-Clause with the PCRE2 binary-like packages exception; SLJIT component BSD-2-Clause |
| Qt Core | double-conversion | 3.4.0 | BSD-3-Clause |
| Qt Core | TinyCBOR | 7.0 | MIT |
| Qt Core | Unicode UCD / CLDR | 36 / 48.2 | Unicode license agreements |
| Qt Core | BLAKE2 | source revision in Qt docs | CC0-1.0 or Apache-2.0 |
| Qt Core | Easing equations, SHA/SipHash and related small algorithms | varies | BSD, CC0 or public-domain notices as recorded in Qt REUSE |
| Qt GUI | FreeType | 2.14.3 | Padfire selects FTL for any FreeType code included in this redistribution; exact binary presence still needs the wheel SBOM |
| Qt GUI | HarfBuzz-NG | 14.5.0 | MIT |
| Qt GUI | libjpeg-turbo | 3.2.0 | IJG and BSD-3-Clause |
| Qt GUI | libpng | 1.6.58 | Libpng and libpng-2.0 |
| Qt GUI | D3D12 Memory Allocator | 3.1.0 | MIT |
| Qt GUI | DejaVu Fonts | 2.37 | Bitstream Vera Font License |
| Qt GUI | Emoji Segmenter | 0.4.0 | Apache-2.0 |
| Qt GUI | Wintab API | included interface | LCS-Telegraphics |
| Qt GUI | sRGB color profile | included profile | International Color Consortium License |
| Qt Network | Public Suffix List | Qt 6.12.0 snapshot | MPL-2.0 |
| Qt Network | libpsl | Qt source revision | BSD-3-Clause |
| TIFF image plugin | libtiff | 4.7.2 | libtiff license |
| WebP image plugin | libwebp | 1.6.0 | BSD-3-Clause |

Qt's `qtimageformats` CMake files include the bundled libtiff/libwebp sources
when the system-library build option is disabled. The binary has no separate
`libtiff.dll` or `libwebp.dll`; they may be compiled into the plugin DLLs.
That is why PE filename inspection alone cannot establish third-party
attributions for TIFF/WebP.

Likewise, Qt's Qt Core and Qt GUI third-party components may be statically
compiled into `Qt6Core.dll` and `Qt6Gui.dll` without separate DLLs. A
build-specific SBOM is required to determine exact inclusion and versions.
Padfire selects FTL as the redistribution option for any FreeType code
included; actual presence of FreeType in the wheel remains unconfirmed.

## Dynamic-link evidence and the remaining replacement test

Static PE import-table inspection of the packaged Windows bundle shows that
the PySide extension modules import separate Qt DLLs:

- `QtCore.pyd` imports `Qt6Core.dll`.
- `QtGui.pyd` imports `Qt6Gui.dll` and `Qt6Core.dll`.
- `QtWidgets.pyd` imports `Qt6Widgets.dll`, `Qt6Gui.dll` and
  `Qt6Core.dll`.
- The Windows platform plugin imports `Qt6Gui.dll` and `Qt6Core.dll`; the
  Schannel TLS plugin imports `Qt6Network.dll` and `Qt6Core.dll`.

This confirms that those runtime libraries are separate files with PE
dependencies, rather than statically merged into Padfire.exe. CI now copies
Qt6Core.dll byte-for-byte to a new path, substitutes it into the installed
tree, starts the smoke UI, restores the original, and verifies its SHA-256.
This checks replaceable-file mechanics. It does not test modified/rebuilt Qt
code; a genuine modified-library relink demonstration remains open.

## Microsoft Visual C++ runtime files

Ordinary app-local `MSVCP140*.dll`, `VCRUNTIME140*.dll`, and
`ucrtbase.dll` copies are now removed from both GUI and CLI outputs.
Qt's Windows deployment documentation states that loose compiler runtime
DLLs picked up from a developer machine are not intended or licensed for
redistribution and directs application distributors to Microsoft's official
Visual C++ Redistributable package. The Inno Setup package therefore checks
for the system x64 redist at version 14.44.35211.0 or newer; portable users
must install it separately.

This policy was exercised on a disposable copy of the connected Windows 11
laptop's Padfire install: 33 ordinary runtime DLL copies were removed and the
offscreen application launched and published Windows IPC status using the
installed Microsoft redist (14.51.36247.00). The real installation was not
modified.

## NumPy wheel's original private runtime dependency

The NumPy 2.5.4 CPython 3.12 x64 wheel originally supplied
`numpy.libs/msvcp140-a4c2229bdc2a2a630acdc095b4d86008.dll`. Its two importing
extensions (`_multiarray_umath.cp312-win_amd64.pyd` and
`_pocketfft_umath.cp312-win_amd64.pyd`) are unsigned PE files; their import
descriptors are rewritten to the normal system name `msvcp140.dll` using
pinned `delvewheel 1.13.2`. The hash-named DLL is deleted before PyInstaller
packaging. A fresh process verifies NumPy dot, SciPy FFT and SciPy linalg
operations; the manifest records original and patched importer hashes.

The exact wheel is `numpy-2.5.4-cp312-cp312-win_amd64.whl`, SHA-256
`a7b1b6353e36a7e50de2973a38d705c88ee93adcf120673cee7f45a4a3fa223a`.
PyPI lists a verified Publish Attestation from `numpy/numpy-release` (run
[38074635821](https://github.com/numpy/numpy-release/actions/runs/38074635821)).
Padfire captures pip's install report and validates the exact file-host URL
and hash before patching. This publisher attestation does not certify every
compiler input or Padfire's downstream PE import rewrite. See
`WINDOWS_RUNTIME_REQUIREMENTS.md`.
## OpenSSL runtime distinction

The Padfire PyInstaller bundle contained CPython 3.12.10's `_ssl.pyd` and
`libcrypto-3.dll`/`libssl-3.dll`, with OpenSSL 3.0.16 embedded in the Python
runtime. The Apache-2.0 license text for OpenSSL 3.0.16 is included as
`ThirdPartyLicenses/OpenSSL-3.0-Apache-2.0.txt`.

An earlier package also contained `libcrypto-3-x64.dll` and
`libssl-3-x64.dll` with OpenSSL 3.5.5. Those were not the CPython 3.0.16
runtime pair and were not declared by the PySide6 wheel metadata. The Windows
normalizer now strips that extra pair after PyInstaller and CI asserts that
they are absent, while retaining the CPython pair. Qt for Python's official
6.12.0 README says its official Windows packages do not ship OpenSSL DLLs and
that Qt looks for a separately supplied OpenSSL at runtime; the Schannel
backend is shipped for native Windows TLS.

## Remaining binary-specific work

1. Obtain the Qt SPDX 2.3 SBOM for the exact Qt runtime build used in the
   PySide6 6.12.0 wheel, or have Qt for Python maintainers provide the wheel's
   exact source revisions, build configuration and third-party manifest.
2. Map each third-party candidate above to the actual Qt DLL/plugin contents
   and selected build variant. Padfire selects the FTL option for any FreeType
   code included; the wheel-specific SBOM is still needed to confirm presence.
3. Use the Qt/PySide build-specific SBOM and source revisions to identify
   which mapped Qt third-party components are actually compiled into the
   distributed module/plugin DLLs, then confirm their selected license
   variants and any additional notices required in both bundles. The Python
   wheel inventory does not establish the contents of statically linked Qt
   third-party code.
4. Re-audit the pinned NumPy wheel and PE-import rewrite whenever NumPy or
   delvewheel changes; retain the PyPI attestation, pip report, and generated
   before/after hash manifest for each build.
5. Repeat the audit for each new Qt/PySide wheel or plugin set.

Until these are complete, this crosswalk is evidence for review, not a
certification that the Windows bundle's Qt third-party attribution is
complete.


## Exact PySide/Shiboken wheel identity

The build now checks the PyPI install report for the exact Windows x64 wheels:
- PySide6-Essentials 6.12.0, SHA-256
  `c9a95102aa23c1f86516a30d5a9be37552d324eee7e567eb7508973f1218cb42`.
- Shiboken6 6.12.0, SHA-256
  `a1906cb8116869178c64b6bab52623bbc8b4ceed7b30bfc3c9aa0f6e67f6438e`.

PyPI marks both files "Without provenance". The pip report records their
direct wheel URLs and hashes, but no trusted publisher attestation accompanies
them. This makes the exact binary inputs reproducible by identity, while the
actual wheel build pipeline and binary-specific Qt SPDX SBOM remain open.
