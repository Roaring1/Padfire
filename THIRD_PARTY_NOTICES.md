# Third-party notices — Windows build

The Windows front end uses third-party libraries. Their licenses apply to those
components independently of Padfire's own source/license status.

## Qt for Python (PySide6), Shiboken6 and Qt 6

For the Windows community build, Padfire selects the **LGPL v3** licensing
option for the PySide6/Shiboken6 bindings and the Qt libraries. This is not a
commercial Qt build. The PySide6 upstream project confirms that its PyPI
community wheels are available under LGPLv3, GPLv2/GPLv3, or commercial terms:
https://github.com/pyside/pyside-setup/blob/v6.12.0/README.pyside6_essentials.md

The PySide6-Essentials and Shiboken6 wheel metadata declares the alternatives
`LGPL-3.0-only OR GPL-2.0-only OR GPL-3.0-only`. The wheels also carry a short
`LicenseRef-Qt-Commercial.txt` reference for users with separate commercial
agreements. That file is **not** the LGPL text and is not evidence that Padfire
selected commercial terms. Padfire's Windows community build records its
selection of the LGPL-3.0-only option below; the complete LGPL text is included
separately in each distribution. The generated
`ThirdPartyLicenses/COMPONENTS.md` explicitly records this distinction. This
project selection is not, by itself, proof that the corresponding-source,
replace/relink, and installation-information conditions have been satisfied.

The complete GNU LGPL v3 and GNU GPL v3 license texts are included separately
in `ThirdPartyLicenses/LGPL-3.0-only.txt` and
`ThirdPartyLicenses/GPL-3.0-only.txt` in both Windows distributions.

The bundles also include source-derived Qt 6.12.0 third-party license and
copyright texts under `ThirdPartyLicenses/QtThirdParty/`. This is a broad
candidate superset, not a binary-specific SBOM: the PySide6 wheel does not
disclose which conditional third-party components and build variants were
included. The directory README and Qt crosswalk explain this limitation.
For any FreeType 2.14.3 code included in the Qt distribution, Padfire selects
the FreeType License (FTL) redistribution option; the full upstream license
notice is included.
Qt/PySide use is also identified in the application's **About** dialog. Qt
modules may have differing terms; the bundled Qt module/plugin inventory must
be checked against the official Qt 6.12 licensing matrix before each release:
https://doc.qt.io/qt-6.12/licensing.html

**This notice and the license copies alone do not complete LGPL compliance.**
Before redistributing the Windows binaries, the release must also provide the
complete corresponding source for the exact Qt and PySide/Shiboken libraries
used (or a legally valid written offer controlled by the distributor), preserve
the user's ability to replace/relink compatible LGPL libraries and run the
result, and provide any required installation information. A one-folder layout
is helpful but is not, by itself, proof that those requirements are met.
The current CI artifact is an engineering build, not a declaration that the
Qt source/relinking obligations have been completed. The repository provides
a separate CI source-package workflow that verifies and assembles the official
Qt and PySide 6.12.0 archives into three Qt chunk artifacts plus a PySide/
manifest artifact (each with 90-day retention); this is not a permanent written
source offer. See [WINDOWS_QT_SOURCE_COMPLIANCE.md](WINDOWS_QT_SOURCE_COMPLIANCE.md)
for the exact checksums, source workflow, binary component inventory and
remaining release checklist. The separate
[Qt third-party crosswalk](packaging/windows/qt-third-party-crosswalk-6.12.0.md)
records the outstanding binary-specific SBOM and FreeType license questions.
See the official requirements:

- Qt's LGPL obligations:
  https://www.qt.io/development/open-source-lgpl-obligations
- Qt's open-source licensing FAQ:
  https://www.qt.io/faq/qt-open-source-licensing
- PySide6 licensing: https://doc.qt.io/qtforpython-6/licenses.html
- GNU LGPL v3: https://www.gnu.org/licenses/lgpl-3.0.html

## Microsoft Visual C++ runtime prerequisite

The Windows x64 app requires the system Microsoft Visual C++ Redistributable
14.44.35211.0 or newer. The installer checks for it and opens Microsoft's
official download page if missing; portable users must install it separately.
Ordinary app-local MSVC/UCRT DLL copies are removed from both bundles in
accordance with Qt's Windows deployment guidance. See
`WINDOWS_RUNTIME_REQUIREMENTS.md`.

NumPy's upstream wheel also contains a hash-renamed MSVC runtime DLL imported
by two NumPy extension modules. It remains present to preserve the wheel's PE
import contract; its exact Microsoft redistribution basis remains under
review. This is separate from the system runtime prerequisite.

## CPython TLS runtime (OpenSSL)

The Windows package contains CPython 3.12.10's `_ssl.pyd` extension and its
OpenSSL 3.0.16 dependencies `libssl-3.dll` and `libcrypto-3.dll`. OpenSSL 3.x
is licensed under Apache License 2.0; the exact OpenSSL 3.0.16 license text
is included in `ThirdPartyLicenses/OpenSSL-3.0-Apache-2.0.txt` in both bundles.

The Qt OpenSSL TLS backend plugin is also present, but it does not statically
import OpenSSL. The official Qt for Python Windows-wheel documentation states
that official Windows wheels do not ship OpenSSL libraries; Qt can use
Windows Schannel or a separately supplied OpenSSL installation:
https://github.com/pyside/pyside-setup/blob/v6.12.0/README.pyside6_essentials.md

A previous PyInstaller artifact also captured an unrelated OpenSSL 3.5.5
`libssl-3-x64.dll`/`libcrypto-3-x64.dll` pair from the Windows build host.
The post-build normalizer now removes that ambient pair; the Python 3.0.16
pair required by CPython remains. CI asserts this distinction in both
portable bundles. Padfire itself uses local Qt IPC and does not make TLS
requests; the native Schannel plugin remains available for Qt's Windows TLS
support.
## Other bundled/runtime dependencies

The Windows build directly depends on these projects. Their component licenses
are independent of Padfire's MIT license:

| Component | License information |
|---|---|
| mido | MIT |
| python-rtmidi / RtMidi | MIT |
| NumPy | BSD-3-Clause plus licenses for bundled third-party components |
| sounddevice | MIT |
| PortAudio | MIT |
| SoundFile | BSD-3-Clause |
| libsndfile | LGPL-2.1-or-later |
| pyloudnorm | MIT |
| SciPy 1.18.1 | BSD-3-Clause; its bundled OpenBLAS DLL notice also records OpenBLAS BSD-3-Clause, LAPACK BSD-3-Clause-Open-MPI, and GCC Runtime Library Exception terms (see generated component inventory) |
| PySide6-Essentials / Shiboken6 / Qt 6 | LGPLv3 selected for this build; exact Qt module terms vary |
| CPython 3.12 runtime | Python Software Foundation License 2.0 |
| OpenSSL 3.0.16 used by CPython `_ssl.pyd` | Apache-2.0 |
| PyInstaller bootloader | GPLv2 with the PyInstaller bootloader exception; inspect bundled notice |

The Windows build runs `tools/collect_third_party_licenses.py` after
installing its runtime requirements. It also inventories PyInstaller and the
CPython runtime, and explicitly includes the PSF 2.0 license. The collection is
strict in the release build: a distribution without a discoverable license
file fails the build. It copies wheel-reported files from any `LICENSES/`
directory, `.dist-info/licenses/` directory, and conventional license/notice
filenames into `ThirdPartyLicenses/` in both GUI and CLI one-folder outputs.
It generates `ThirdPartyLicenses/COMPONENTS.md` with exact installed versions,
license metadata, and the PySide/Shiboken distinction between their
multi-option license expression and the commercial-reference notice. If a
wheel does not expose a matching license file, the inventory creates a
`LICENSE_FILE_NOT_INCLUDED.txt` review marker for that component instead of
silently pretending the license was copied. The latest tested GUI and CLI
inventories contain no missing-wheel-license markers.

This automated inventory is evidence collection, not a legal certification.
Review each marker, every bundled native library, and the Qt third-party
crosswalk before redistribution.
Qt/PySide6 still requires a deliberate license path and, where LGPL is used,
the applicable license texts, Qt source offer/corresponding source, relinking
mechanism, and installation information. See the official Qt obligations
linked below.

Official component references:

- [mido](https://pypi.org/project/mido/)
- [python-rtmidi](https://pypi.org/project/python-rtmidi/)
- [NumPy](https://pypi.org/project/numpy/)
- [sounddevice](https://pypi.org/project/sounddevice/)
- [SoundFile](https://pypi.org/project/soundfile/)
- [pyloudnorm](https://pypi.org/project/pyloudnorm/)
- [Qt for Python licensing](https://doc.qt.io/qtforpython-6/licenses.html)
- [Qt LGPL obligations](https://www.qt.io/development/open-source-lgpl-obligations)

The PyInstaller outputs retain dynamically loaded Qt and native audio/MIDI
binaries. Both GUI and CLI packages include the root MIT license, this summary,
the generated component inventory, and every license file found in the
resolved wheels; missing wheel-provided files are explicitly marked for review.

## Padfire project license

Padfire's own source code and documentation are licensed under the MIT License;
the complete license text is in the repository's root `LICENSE` file. The MIT
license applies only to Padfire-authored material. It does not relicense Qt,
PySide6, Shiboken6, audio/MIDI libraries, codecs, or other third-party files
included in a binary distribution. Each bundled component retains its own
license and attribution requirements.
