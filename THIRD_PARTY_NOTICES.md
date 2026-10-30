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

The Windows environment installs the following projects. Consult the
distribution metadata shipped with each package for the exact license and
copyright notices for the version you redistribute:

- mido
- python-rtmidi / RtMidi
- NumPy
- sounddevice / PortAudio
- SoundFile / libsndfile
- pyloudnorm
- PySide6 / Shiboken6 / Qt 6

The Windows installer includes this notice file. The PyInstaller build folder
also retains the dynamically loaded Qt and native audio/MIDI binaries.

## Padfire project license

This repository currently has no root LICENSE file. This notice does not grant
or select a license for Padfire's own code. The repository owner should choose
and add an explicit project license before publishing a redistributable build.
