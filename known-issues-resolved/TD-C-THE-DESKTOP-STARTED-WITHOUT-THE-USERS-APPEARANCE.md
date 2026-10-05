## TD-C-THE-DESKTOP-STARTED-WITHOUT-THE-USERS-APPEARANCE (lane C, 2026-09-25) -- FIXED the same day

**In short:** the real desktop ignored the user's saved appearance when it
started. Theme, accent, wallpaper, fonts, scale, animation speed, auto-hide
and the desktop widgets all came up at their defaults, and changed to the
user's choices only when something sent the compositor `ReloadAppearance` --
in practice, the first time the user saved something in Settings. Found while
wiring the date and time settings (design-decisions 875), which load through
the same call.

**Where:** `gui/desktop/src/main.rs` started the session with
`ShellSession::start`. `start`'s documentation says it "does **not** read the
user's appearance settings from disk ... The caller does it", and the caller
did not: the binary went straight to pumping. The shell re-reads
`appearance.yaml` only on a `SettingsChanged { Appearance }` announcement, and
the compositor announces only when asked.

**Why nothing caught it:** it is the "door that does not exist" shape again
(`load_pinned`, `load_shortcuts`, `apps/fileassoc` before it), and the check
built for that shape, `scripts/check-tested-but-uncalled.py`, could not see
this one for three separate reasons -- worth listing, because each is a way
the next one will hide too:

1. The name is shared. `DesktopShell::load_appearance` and
   `ShellSession::load_appearance` are two functions, and the checker skips a
   name it cannot attribute to one definition.
2. There is no `save_appearance` in the shell to pair it with; the file is
   written by the Settings app, in another crate, and the checker pairs halves
   within one program only.
3. `ShellSession::load_appearance` does have a non-test caller:
   `gui/desktop/src/bin/demo.rs`, the scripted demo, stood in for the binary
   that runs.

**The fix:** `ShellSession::start_for_user` -- `start`, then
`load_appearance` (appearance, widgets, and now the date and time), then a
repaint -- is what the binary calls. `start` keeps its contract, which every
session test relies on so that none depends on the developer's configuration
directory. Tests: `a_session_started_for_a_user_starts_in_their_settings`
(an accent and a time zone on disk reach a session started through the
binary's door) and `a_bare_session_does_not_read_the_users_appearance_or_clock`.
The test is the guard here, not the checker: what it checks is the door the
binary actually uses.
