## TD-THREE-INDEPENDENT-APPEARANCE-MODELS -- FIXED

**Fixed 2026-08-14** (steps 1 and 2 of the proper fix below; step 3 was
"leave the toolkit alone", which stands). `gui/appearance` now owns the model,
its configuration-file spellings, the file's name and location, and the atomic
write. Both front ends hold an `AppearanceFile` — the settings together with
the document they were read from, which is a type rather than two fields
because a save has to splice into the file the user actually has, comments and
unknown keys included. `apps/settings` reads that file at startup and writes it
back whenever an event changed anything; the accent it stores is now a named
`AccentColor` rather than a position in a local array, and its Personalization
pages gained the two accents and the two transparency levels its own copy of
the model could not express. Covered end to end by
`test_a_click_that_changes_an_accent_reaches_the_file`, which runs against a
scratch configuration directory via the new `appearance::config::testing`.

Still open, as the end of this entry already noted: a running app has no way to
learn the file changed, so a live desktop and a live Settings application agree
only across a restart. That half belongs with the change-notification channel
design-decisions.md §400 wants.

**Update 2026-09-06 — both halves are built; this part is closed.**
The telling half landed the same day: `Compositor::announce_settings_change`
sends `Event::SettingsChanged { group }` to every window whenever it handles a
reload request, and the shell acts on the appearance one. A live desktop and a
live Settings app now agree without a restart. What follows is the record of
the noticing half, which came first.

**The noticing half.**
`settingsfile::Watcher` plus `DesktopShell::poll_appearance` mean a running
shell *can* pick up a change without a restart, and does so correctly: the
watcher compares contents rather than timestamps (which would both miss a
slider drag and repaint on every no-op re-save), and `poll_appearance` reports
a change only when a setting actually differs, not merely when the file did.
What is not built is anything that tells the shell *when* to look. That is on
purpose -- a timer would end the idle desktop's unbounded park -- and the
answer is the `ReloadAppearance` notification relayed from the compositor.
See design-decisions 812 and `todo.txt`.

**Superseded 2026-09-03 — step 3, and the third row of the table below.** Step 3
said to leave `gui/toolkit`'s `ThemeMode` and `Theme` alone, and that the shared
crate should *derive* a `guitk::Theme` the way `DesktopTheme::from_settings`
derives the shell's palette. Neither is possible or wanted any more, for two
reasons found while closing
`TD-C-THE-TOOLKIT-HOLDS-A-THIRD-COPY-OF-THE-PALETTE-AND-DISAGREES-WITH-ITSELF-ABOUT-IT`:
`gui/appearance` depends on `guitk`, not the reverse, so `appearance` deriving a
`guitk::Theme` would be a dependency cycle; and the toolkit's `Theme`,
`ThemeMode`, `ThemeColors` and `ThemeManager` had no constructors anywhere in
the tree. They were deleted rather than rewired — see design-decisions.md §810.
The three-models table below is therefore now a two-models table, both of them
`gui/appearance`, and the row for `gui/toolkit/src/theme.rs` describes code that
no longer exists.

The original entry follows.

**What.** "What the desktop looks like" is modelled three separate times, in
three crates, with three incompatible representations and no shared owner:

| Where | Type | Accent model | Persisted? |
|---|---|---|---|
| `gui/desktop/src/appearance_settings.rs` | `ThemeMode {Dark, Light, System}` | `AccentColor` — 14 named variants + `Custom(Color)` | yes, `appearance.yaml` via `yamldoc` |
| `apps/settings/src/main.rs` | `ThemeMode {Light, Dark, System}` | `accent_color_index: usize` into a local array | **no** — nothing is written or read |
| `gui/toolkit/src/theme.rs` | `ThemeMode {Light, Dark, Custom(String)}` | full `Theme` struct of ~30 colours | n/a (a widget palette, not a preference) |

The first two are the same *user-facing setting* in two processes. They do not
agree on the type, the variant order, or the file, and only one of them has a
file at all. `apps/settings` is the application a user actually opens to change
their theme (the desktop's ~20 `*_settings.rs` panels are all `#[allow(dead_code)]`
and unconstructed), and it is the one that persists nothing.

**Why it bites.** Concretely, today: changing the accent in the Settings app
changes nothing on the desktop and is forgotten when the app closes, while the
desktop reads an `appearance.yaml` that the Settings app never writes. Two
processes that disagree about a user preference is the same class of bug as
design-decisions.md §400 (desktop and compositor disagreeing about the font) —
one setting, one writer, or they diverge.

`accent_color_index: usize` is the sharper half. It is a position in a local
array, so persisting it directly would create a file format that silently
remaps every user's accent the moment an accent is inserted or reordered —
exactly the label-vs-spelling failure that `yaml_enum!` exists to prevent.

**Proper fix.** One shared model, in a crate both sides depend on:

1. Extract the model half of `appearance_settings.rs` — the enums, the two
   accent palettes, `AppearanceSettings`, `yaml_enum!`, `read_from`/`write_into`
   — plus `gui/desktop/src/config.rs` into a new `gui/appearance` crate. The
   config location and the atomic-write protocol have to move with it: two
   processes writing one file must agree on the path *and* on how it is
   replaced, not merely on the schema. Leave `AppearanceSettingsUI` and all
   rendering behind in the desktop.
2. Rewire `apps/settings`' Personalization pages onto it, deleting its local
   `ThemeMode` and index-based accent list, and give it load/save.
3. `gui/toolkit`'s `ThemeMode` stays as it is — a widget palette is a different
   thing from a stored preference, and collapsing them would make the toolkit
   depend on a config file. The shared crate should *derive* a `guitk::Theme`,
   the way `DesktopTheme::from_settings` already derives the shell's palette.

Still open after that: a running app has no way to learn the file changed, so
a live desktop and a live Settings app still need a change notification (the
same channel §400 wants for the font). Persisting first at least makes the two
agree across a restart.

**Where.** `gui/desktop/src/appearance_settings.rs`, `gui/desktop/src/config.rs`,
`apps/settings/src/main.rs` (`ThemeMode` at :342, `ACCENT_COLORS` at :380,
`theme_mode`/`accent_color_index` at :685, the Themes/Colors pages at
:1634/:1709 and their handlers at :3199/:3247), `gui/toolkit/src/theme.rs`.
