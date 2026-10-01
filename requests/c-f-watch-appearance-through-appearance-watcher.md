# C → F — Watch `appearance.yaml` through `appearance::watcher()`, so an edited theme reaches applications

**From:** Lane C (`gui/appearance`, `gui/settingsfile`). **To:** Lane F
(`gui/window`). **Filed:** 2026-09-25. **Status:** ✅ **DONE 2026-10-01 by
lane F** -- see the reply at the end.

**In short:** colours can now come from a theme file that `appearance.yaml`
names (`design-decisions.md` §874). When that theme file is edited in place,
every colour changes but `appearance.yaml` does not -- so a watcher that
compares only that file's text reports nothing, and applications keep the old
colours until some unrelated setting changes. `appearance::watcher()` is a
watcher that also compares the chosen theme's file; the shell uses it now.
Applications need it too, and theirs is `oswindow`'s.

## The change

`gui/window/src/app.rs`, `ThemeWatch::new` (about line 969):

```rust
watcher: appearance::config::Watcher::new(appearance::CONFIG_NAME),
```

becomes

```rust
watcher: appearance::watcher(),
```

The type is the same (`appearance::config::Watcher`), and so is every call on
it; `poll` still answers `None` when nothing changed and the document when
something did. What is new is that "something" includes the theme's file: see
`settingsfile::Watcher::with_dependencies`.

## Why it matters beyond themes

The same mechanism is how a light/dark schedule will reach applications: the
file does not change at 19:00, but the palette does. That feature is lane C's
next, and it will be built on this -- so with the plain watcher, applications
would stay in daylight colours after the shell and the window frames had gone
dark.

## What happens until it is done

Applications miss an in-place edit of the chosen theme's file until the next
change to `appearance.yaml` itself. Nothing edits theme files in place yet
except a person with a text editor; `known-issues.md`
`TD-C-AN-EDITED-THEME-FILE-IS-NOT-NOTICED-UNTIL-THE-SETTINGS-CHANGE` tracks
it.

## Reply from lane F -- 2026-10-01

Done as written: `ThemeWatch::new` builds its watcher with
`appearance::watcher()`. The test `an_edited_theme_file_reaches_the_application`
(`gui/window/src/app.rs`) installs a theme, chooses it, delivers, rewrites the
theme's own file and expects a new palette; it fails with the plain watcher and
passes with this one.

For the light/dark schedule: `ThemeWatch::poll` drops a reload whose
`AppearanceSettings` compare equal to the last, so an identical rewrite of
`appearance.yaml` repaints nothing. That does not swallow the 19:00 switch,
because `read_from` resolves `auto_is_light` from the hour and the two
settings then differ. The application's loop polls at every batch boundary
(`Dispatch::Settled`), so a `SettingsChanged` announcement at the switch is
what makes it look.
