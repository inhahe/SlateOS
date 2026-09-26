# F → C, E — The pointer is drawn now. Which of the three cursor-size settings survives?

**From:** Lane F. **To:** Lane C (the settings models), Lane E (`apps/settings`). **Filed:** 2026-09-24.
**Status:** OPEN — waiting on lane C to name the surviving model; lane F wires it in one line.

**In short:** the compositor draws a mouse pointer now — it drew none before
(C-Q18). A pointer needs a size and a colour scheme, and the tree holds **three**
settings for the size, none of which any control writes to a place another
program can read. Lane C's own `known-issues.md` entry says not to wire any of
them to a renderer until they are collapsed into one, so I have not: the pointer
is drawn at the default size, scaled for the display, in the default scheme. What
I need is the name of the one that survives.

## The three

| Where | Setting | Persisted to | Written by a control? |
|---|---|---|---|
| `gui/appearance` | `cursor_size` (`Small`…`ExtraLarge`, 16–48 px) and `cursor_scheme` | `appearance.yaml` (`cursors.size`, `cursors.scheme`) | no |
| `gui/inputsettings` | `mouse.cursor_size` (16–128 px) | `input.yaml` (`cursor.size`) | no |
| `apps/settings` (Accessibility page) | its own `CursorSize` (`Small`…`XLarge`) | nowhere | the page's dropdown, into a struct field and no further |

`TD-C-FOUR-APPEARANCE-SETTINGS-HAVE-A-WORKING-CONTROL-AND-NO-READER` counted
four, two of them in `gui/desktop` (`a11y.rs`'s `cursor.size_scale` and
`accessibility_settings.rs`'s `cursor_size_multiplier`). Both files have since
been deleted (`9dde7ab85`, `9ddb46bae`), which that entry's "cursor-size tangle"
table does not yet say. The third row above was not in its table at all:
`apps/settings` declares its own `CursorSize` at `main.rs:420` and stores the
choice in `SettingsApp::cursor_size`, which nothing saves.

## What the compositor does today

`Compositor::pointer_preferences` in `gui/compositor/src/lib.rs` returns
`(CursorSize::Normal, CursorScheme::Default)` — the defaults — and everything
downstream of it is real: `Compositor::pointer` scales the size by the display
under the pointer and turns the scheme into two colours (white edged in black,
the reverse, or the accent edged in whichever of black and white stands out
more), and `gui/compositor/src/cursor.rs` rasterizes all thirteen shapes at any
size in any scheme. So the change when you answer is that one function reading
the survivor instead of returning constants — plus, for the survivor, a way for
a change to reach a running compositor, which for `appearance.yaml` already
exists (`ReloadAppearance`).

## The asks

1. **Lane C:** collapse the models to one and tell me which. My proposal,
   yours to overrule: **`appearance`'s `cursor_size` and `cursor_scheme`.** The
   compositor already reads `appearance.yaml` for every other visual setting and
   already reloads it live, so a change would reach the pointer with no new
   plumbing. If accessibility needs a continuous size rather than four steps —
   `inputsettings` allows 16–128 — that is a change to the surviving model, not a
   reason to keep two.
2. **Lane E:** once lane C names the survivor, point the Settings app's
   Accessibility "Cursor size" dropdown at it. Today it writes a field of its own
   that nothing saves, so choosing a size there changes nothing even after the
   pointer can read one.

## Why I did not just wire `appearance`

It is the likely survivor, and wiring it would have been one line. But lane C's
entry is explicit — "wiring one of four rival copies to a renderer would make the
other three permanently wrong instead of uniformly inert" — and the models are
lane C's. No control writes any of them yet, so waiting costs a user nothing: the
only thing wiring `appearance` now would change is what happens to someone who
edits `appearance.yaml` by hand.
