# Third-party notices — Windows build

The Windows front end uses third-party libraries. Their licenses apply to those
components independently of Padfire's own source/license status.

## Qt for Python (PySide6) and Qt 6

PySide6 is available under LGPLv3, GPLv2/GPLv3, or a commercial license. Qt
components also have component-specific licensing; some modules are GPL-only.
The Windows build is configured as a PyInstaller **one-folder** distribution so
the Qt shared libraries remain separate from Padfire's Python executable and
can be inspected and replaced by a user with compatible builds.

Before redistributing a Windows binary, the distributor must select and comply
with an applicable PySide6/Qt licensing option, include the applicable license
texts and notices, preserve required copyright attributions, and meet any
corresponding source/relinking requirements. A one-folder build is a packaging
choice, not by itself proof of LGPL compliance. Review the official terms:

- PySide6 package licensing: https://pypi.org/project/PySide6/
- Qt licensing: https://doc.qt.io/qt-6/licensing.html
- Qt for Python deployment: https://doc.qt.io/qtforpython-6/deployment/index.html
- GNU LGPL v3: https://www.gnu.org/licenses/lgpl-3.0.html

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
| PySide6-Essentials / Shiboken6 / Qt 6 | LGPLv3, GPL options, or commercial terms; exact Qt module terms vary |

The Windows build runs `tools/collect_third_party_licenses.py` after
installing its runtime requirements. It copies license/notice files exposed by
the resolved wheels into `ThirdPartyLicenses/` in both GUI and CLI one-folder
outputs, and generates `ThirdPartyLicenses/COMPONENTS.md` with exact installed
versions and license metadata. If a wheel does not expose a matching license
file, the inventory creates a `LICENSE_FILE_NOT_INCLUDED.txt` review marker
for that component instead of silently pretending the license was copied.

This automated inventory is evidence collection, not a legal certification.
Review each marker and every bundled native library before redistribution.
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
