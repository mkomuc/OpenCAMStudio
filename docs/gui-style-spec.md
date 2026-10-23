# GUI implementation style specification

This is the implementation contract for new and modified OpenCAMStudio GUI
features. Reuse the existing iced widgets and styles so a feature fits the same
desktop interface without inventing its own buttons, colors, or form layout.
Existing screens can adopt it incrementally when they are changed.

## Sources of truth

- [Inspector styles](../crates/cam-app/src/gui_inspector_style.rs): section
  headings, optional sections, buttons, tabs, inputs, pickers, popups and footers.
- [Shell palette](../crates/cam-app/src/gui.rs): `palette`, ribbon metrics,
  `command_button_style`, `command_toggle_style`, and `tab_button_style`.
- [Operation inspector](../crates/cam-app/src/gui_operation_tabs.rs) and
  [setup editor](../crates/cam-app/src/gui_setup_editor.rs): reference form layouts.

Use these helpers instead of copying style closures into a new feature. If a new
shared treatment is needed, add it to the style module and update this document
in the same change. Keep domain behavior in the controller/model; styles must
never change machining rules or bypass validation.

## Typography

Use iced's default application font and logical pixel sizes (allow platform DPI
scaling; do not compensate with screen-specific font sizes).

| Role | Size | Weight | Treatment |
| --- | --- | --- | --- |
| Inspector title, e.g. “Operation 4 · Profile” | 17 | Semibold | `inspector_heading` |
| Configuration section, e.g. “Operation parameters” | 12 | **Bold** | `inspector_section` / `inspector_section_label` |
| Parameter label, value, picker, button, tab | 12 | Normal | `INSPECTOR_TEXT_SIZE` |
| Units, hints, secondary descriptions | 11 | Normal | Muted text |
| Ribbon group/command captions | 10 | Normal | Existing ribbon renderers |

Section names have **the same size as parameter labels**, with bold weight to
show hierarchy. Do not enlarge them or use muted caption styling to simulate a
section heading. Screen titles remain larger. Use sentence case and concrete
labels; put explanations in nearby help or tooltips.

## Color scheme

Use the existing dark theme (`iced::Theme::Dark`). The following RGB values are
normalized to 0–1 and name the roles already implemented in the shared styles.
Use the referenced constants/functions instead of repeating numeric colors.

| Role | RGB | Source |
| --- | --- | --- |
| Inspector text and section headings | 0.86, 0.88, 0.91 | `INSPECTOR_TEXT` |
| Inspector hints, units, placeholders | 0.61, 0.65, 0.71 | `INSPECTOR_MUTED` |
| Secondary control surface | 0.16, 0.18, 0.21 | `inspector_secondary` |
| Input/picker/popup surface | 0.12, 0.14, 0.17 | Shared input/popup styles |
| Inspector outline/divider | 0.25, 0.28, 0.32 | Shared inspector styles |
| Primary action fill | 0.18, 0.49, 0.80 | `inspector_primary` |
| Focus/selection accent | 0.20, 0.55, 0.90 | `palette::ACCENT_BLUE` |
| Selected project row / input selection | 0.20, 0.28, 0.38 | `palette::SELECT_BG` |
| Pane background | 0.15, 0.15, 0.15 | `palette::PANE_BG` |
| Ribbon background | 0.22, 0.22, 0.22 | `palette::RIBBON_BG` |
| Top tab strip | 0.17, 0.17, 0.17 | `palette::TOPBAR_BG` |
| Warning | 0.95, 0.62, 0.10 | `palette::WARN` |
| Error | 0.92, 0.30, 0.28 | `palette::ERROR` |

Use blue for actions, focus, and selection. Pair errors and warnings with readable
text or an icon and another visual cue (such as the thicker invalid-field border).
Do not communicate state by color alone. Keep the established viewport/toolpath
colors separate from form styling.

## Buttons and navigation

- Use `inspector_button` for actions: 12 px text, padding 7 px vertically / 10 px
  horizontally, 5 px corner radius. Secondary actions have a 1 px outline.
- Apply `inspector_primary` to the main commit action, such as “Create operation”
  or “Apply changes”. Use secondary styling for Cancel, Clear, and auxiliary
  actions. Keep one primary commit action per editor footer.
- Preserve the shared active, hovered, pressed and disabled states. Secondary
  hover/pressed fills are RGB (0.23, 0.26, 0.30) / (0.12, 0.15, 0.18); primary
  hover/pressed fills are (0.26, 0.59, 0.92) / (0.14, 0.40, 0.69). Primary text is
  white. Disabled primary actions use the disabled secondary treatment.
- Disable unavailable actions with `on_press_maybe(None)` (or no handler), so
  appearance and interaction agree. Explain consequential unavailability nearby.
- Use `inspector_tab` for inspector navigation. Active tabs use the shared blue
  text, subdued fill and outline. Switching tabs must preserve the working draft.
- Use the existing ribbon command renderers and bundled SVG icons for ribbon
  actions; their compact/large layouts and hover behavior belong to the shell.
  Follow [icon credits](../crates/cam-app/assets/icons/CREDITS.md) for new assets.

## Forms and spacing

- Inspector content uses 14 px outer padding, 10 px between form rows, and 8 px
  between label/control/unit columns. Keep controls vertically centered.
- Use `inspector_input`: 12 px text, padding 7 / 9 px, 1 px border, 5 px radius.
  Focus uses the accent outline; hover uses the muted outline.
- Use `inspector_picker` with matching 12 px text and 7 / 9 px padding. Reuse
  `popup_surface` and `popup_item_style` for custom selectors and context menus.
- Prefer `inspector_value` or the existing field-row helpers for numeric inputs:
  labels on the left, right-aligned numbers, explicit units. Use a 24 px unit
  gutter when using `inspector_value`; do not use spaces to align text.
- Keep units and zero/automatic/inherited meanings clear. Show invalid values
  with the shared error treatment and an explanation when the cause is ambiguous.
- Use `inspector_section` between logical groups: a 1 px divider, 10 px gap to
  the heading, and section padding of 10 px above / 2 px below.
- Scroll the editor body while keeping `inspector_footer` visible. Its padding
  is 12 / 14 px. Put Cancel before the right-aligned commit action.
- Use flexible widths and the existing two-row operation-tab layout so the
  configuration remains usable in a narrow, resized inspector.

## Sections that can be enabled or disabled

An optional section has a **small checkbox immediately before its bold name**.
Use `inspector_optional_section` (or `inspector_section_toggle` inside a custom
header). The checkbox is 14 px square with an 8 px gap, and its label is 12 px
bold. The checkbox and its label both toggle the same section state.

- Checked: show the section's applicable parameters directly below its heading.
- Unchecked: collapse the entire parameter body. Keep the heading and checkbox
  visible so the user can enable it again. Do not leave disabled fields, a blank
  placeholder body, or a second enable control elsewhere.
- Bind the checkbox to the existing domain/draft enable state. Derive visibility
  from that same state; do not create a separate collapse flag that can disagree.
- For sections repeated across operation tabs, show the same checkbox and state
  on each relevant tab. Enabling on any tab reveals that tab's parameters.
- If a section is unavailable, keep its toggle disabled and show a concise reason
  outside the collapsed body. For example, holding tabs require one closed contour.
- Stop picking or editing controls that become hidden. Preserve the editor's
  Apply/Cancel and undo behavior. Collapsing the view alone must not commit edits.
- Retain configured values when the model supports a separate enable flag. If
  absence represents disabled state, keep existing initialization rules: carving
  clearing seeds a compatible end mill and its initial parameters on enable.

Current examples are Holding tabs (Geometry), Preferred start XY (Linking),
Clearing pass (Tool/Passes/Linking), and Feed height (Heights). Feed height retains
its reference and offset while toggled within a draft; a saved disabled feed has
no persisted previous reference, so enabling it starts from Top height. Other
height planes remain required and use ordinary expand/collapse headings.

A normal boolean parameter (Climb, Reverse direction, etc.) is still a parameter,
not an optional section. An expand/collapse control for a required group changes
visibility only and must not imply that machining behavior has been disabled.

## Review checklist for GUI changes

- Reuse the shared styles; document any new shared token or component here.
- Check headings against parameter labels: equal size, section names bold.
- Check default, hover, pressed, selected, focused, invalid and disabled states
  as applicable, including tooltips and readable unavailability reasons.
- Toggle each optional section off/on, switch tabs, and reopen an existing
  disabled configuration. Check that unchecked bodies disappear and the header
  stays reachable. Verify available values survive where supported.
- Check narrow inspectors, scrolling, and platform DPI scaling; keep the footer
  and all tab choices reachable.
- Verify Apply, Cancel and undo retain the existing transaction boundaries. Add
  behavior tests for new state transitions; cosmetic-only changes need visual
  review rather than tests that just repeat style constants.
- Run `cargo clippy -p cam-app --features gui -- -D warnings` and
  `cargo test -p cam-app --features gui` for affected GUI behavior. Headless-only
  checks do not compile the desktop shell. Report any checks that could not run.
