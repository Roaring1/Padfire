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
- Download artifacts (each with 90-day Actions retention):
  - `Padfire-Windows-Qt-Source-6.12.0-Part-1-of-3`
  - `Padfire-Windows-Qt-Source-6.12.0-Part-2-of-3`
  - `Padfire-Windows-Qt-Source-6.12.0-Part-3-of-3`
  - `Padfire-Windows-Qt-PySide-And-Source-Manifest-6.12.0`


The workflow can be run manually from GitHub Actions. It is also triggered
when its source recipe, chunk splitter, Qt runtime pins/specifications, or
binary component inventory changes on `port/windows-native`. The script retries official mirrors and refuses to
retain either archive unless its exact size and SHA-256 match the official
Qt mirror records above.

The source archives are stored unchanged. The Qt Windows source ZIP is split
into three 250 MiB-or-smaller pieces, with per-piece SHA-256 values and a
binary reassembly check. The PySide/Shiboken TAR.XZ and manifests are in a
separate small artifact. Together these are a reproducible corresponding-source
package for review and possible release attachment. They are not a permanent
public source offer; each Actions artifact expires after 90 days.

### Binary module/plugin inventory

The exact Qt libraries and plugins observed in the tested Windows artifact are
listed in `packaging/windows/qt-binary-components-6.12.0.md`. The source-based
third-party license map is in `packaging/windows/qt-third-party-crosswalk-6.12.0.md`. The five module
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
4. Obtain the Qt SPDX 2.3 SBOM that corresponds to the actual wheel binaries,
   or get an authoritative source/build/third-party manifest from Qt for Python.
   The source archive does not identify the exact wheel build configuration.
5. Supply the required build/relink and installation information for the
   supported Windows target. Test replacement on a clean Windows machine.
6. Audit the license of every Qt module and plugin actually bundled. If a
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

- [x] Current PR validation run [41160613711](https://github.com/Roaring1/Padfire/actions/runs/41160613711) passed both Linux and Windows jobs on commit `cdbdffe`. Windows CI verifies the exact attested NumPy wheel, rebinds its two MSVC PE imports to the system runtime, runs fresh-process NumPy/SciPy numerical tests, enforces removal of the hash-named DLL, builds GUI and CLI, compiles the installer, tests the per-user install/startup/uninstall lifecycle, exercises the byte-identical Qt DLL replacement-and-restore check, and uploads both artifacts. These are 14-day Actions artifacts, not permanent releases:
  - [Windows x64 portable GUI + CLI](https://github.com/Roaring1/Padfire/actions/runs/41160613711/artifacts/13026639371)
  - [Windows installer](https://github.com/Roaring1/Padfire/actions/runs/41160613711/artifacts/13026978049)
- [x] The `cdbdffe` portable artifact was downloaded and inspected locally. Both GUI and CLI folders contain Qt modules/plugins, complete LGPL/GPL texts, PSF notice, OpenSSL 3.0.16 Apache notice, generated component inventories, the runtime prerequisite, Qt third-party candidate notices, and `WINDOWS_RUNTIME_PATCHES.md`. Neither bundle contains ordinary app-local MSVC/UCRT DLLs or NumPy's hash-renamed runtime DLL. Both patch manifests record the original wheel/runtime hash, two changed PE importers and before/after hashes. Neither inventory contains a missing-wheel-license marker.
- [x] Static PE import-table inspection confirms the Qt DLLs are separate dynamic dependencies of the PySide extension modules and Windows plugins. CI also copies `Qt6Core.dll` byte-for-byte to a new path, substitutes it into the installed tree, starts the smoke UI, restores the original, and verifies its SHA-256. This tests replaceable-file mechanics, not a modified/rebuilt Qt implementation.
- [x] The bundle retains CPython's `libssl-3.dll`/`libcrypto-3.dll` pair and excludes the unrelated ambient OpenSSL 3.5 `*-x64.dll` pair.
- [ ] Physical Launchpad Mini MK1/MK2 MIDI, LED feedback/reconnect, actual audio playback/quality, and visible Windows tray behavior remain untested.
- [x] Padfire-authored code and documentation have an explicit MIT license.
- [x] LGPLv3 and GPLv3 license texts are included in the Windows bundles.
- [x] The About dialog identifies Qt/PySide6 use and points to the license files.
- [x] The build records exact Python dependency versions and copies available
      license/notice files; strict CI rejects missing wheel-provided notices.
- [x] Source workflow run [`41037541392`](https://github.com/Roaring1/Padfire/actions/runs/41037541392) completed successfully on
      2026-10-30 and published three Qt 6.12.0 chunks plus the
      PySide/Shiboken source and manifest artifact (90-day retention). The
      same source workflow output from run `41037318209` was downloaded and
      locally verified: every chunk hash matched, the reassembled Qt ZIP was
      604,643,023 bytes with the upstream SHA-256, and the PySide TAR.XZ hash
      matched its upstream value. Run `41037541392` republished the same
      source archives and checksummed chunk format.
- Source artifacts from run `41037541392`:
  - [Qt source part 1](https://github.com/Roaring1/Padfire/actions/runs/41037541392/artifacts/12969185140)
  - [Qt source part 2](https://github.com/Roaring1/Padfire/actions/runs/41037541392/artifacts/12967979721)
  - [Qt source part 3](https://github.com/Roaring1/Padfire/actions/runs/41037541392/artifacts/12969340193)
  - [PySide source and manifests](https://github.com/Roaring1/Padfire/actions/runs/41037541392/artifacts/12969465065)
- [ ] Attach the verified source to a permanent release or provide a legally
      valid distributor-controlled written offer before distributing the
      Windows binaries. Actions artifacts expire and are not included in the
      installer.
- [ ] No build-specific Qt SPDX SBOM or exact PySide wheel source/build provenance has been obtained.
- [ ] No controlled written source offer has been issued.
- [x] Byte-identical Qt DLL file replacement and successful load/restoration are covered by CI.
- [ ] A modified or rebuilt Qt DLL has not been tested. The LGPL relinking/replaceability obligation is not yet demonstrated by a genuinely modified Qt library.
- [x] A first-pass Qt source/plugin map is recorded in
      `packaging/windows/qt-third-party-crosswalk-6.12.0.md`.
- [ ] Exact third-party dependencies and binary presence remain unconfirmed until the Qt/PySide build-specific SBOM is available. Padfire records FTL as its selected redistribution option for any FreeType code included; whether the wheel includes FreeType is still unconfirmed.
- [x] Ordinary app-local MSVC/UCRT copies are removed; Inno Setup requires the official x64 VC++ Redistributable 14.44.35211.0 or newer. A disposable-copy run on the connected Windows 11 laptop succeeded after all 33 ordinary copies were removed.
- [x] NumPy's hash-renamed runtime DLL is no longer shipped. The two import descriptors are rewritten to standard system `msvcp140.dll`; CI verifies the exact PyPI-attested wheel hash, fresh-process NumPy/SciPy behavior and absence of the private DLL from both bundles.

Until the remaining items are complete, CI outputs should be treated as
engineering/test artifacts, not as a declaration of release-ready LGPL
compliance.


## Runtime and replacement-mechanics follow-up (2026-10-31)

Qt's Windows deployment guide states that the individual compiler runtime DLLs
that can be picked up from a development machine are not intended or licensed
for redistribution, and directs distributors to use the official Microsoft
Visual C++ Redistributable. The build now removes ordinary app-local
`MSVCP140*.dll`, `VCRUNTIME140*.dll`, and `ucrtbase.dll` copies from both
GUI and CLI folders. The installer checks for the system x64 redist at
14.44.35211.0 or newer and links to Microsoft's official download page when
missing. See `WINDOWS_RUNTIME_REQUIREMENTS.md`.

On the connected Windows 11 laptop, the installed runtime reports
14.51.36247.00. A disposable copy of the installed GUI/CLI had 33 ordinary
app-local MSVC/UCRT DLL copies removed; the app launched offscreen, reported
working Windows IPC status, and the temporary directory/profile were deleted.
The actual installed app and user profile were not changed. The NumPy 2.5.4 wheel originally carried
`numpy.libs/msvcp140-a4c2229bdc2a2a630acdc095b4d86008.dll`; two unsigned PE
extensions (`_multiarray_umath` and `_pocketfft_umath`) imported that exact
name. Padfire pins the PyPI-attested CPython 3.12 x64 wheel and checks its
published SHA-256 from pip's report. Pinned delvewheel 1.13.2 rewrites both
PE imports to the standard system `msvcp140.dll`, deletes the private wheel DLL,
and runs fresh-process NumPy/SciPy numeric smoke tests. The generated manifest
retains the original DLL SHA-256 and before/after extension hashes. The PyPI
attestation identifies NumPy's publishing workflow; it does not certify every
compiler input or Padfire's downstream PE edit.

Both GUI and CLI build outputs now contain the Qt 6.12 source-derived
third-party notice superset under `ThirdPartyLicenses/QtThirdParty`. This is
not a wheel-specific SBOM and does not prove each candidate is compiled into
the wheel. For any FreeType source included in the Qt distribution, Padfire
selects the FreeType License (FTL) option; exact binary presence remains
unverified without the wheel's build-specific SBOM.

The PR validation at `cdbdffe` also runs a byte-identical QtCore DLL
substitution inside the newly installed CI test tree. The smoke UI launches
against the substituted file and the original is restored and SHA-256
verified. This does not modify/rebuild Qt code and is not represented as a
completed LGPL relink test.

The laptop hardware check confirms only Windows MIDI 2.0 virtual/test devices
are currently present; no physical Launchpad/Novation controller is visible.
Audio endpoints enumerate but no audio sample has been played. Tray/window
visual behavior remains unverified because the launch was offscreen.


## Exact NumPy wheel provenance and patched PE importers (2026-10-31)

The current build pins NumPy 2.5.4 and validates pip's install report against
the PyPI-attested Windows CPython 3.12 x64 wheel:
`numpy-2.5.4-cp312-cp312-win_amd64.whl`, SHA-256
`a7b1b6353e36a7e50de2973a38d705c88ee93adcf120673cee7f45a4a3fa223a`.
PyPI lists a verified Publish Attestation from `numpy/numpy-release`, with
the Windows wheel job in [run 38074635821](https://github.com/numpy/numpy-release/actions/runs/38074635821)
(job 114278931605). Its build log records the `maintenance/2.5.x` source ref.
This proves publisher/workflow identity for the wheel but should not be
overstated as a full reproducible-build record for every compiler input.

The Padfire build changes only the two dependent NumPy PE import descriptors
from the wheel's hash-named runtime to the standard system `msvcp140.dll`,
then removes the private wheel DLL. A fresh-process numerical smoke test
passes after deletion. The generated patch manifest records the exact wheel
URL/hash, original runtime DLL hash and before/after importer hashes.
No NumPy source is rebuilt by Padfire; this is a downstream binary packaging
modification that must be re-audited if NumPy or delvewheel changes.
