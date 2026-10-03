## TD-APPEARANCE-SETTINGS-ARE-NEVER-WRITTEN-TO-DISK

**What.** `gui/desktop/src/appearance_settings.rs` presents a full settings
model — `FontSettings { ui_font, mono_font, ui_size, mono_size, hinting,
subpixel, smoothing }`, theme, wallpaper — with an apply/revert flow built on
a pending-vs-saved pair. But `save()` is:

```rust
pub fn save(&mut self) { self.saved = self.settings.clone(); }
```

It copies one field into another. Nothing is serialized, nothing is written,
and nothing is read back at startup, so every setting reverts the moment the
process exits and no *other* process can ever see it.

**Why it matters now.** It is the reason `ui_font` is inert. The toolkit and
the compositor pick the UI font by walking a compiled-in fallback list
(design-decisions.md §400), which is deliberate but is a *fallback*, not a
setting. Driving the font from configuration is a one-line call to
`guitk::text::set_font_family` in each process — and would be actively wrong
today, because the app that changed the setting would be the only process that
could observe it, so it would measure in the chosen font while the compositor
kept drawing in the fallback. Font divergence across processes is precisely
the failure mode §400 exists to avoid.

It is not only the font: theme, wallpaper and text-rendering options have the
same problem and the same blast radius.

**Proper fix.** Persist to a YAML file under the user's config directory —
YAML with comment preservation is the project-wide rule (`design.txt`) — and
load it during desktop startup, before the first frame. Then have the
compositor read the same file, or (better) have the desktop push the resolved
family to the compositor over the existing protocol, so there is one writer
and the compositor never has to guess. `set_font_family` already returns
`false` and changes nothing on a bad value, so a stale or hand-edited config
naming an uninstalled family degrades to the fallback rather than to
un-drawable text.

**Where.** `gui/desktop/src/appearance_settings.rs` (`save`, and the absent
`load`); `gui/toolkit/src/text.rs::set_font_family` is the ready-made sink.

**Update 2026-08-14 — the persistence half is done; the cross-process half is
not.** `save()` is no longer a field copy. `gui/desktop/src/config.rs` is new
and is the shell's one answer to "which file, and how do I write it without
risking the user's copy": `$XDG_CONFIG_HOME/slateos/<name>.yaml`, falling back
to `$HOME/.config/slateos/`, written to a `.new` temporary and renamed over the
target so a crash mid-save cannot truncate the original. With neither variable
set it reports `NotFound` rather than inventing a system-wide path to put one
user's preferences in. `AppearanceSettingsUI` gained `load()`, `from_document()`
and `apply()`; `save()` now returns `io::Result<()>`.

The format layer is `AppearanceSettings::read_from`/`write_into` over the new
`yamldoc` crate, so a save splices values into the user's own file: comments,
blank lines, ordering and any key belonging to a different version of the
desktop all survive, and a second identical save produces no diff. Reading is
total — a missing key, an unknown enum spelling, or a value out of range leaves
the field at its default (then `validate()` clamps), so a file from a newer
desktop degrades instead of failing. Config spellings are deliberately separate
from `label()`: the UI text is free to change without invalidating every user's
saved choice.

**A second bug was found and fixed on the way in.** `is_dirty()` compared a
hand-picked list of "key fields" that omitted `mono_font`, `subpixel`,
`smoothing` and `custom_accent` — so changing the terminal font left Save greyed
out and the change was lost on close — and ignored font-size changes under
0.1 pt, which a slider step can produce. `AppearanceSettings`/`FontSettings` now
derive `PartialEq` and `is_dirty()` compares the whole struct, which cannot fall
behind a newly added field the way the hand-written check did.

**Still open (the reason this entry is narrowed, not closed):**

1. ~~**Nothing calls it yet.**~~ **Fixed 2026-08-14 (second pass).** Startup now
   reads the file: `DesktopShell::load_appearance()` loads `appearance.yaml` and
   `set_appearance()` re-derives the palette, and `main()` calls it. Everything
   the shell paints itself — taskbar, title bars, borders, start menu, Alt+Tab —
   now follows the saved theme mode, accent, accent-on-taskbar/title-bars and
   transparency. `DesktopTheme` gained `dark()`/`light()` base palettes and
   `from_settings()`; the light palette is new (the shell had only ever had a
   dark one). Reading the file is deliberately *not* in `DesktopShell::new()`:
   a constructor that reads `$HOME` gives every test a machine-dependent result.

   Three latent bugs surfaced while wiring it, all fixed here:
   - The Alt+Tab overlay hard-coded `rgba(30, 30, 46, 230)` and drew
     `taskbar_fg` on it. In light mode that is dark text on a dark overlay. The
     overlay is now its own themed surface (`overlay_bg`/`overlay_fg`/
     `overlay_selected_bg`).
   - The start glyph is drawn in the accent colour *on the taskbar*, so turning
     on "accent on taskbar" made it vanish into its own background. Hence the
     separate `taskbar_accent`, which becomes the contrasting colour when the
     taskbar itself is the accent.
   - One `window_title_fg` served both the focused and the unfocused title bar.
     That is only safe while the two backgrounds are a shade apart, which stops
     being true under accented title bars, so there is now an
     `window_title_inactive_fg`.

   **Catppuccin's Latte accents turned out to be unusable as text** and are the
   one place this deviates from the upstream palette. Measured against the Latte
   base, only blue, mauve and red clear 4.5:1; yellow is 2.31:1, pink 2.34:1,
   rosewater 2.34:1, sky 2.47:1, lavender 2.81:1. The shell draws the accent as
   text (start glyph, start-menu heading), so `AccentColor::color_light()` uses
   each Latte hue scaled toward black by the smallest factor reaching 4.6:1.
   `every_accent_is_readable_as_text_in_both_modes` is the test that rejects the
   unmodified palette; 14 theme tests in total.

   **Still not wired:** the settings *window* itself. `main.rs` still has no
   settings-window routing — "Settings" is a start-menu label with no handler —
   so the only way to change the file is to edit it by hand. `set_appearance()`
   is the entry point that routing will call once it exists. Also unread by any
   renderer: `window_corners` (decorations still use square `fill_rect`),
   `drop_shadows`, `animation_speed`, `icon_size`, `cursor_size`,
   `cursor_scheme`, and `scaling_percent` — all now reachable on
   `DesktopShell::appearance`, none consulted.
1a. **A running desktop now notices a change, but only when asked
   (2026-09-06).** `settingsfile::Watcher` compares the file's *contents* --
   not its modification time, which here both misses changes and invents them
   (see design-decisions 812) -- and `DesktopShell::poll_appearance` applies
   one, returning whether any *setting* actually differed. So the desktop and
   the Settings app no longer agree only across a restart.

   **Closed 2026-09-06 (same day): the shell is now told.** The compositor
   relays `ReloadAppearance`/`ReloadInput` to every window as
   `Event::SettingsChanged { group }` (wire tag `0x0A`, `INPUT_VERSION` 3),
   and `ShellSession` answers the appearance one by calling
   `poll_appearance`. So the chain runs end to end: the Settings app writes
   the file and sends the request, the compositor re-reads and announces, the
   shell re-reads and repaints. No timer anywhere. The paragraph below is kept
   because it is the argument for why there is no timer, which is still the
   live constraint on anything added here later.

   **What is deliberately *not* here is a cadence.** Nothing
   calls `poll_appearance` on a timer, because `ShellSession::animations`
   records that an empty animation set means no wake-up is registered and the
   loop parks unbounded -- which is what keeps an idle desktop idle. A
   once-a-second check would end that to notice a setting nobody is changing.
   Polling on wake-ups that already happen was tried and backed out: a `$HOME`
   read inside `pump()` makes every session test machine-dependent, and it
   broke two of the shell's own tests against the developer's real
   `appearance.yaml`. The finished shape is the `ReloadAppearance`
   notification the compositor already receives, relayed to shell clients --
   immediate *and* idle-preserving, which no poll can be at once. See
   `todo.txt`.

2. **The compositor still cannot see the setting**, so driving `ui_font`
   through `guitk::text::set_font_family` remains wrong for the reason above:
   the desktop would measure in the chosen family while the compositor drew in
   the compiled-in fallback (design-decisions.md §400). The fix is unchanged —
   the desktop pushes the resolved family to the compositor over the existing
   protocol, one writer, no guessing.
3. **The other ~20 `*_settings.rs` panels are still `saved = settings.clone()`.**
   `config.rs` is deliberately general so each can be converted the same way;
   none have been.

**Where (updated).** `gui/desktop/src/config.rs` (new); the "Configuration
file" and light-accent sections of `gui/desktop/src/appearance_settings.rs`;
`DesktopTheme` and `DesktopShell::{set_appearance, load_appearance}` in
`gui/desktop/src/main.rs`; `yamldoc/src/lib.rs`.
