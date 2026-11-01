# Qt 6.12.0 third-party candidate notices

These notices are copied into the Windows GUI and CLI bundles from the
versioned source crosswalk at
packaging/windows/qt-third-party-crosswalk-6.12.0.md.

They are intentionally a candidate superset, not a binary-specific SBOM.
The published PySide6-Essentials 6.12.0 Windows wheel does not include the
Qt build-specific SPDX SBOM that would identify every selected module, plugin,
third-party package, and build option. Do not infer from the presence of a
notice here that the corresponding component is necessarily compiled into
the wheel. The notices are included to make source-derived copyright and
license texts available while exact wheel composition remains under review.

For any FreeType 2.14.3 code included in this Qt distribution, Padfire selects
the FreeType License (FTL) for this redistribution, not the GPL-2.0-only
alternative. The full upstream freetype-LICENSE.txt contains the FTL and the
alternative GPLv2 terms; the FTL is the selected path compatible with the
project's LGPLv3 Qt/PySide distribution choice. The actual FreeType presence
and build composition still require the Qt wheel's build-specific SBOM.

All files remain in their original Qt source-derived form. The parent
THIRD_PARTY_NOTICES.md and Qt crosswalk describe the source tags and
outstanding binary-specific checks.
