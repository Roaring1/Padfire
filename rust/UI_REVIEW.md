# Padfire UI review standard

Padfire is a physical Launchpad instrument with a desktop editor, not a product tour or a dashboard. The interface should feel like the hardware it controls.

## Visual principles

- **No explanatory chrome.** Remove persistent footers, onboarding lines, redundant connection sentences, implementation notes and generic “how to use this” text. Controls should look and behave like controls; help belongs in contextual tooltips or the relevant editor.
- **State without narration.** Show connection state through the PADFIRE mark (green when a supported Launchpad is connected, red when it is not). Avoid a paragraph or status sentence that repeats the same information.
- **Controls where they belong.** Stop Audio is immediately available and unmistakable. Monitor configuration lives in Settings, not as an extra top-bar toggle.
- **Stable physical geometry.** Pad labels wrap or clip inside fixed cells. Text must never resize the grid, move a pad, change its hit target or make the layout cease to resemble a Launchpad.
- **Tactile feedback.** Pressing a pad with the mouse or a physical controller should animate the face with a subtle, centered shrink/release. The input target remains stationary so click, right-click and drag/drop remain reliable. A sustained hold does not restart the animation, while a distinct rapid second press starts a fresh twinge.
- **Use the desktop theme.** Follow the native light/dark preference and theme widget colors. Custom pad colors should remain meaningful without forcing the whole app into a fixed palette.
- **Intentional hierarchy.** Keep only headings and whitespace that help locate an action or distinguish a real section. Do not add a section label, subtitle, helper line or footer simply to fill space.

## Review rubric

| Area | Required evidence |
|---|---|
| Grid geometry | All 64 pads and the side column stay fixed-width with long labels |
| Pointer interaction | Primary press, secondary/right-click, page selector and Stop Audio events |
| Physical input | MIDI press event reaches the same pad feedback path |
| Settings | Compact layout, monitor checkbox and output selectors fit without horizontal overflow |
| System theme | Light and dark system theme changes update active UI visuals |
| Product chrome | No persistent status/footer/tutorial lines or redundant connection sentence |
| Visual review | CI screenshots of main grid and Settings are readable, nonblank and attached to the review |

The automated UI suite covers the deterministic pointer, theme and state-color behavior. Screenshot artifacts let reviewers inspect the rendered layout. Physical Launchpad input/LED, PipeWire routing, sleep/reconnect and Windows device behavior remain separate hardware acceptance gates; a screenshot or CI pass does not claim those checks are complete.
