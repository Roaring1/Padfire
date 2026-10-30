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

The Qt source-package page identifies a Windows ZIP for Qt 6.12.0 (577 MB):

- Source-package listing: https://www.qt.io/development/offline-installers
- Direct source archive:
  https://download.qt.io/official_releases/qt/6.12/6.12.0/single/qt-everywhere-src-6.12.0.zip
- Windows build instructions:
  https://doc.qt.io/qt-6.12/windows-building.html
- Qt 6.12 supported Windows configurations:
  https://doc.qt.io/qt-6.12/windows.html

**PySide6 and Shiboken6 6.12.0 source**

The official Qt for Python source repository is:

- Repository: https://code.qt.io/pyside/pyside-setup.git
- Version tag: `v6.12.0`
- Windows build instructions:
  https://doc.qt.io/qtforpython-6/building_from_source/windows.html

A source checkout should include submodules and use the tag matching the Qt
runtime version:

```powershell
git clone --recursive https://code.qt.io/pyside/pyside-setup.git pyside-setup-6.12.0
git -C pyside-setup-6.12.0 checkout v6.12.0
git -C pyside-setup-6.12.0 submodule update --init --recursive
```

Use the same Qt 6.12.0 build and Python ABI as the distributed binary. The
upstream build guide describes building PySide against the matching Qt
installation; a source checkout by itself is not a rebuilt/relink-tested
library set.

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
- [ ] Complete corresponding Qt/PySide/Shiboken source package is not yet
      bundled with the Windows installer.
- [ ] No controlled written source offer has been issued.
- [ ] DLL replacement/relinking has not yet been demonstrated on Windows.
- [ ] Per-module Qt/plugin license audit has not yet been completed.

Until the remaining items are complete, CI outputs should be treated as
engineering/test artifacts, not as a declaration of release-ready LGPL
compliance.
