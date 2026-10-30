# Windows Qt/PySide corresponding-source checklist

This is the source-reference record for the Windows community build. It is
**not** a written source offer and does not, by itself, satisfy the LGPL.

## Exact versions in the tested binary

The current Windows build pins:

- Qt runtime: 6.12.0
- PySide6-Essentials: 6.12.0
- Shiboken6: 6.12.0
- CPython: 3.12.10, x64
- Build target: Windows x64

The Qt/PySide version pins live in `packaging/windows/requirements.txt`.
The generated `ThirdPartyLicenses/COMPONENTS.md` records the complete
resolved Python distribution set used by a particular build.

## Upstream source references

**Qt 6.12.0 source package for Windows**

The Qt source-package page identifies the full Windows ZIP for Qt 6.12.0:

- Source-package listing: https://www.qt.io/development/offline-installers
- Direct source archive (604,643,023 bytes):
  https://download.qt.io/official_releases/qt/6.12/6.12.0/single/qt-everywhere-src-6.12.0.zip
- Official mirror checksum record:
  https://download.qt.io/official_releases/qt/6.12/6.12.0/single/qt-everywhere-src-6.12.0.zip.mirrorlist
- Verified SHA-256:
  `a72e89c33726d742e67edf494abf7b625eaffc756673221c9823322ecd1a32d1`
- Windows build instructions:
  https://doc.qt.io/qt-6.12/windows-building.html
- Qt 6.12 supported Windows configurations:
  https://doc.qt.io/qt-6.12/windows.html

**PySide6 and Shiboken6 6.12.0 source**

The official Qt for Python source archive for 6.12.0 is available as:

- Source archive (18,211,772 bytes):
  https://download.qt.io/official_releases/QtForPython/pyside6/PySide6-6.12.0-src/pyside-setup-everywhere-src-6.12.0.tar.xz
- Official mirror checksum record:
  https://download.qt.io/official_releases/QtForPython/pyside6/PySide6-6.12.0-src/pyside-setup-everywhere-src-6.12.0.tar.xz.mirrorlist
- Verified SHA-256:
  `099c1a597cf33c1b000e11cf68a0b82d216fb802bf81726efc720394f10c7751`
- Source superproject repository: https://code.qt.io/pyside/pyside-setup.git
- Matching source tag: `v6.12.0`
- Windows build instructions:
  https://doc.qt.io/qtforpython-6/building_from_source/windows.html

Use the same Qt 6.12.0 build and Python ABI as the distributed binary. The
upstream build guide describes building PySide against the matching Qt
installation; a source archive by itself is not a rebuilt/relink-tested
library set.

## Reproducible source package

The repository now has a source-packaging workflow:

- Workflow: `.github/workflows/windows-qt-corresponding-source.yml`
- Script: `tools/package_qt_corresponding_source.py`
- Output: unchanged Qt and PySide source archives, `SOURCE_MANIFEST.md`,
  `SHA256SUMS.txt`, and the audited binary component inventory.
- Artifact: `Padfire-Windows-Qt-Corresponding-Source-6.12.0`, with 90-day
  Actions retention.

The workflow can be run manually from GitHub Actions. It is also triggered
when its source recipe or the Qt binary component inventory changes on
`port/windows-native`. The script retries official mirrors and refuses to
retain either archive unless its exact size and SHA-256 match the official
Qt mirror records above.

The source archives are stored unchanged: the full Qt Windows source ZIP and
the PySide/Shiboken source TAR.XZ. This provides a reproducible corresponding-
source package for review and possible release attachment. It is not a
permanent public source offer; the Actions artifact expires after 90 days.

### Binary module/plugin inventory

The exact Qt libraries and plugins observed in the tested Windows artifact are
listed in `packaging/windows/qt-binary-components-6.12.0.md`. The five module
DLLs are Qt Core, GUI, Network, SVG and Widgets. The bundle also includes
the Windows platform plugin, TLS backends, image format plugins, a style
plugin, an icon engine, and a network information plugin.

Qt's Qt 6.12 licensing page lists specific GPL-only modules. None of those
named modules appears as a module DLL in the audited binary. This does not
replace the remaining individual plugin and third-party-code review.

## Required before a public Windows binary release

The build currently includes the LGPLv3 and GPLv3 license texts, the
prominent Qt/PySide notice, and an inventory of component license files.
That is necessary notice material, but it is not the corresponding-source
package and does not demonstrate that a user can replace the Qt libraries.

Before publishing or otherwise distributing a Windows binary under the LGPL
path, the release owner must complete and verify all of the following:

1. Obtain and preserve the complete corresponding source for the exact Qt,
   PySide6 and Shiboken6 libraries included in the binary, including source
   for the actual Qt modules/plugins, build scripts, and any modifications
   or patches. Record archive checksums and tag/commit identities.
2. Provide that source with the application, or make a legally valid written
   offer with instructions for obtaining it that is controlled by the
   distributor. An upstream URL alone is not such an offer.
3. Demonstrate that a recipient can replace/relink the LGPL libraries with
   compatible modified versions and run the resulting application. The
   one-folder PyInstaller layout keeps DLLs visible, but visibility alone does
   not prove replaceability or successful relinking.
4. Supply the required build/relink and installation information for the
   supported Windows target. Test replacement on a clean Windows machine.
5. Audit the license of every Qt module and plugin actually bundled. If a
   GPL-only component is present, re-evaluate the licensing of the combined
   application before release.

Official explanations of the Qt obligations:

- https://www.qt.io/development/open-source-lgpl-obligations
- https://www.qt.io/faq/qt-open-source-licensing
- https://doc.qt.io/qt-6.12/licensing.html
- https://www.gnu.org/licenses/lgpl-3.0.html

Qt's FAQ specifically says the source-delivery offer must be under the
distributor's control; a link to upstream Qt sources alone is not sufficient.
No written source offer is made by this repository document.

## Current status

- [x] Padfire-authored code and documentation have an explicit MIT license.
- [x] LGPLv3 and GPLv3 license texts are included in the Windows bundles.
- [x] The About dialog identifies Qt/PySide6 use and points to the license files.
- [x] The build records exact Python dependency versions and copies available
      license/notice files; strict CI rejects missing wheel-provided notices.
- [x] Exact Qt 6.12.0 and PySide/Shiboken 6.12.0 archives have official
      SHA-256 values, and a CI workflow assembles them into a separate 90-day
      source artifact. The source artifact still needs a successful run and
      permanent release attachment; it is not embedded in the installer.
- [ ] No controlled written source offer has been issued.
- [ ] DLL replacement/relinking has not yet been demonstrated on Windows.
- [ ] The first-pass DLL inventory contains no module DLL named on Qt's
      published GPL-only module list, but per-plugin and third-party-code
      license review remains incomplete.

Until the remaining items are complete, CI outputs should be treated as
engineering/test artifacts, not as a declaration of release-ready LGPL
compliance.
