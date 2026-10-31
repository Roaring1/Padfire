# Qt/PySide binary components — Windows x64, 6.12.0

Audited from the `Padfire-Windows-x64` CI artifact built from the
`f571f138` branch revision. The same PyInstaller Qt payload is present in
both the GUI and CLI one-folder distributions.

## Qt module libraries

| Binary | Qt module |
|---|---|
| `Qt6Core.dll` | Qt Core |
| `Qt6Gui.dll` | Qt GUI |
| `Qt6Network.dll` | Qt Network |
| `Qt6Svg.dll` | Qt SVG |
| `Qt6Widgets.dll` | Qt Widgets |

## PySide/Shiboken binding runtime

| Binary | Role |
|---|---|
| `pyside6.abi3.dll` | PySide6 binding support runtime |
| `QtCore.pyd` | Python QtCore extension |
| `QtGui.pyd` | Python QtGui extension |
| `QtNetwork.pyd` | Python QtNetwork extension |
| `QtWidgets.pyd` | Python QtWidgets extension |

The PySide6 wheel dependency also installs Shiboken6 6.12.0. Its exact
resolved license files and package version are recorded in the generated
`ThirdPartyLicenses/COMPONENTS.md` inventory.

## Qt plugins shipped in the bundle

| Relative path under `_internal/PySide6/` | Plugin category |
|---|---|
| `plugins/generic/qtuiotouchplugin.dll` | Generic |
| `plugins/iconengines/qsvgicon.dll` | Icon engines |
| `plugins/imageformats/qgif.dll` | Image formats |
| `plugins/imageformats/qicns.dll` | Image formats |
| `plugins/imageformats/qico.dll` | Image formats |
| `plugins/imageformats/qjpeg.dll` | Image formats |
| `plugins/imageformats/qsvg.dll` | Image formats |
| `plugins/imageformats/qtga.dll` | Image formats |
| `plugins/imageformats/qtiff.dll` | Image formats |
| `plugins/imageformats/qwbmp.dll` | Image formats |
| `plugins/imageformats/qwebp.dll` | Image formats |
| `plugins/networkinformation/qnetworklistmanager.dll` | Network information |
| `plugins/platforms/qdirect2d.dll` | Windows platform |
| `plugins/platforms/qminimal.dll` | Windows platform |
| `plugins/platforms/qoffscreen.dll` | Windows platform |
| `plugins/platforms/qwindows.dll` | Windows platform |
| `plugins/styles/qmodernwindowsstyle.dll` | Styles |
| `plugins/tls/qcertonlybackend.dll` | TLS backend |
| `plugins/tls/qopensslbackend.dll` | TLS backend |
| `plugins/tls/qschannelbackend.dll` | TLS backend |

## Microsoft Visual C++ runtime dependency (system-installed)

The PyInstaller output previously included ordinary `MSVCP140*.dll` and
`VCRUNTIME140*.dll` copies in PySide/Shiboken and at the bundle root.
Those copies are now removed by the post-build normalizer. The Windows
installer requires Microsoft's system x64 Visual C++ Redistributable
14.44.35211.0 or newer; the portable distribution documents the same
prerequisite in `WINDOWS_RUNTIME_REQUIREMENTS.md`.

The NumPy 2.5.4 Windows wheel originally contributes a private hash-renamed
MSVC runtime. Padfire rewrites the two importing PE extension descriptors to
the standard system `msvcp140.dll`, deletes the private wheel DLL, and runs
fresh-process NumPy/SciPy smoke tests. The exact wheel and import patch are
recorded in `ThirdPartyLicenses/WINDOWS_RUNTIME_PATCHES.md`.

## CPython TLS runtime (outside the PySide6 subdirectory)

| File under the one-folder bundle | Role |
|---|---|
| `_internal/_ssl.pyd` | CPython 3.12.10 standard-library SSL extension |
| `_internal/libssl-3.dll` | OpenSSL 3.0.16 runtime library used by CPython |
| `_internal/libcrypto-3.dll` | OpenSSL 3.0.16 runtime library used by CPython |

The Apache-2.0 license text for OpenSSL 3.0.16 is included in
`ThirdPartyLicenses/OpenSSL-3.0-Apache-2.0.txt`.

An earlier PyInstaller package also contained an OpenSSL 3.5.5
`libcrypto-3-x64.dll`/`libssl-3-x64.dll` pair in addition to CPython's 3.0.16
pair. Qt for Python's official Windows package documentation says those
wheels do not ship OpenSSL DLLs; the pair was an unrelated build-host addition.
The post-build normalizer now strips those `-x64` files, and CI asserts that
they are absent while the CPython 3.0.16 libraries remain. Qt's Windows
Schannel TLS backend is preserved.

## Implementation licensing and third-party crosswalk

The spot-checked Qt 6.12.0 module/plugin implementation headers use the
Qt-compatible SPDX expression
`LicenseRef-Qt-Commercial OR LGPL-3.0-only OR GPL-2.0-only OR GPL-3.0-only`.
The LGPLv3 option is selected for Padfire's community build. This is distinct
from third-party code statically built into Qt modules and plugins.

See
[`qt-third-party-crosswalk-6.12.0.md`](qt-third-party-crosswalk-6.12.0.md)
for the source-file spot checks, QtCore/QtGui/Network/image-format candidates,
license families and unresolved build-specific questions. The Qt source
archives and Python dependency inventory alone do not prove which conditional
third-party components were included in these wheel binaries.

## Scope and release gate

The main Qt module list above does not include any of the modules named by
Qt's Qt 6.12 page as GPL-only (for example Qt Graphs, Qt MQTT, Qt Quick 3D, or
Qt Virtual Keyboard). This is a first-pass module mapping from the shipped
file inventory, not a complete per-plugin or bundled-third-party legal audit.
The source archive contains the module/plugin source trees and their
attribution/license material; every shipped plugin and any third-party code
it embeds still needs review against the exact source tree.

The inventory must be regenerated and reviewed whenever PySide6/Shiboken, Qt,
the PyInstaller hooks, or the packaged plugins change.


## Current normalized package verification

The PR artifact from run 41162260178 (commit 2309fd5) was inspected from its
downloaded ZIP. Both GUI and CLI one-folder bundles contain runtime
requirements, Qt third-party candidate notices and the NumPy runtime patch
manifest with exact PyPI hashes for NumPy, PySide6-Essentials and Shiboken6.
Neither contains ordinary app-local MSVC/UCRT DLLs or the private hash-renamed
NumPy DLL. Both have no `LICENSE_FILE_NOT_INCLUDED.txt` markers. This artifact
is a 14-day Actions test artifact, not a permanent release.
