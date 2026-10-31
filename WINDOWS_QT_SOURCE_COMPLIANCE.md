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

- [x] Latest PR validation run [41172239266](https://github.com/Roaring1/Padfire/actions/runs/41172239266) passed both Linux and Windows jobs on commit `9103d7e`. Linux passed all 7 steps, including GTK regression and screenshot validation. Windows passed all 21 steps, including exact wheel identity/provenance checks, NumPy PE import normalization and numerical smoke tests, license/runtime checks, Qt source-manifest recipe validation, GUI/CLI builds, Inno Setup compilation, per-user install/startup/uninstall, and byte-identical Qt DLL replacement mechanics. These are 14-day Actions test artifacts, not permanent releases:
  - [Windows x64 portable GUI + CLI](https://github.com/Roaring1/Padfire/actions/runs/41172239266/artifacts/13032200674)
  - [Windows installer](https://github.com/Roaring1/Padfire/actions/runs/41172239266/artifacts/13031284688)
- [x] The exact-head portable artifact was downloaded and independently inspected. Each GUI/CLI folder contains 454 files and 85 DLLs. Both contain the required license/runtime notices and component inventories; neither contains ordinary app-local MSVC/UCRT DLLs, NumPy's private hash-renamed runtime DLL, ambient OpenSSL `*-x64.dll` files, or missing-license markers. Independent PE inspection confirms both NumPy extensions in both bundles import system `msvcp140.dll`, `VCRUNTIME140.dll`, and `VCRUNTIME140_1.dll`. Portable ZIP SHA-256: `2e12d3be94deca1afc56d68bf8ce93a15864c98258564e2890cef471c3c4880e`. The installer artifact contains the single expected setup executable and its ZIP SHA-256 is `abdff308b3dce451afa349492f9350b4070df0f542625e7f9829db6fb93a872d`. The two artifacts expire on 2026-11-14 (GitHub timestamps: 2026-11-14 05:28:37Z and 05:28:55Z).
- [x] Static PE import-table inspection confirms the Qt DLLs are separate dynamic dependencies of the PySide extension modules and Windows plugins. CI also copies `Qt6Core.dll` byte-for-byte to a new path, substitutes it into the installed tree, starts the smoke UI, restores the original, and verifies its SHA-256. This tests replaceable-file mechanics, not a modified/rebuilt Qt implementation.
- [x] The bundle retains CPython's `libssl-3.dll`/`libcrypto-3.dll` pair and excludes the unrelated ambient OpenSSL 3.5 `*-x64.dll` pair.
- [ ] Physical Launchpad Mini MK1/MK2 MIDI, LED feedback/reconnect, actual audio playback/quality, and visible Windows tray behavior remain untested.
- [x] Padfire-authored code and documentation have an explicit MIT license.
- [x] LGPLv3 and GPLv3 license texts are included in the Windows bundles.
- [x] The About dialog identifies Qt/PySide6 use and points to the license files.
- [x] The build records exact Python dependency versions and copies available
      license/notice files; strict CI rejects missing wheel-provided notices.
- [x] Exact-head source workflow run [41172235938](https://github.com/Roaring1/Padfire/actions/runs/41172235938) completed successfully on `9103d7e`. It downloaded and hash-verified both version-matched upstream archives, split and reassembly-checked the Qt ZIP, and uploaded all four source artifacts (90-day retention; expires 2027-01-29 05:20:59Z):
  - [Qt source part 1](https://github.com/Roaring1/Padfire/actions/runs/41172235938/artifacts/13031368399)
  - [Qt source part 2](https://github.com/Roaring1/Padfire/actions/runs/41172235938/artifacts/13031154393)
  - [Qt source part 3](https://github.com/Roaring1/Padfire/actions/runs/41172235938/artifacts/13030949985)
  - [PySide source and manifests](https://github.com/Roaring1/Padfire/actions/runs/41172235938/artifacts/13031575842)
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

The PR validation at `2309fd5` also runs a byte-identical QtCore DLL
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


The pip install report also now records the exact PySide6-Essentials 6.12.0
and Shiboken6 6.12.0 Windows x64 wheels and verifies their PyPI SHA-256 values.
PyPI labels both files "Without provenance" and records Twine uploads, so
their input identity is known but their wheel-production pipeline is not
cryptographically attested. This reinforces why the exact Qt/PySide wheel
build configuration, SPDX SBOM and upstream source correspondence remain
unresolved; matching source archives by tag alone are not enough.


## Three-wheel input manifest and current CI (2026-10-31)

The current `2309fd5` validation passed after expanding the pip install-report
guard to three runtime wheels:
- NumPy 2.5.4: PyPI attested; SHA-256
  `a7b1b6353e36a7e50de2973a38d705c88ee93adcf120673cee7f45a4a3fa223a`.
- PySide6-Essentials 6.12.0: PyPI marks the exact Windows wheel
  "Without provenance"; SHA-256
  `c9a95102aa23c1f86516a30d5a9be37552d324eee7e567eb7508973f1218cb42`.
- Shiboken6 6.12.0: PyPI marks the exact Windows wheel
  "Without provenance"; SHA-256
  `a1906cb8116869178c64b6bab52623bbc8b4ceed7b30bfc3c9aa0f6e67f6438e`.

The manifest is present in both portable folders and the Inno Setup
installer lifecycle verifies it is copied into both the GUI and CLI
installations. The current artifacts remain short-lived CI artifacts with
14-day retention, not permanent release attachments.


## Final-bundle PE import check (2026-10-31)

The final-bundle PE gate was added in commit `768ee02` and passed in both
PR and push CI runs for `33c98cd` (PR run
[41166308578](https://github.com/Roaring1/Padfire/actions/runs/41166308578)).
After normalizing each PyInstaller output, the packaging command uses the
pinned `pefile 2024.8.26` parser to inspect normal and delay-load imports in
the final GUI and CLI PE files. It requires exactly one copy of each expected
NumPy extension per distribution, checks that both import `msvcp140.dll`,
and rejects any remaining `msvcp140-<hash>.dll` dependency anywhere in the
bundle.

The downloaded artifact was also checked independently with LLVM's PE
import-table reader. All four packaged extension binaries (two modules in
each of GUI and CLI) import `msvcp140.dll`; no hashed private name remains.
The final portable archive SHA-256 is
`56ffda1152eab481a9a42b9fb60e37dee5c1933f669996419183dac82af23e13`.
The installer artifact ZIP SHA-256 is
`58274701092765f5a3e5bd54f5d6bbf9f77c7ce53a861f388cdda5c80d8ae5ca`.
These 14-day Actions artifacts are test outputs, not permanent release assets.


## Exact-head corresponding-source artifact audit (2026-10-31)

The four artifacts from source workflow run [41172235938](https://github.com/Roaring1/Padfire/actions/runs/41172235938) were downloaded and independently checked against GitHub's artifact digests:

| Artifact | Artifact ZIP SHA-256 |
|---|---|
| Qt source part 1 | `b5d3318013a516d86bb1aac50d2145d73216611cfc40e4edce23e51c38b21a8f` |
| Qt source part 2 | `67bc25f8d04f71ff9200481a3c63c7164d04d03112a4e20ba8871e032d167986` |
| Qt source part 3 | `fe32a1e6765cc6ca8a26d475630c2c209052570ca4cf89b9252e50446fffa8e6` |
| PySide source and manifests | `f5597dbba7db04b369e4751e7cf04238c59f847fbcd56f2fee054bc08e040ace` |

The extracted Qt chunks have sizes 262,144,000, 262,144,000, and 80,355,023 bytes. Their individual SHA-256 values are recorded in `Qt-Chunks/QT_CHUNKS_SHA256SUMS.txt` and were independently checked:

- Part 001: `bc4c0204856db9b05c6fef4622d5f8d9cd2faae4e640dcee66ccb854f4a99f73`
- Part 002: `6d91b69ee124a263da22fa283496fd62dd13b88db9176be717005c4523019c9f`
- Part 003: `8ee3459f9675692a614b613e6b7eb7a6b3762989d130e0815c7ef852b2f67fdb`

Concatenating those three raw chunks reconstructs `qt-everywhere-src-6.12.0.zip` at exactly 604,643,023 bytes with SHA-256 `a72e89c33726d742e67edf494abf7b625eaffc756673221c9823322ecd1a32d1`; ZIP integrity validation passed. The PySide/Shiboken TAR.XZ is exactly 18,211,772 bytes with SHA-256 `099c1a597cf33c1b000e11cf68a0b82d216fb802bf81726efc720394f10c7751`; `xz -t` passed. The manifest and per-chunk checksum files were also present in the fourth artifact.

This is reproducible source-archive preparation and transport evidence. It is not a distributor-controlled written offer, does not establish that these upstream archives reproduce the exact un-attested PySide/Shiboken wheel build, and does not demonstrate a modified Qt library can be rebuilt, substituted, and run by a recipient. Those release gates remain open.
