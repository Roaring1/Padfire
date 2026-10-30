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

## Additional files in the PySide folder

- `opengl32sw.dll` — software OpenGL implementation distributed with the
  Qt/PySide runtime package.
- `MSVCP140.dll`, `MSVCP140_1.dll`, `MSVCP140_2.dll`,
  `VCRUNTIME140.dll`, and `VCRUNTIME140_1.dll` — Microsoft Visual C++
  runtime libraries, not Qt modules.

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
