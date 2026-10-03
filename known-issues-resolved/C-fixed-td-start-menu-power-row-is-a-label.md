## FIXED: TD-START-MENU-POWER-ROW-IS-A-LABEL

**Fixed 2026-08-14.** The footer row is a real button: `power_button_rect()`
reports `Hit::PowerButton`, which toggles `power_menu_open`, and
`power_menu_rect()` / `power_menu_row_rect(row)` place a popup that
`power::render_power_menu` draws and `hit_test` reads — one accessor per
clickable part, as the `Rect` documentation requires. Its five rows are
`power_menu_entries()`, exactly the `Category::System` entries that
`start_menu_entries()` filters out, and clicking one returns the same
`ShellAction::Launch` an application row does: `/sbin/shutdown` and its
neighbours are what actually shut the machine down, not the window manager.
The popup is themed and scaled by the shell (it takes a `PowerMenuStyle`)
rather than by `power.rs`'s own palette, so it follows the light theme and the
display scaling like everything else. `close_start_menu()` is now the single
place the menu closes, which is what keeps the submenu from being stranded over
an empty desktop. Nine tests in `pointer_tests.rs`, including one that walks
every scale from 100% to 200% asserting no system action is dropped or drawn
where it cannot be clicked. No confirmation prompt: Start → Power → Shut down
is one click on every desktop that has this menu, and an extra "are you sure"
is not what makes shutdown safe.

The original report follows.

**What.** The foot of the start menu draws the word "Power" in grey. It is
text, not a control: `hit_test` reports `Hit::StartMenuPanel` there, and the
five `Category::System` entries of the app database — Shutdown, Restart,
Sleep, Lock, Logout — are consequently unreachable from the shell. They are
filtered *out* of `start_menu_entries` on purpose, so that "Shutdown" is not
one mis-click below "Screenshot"; but nothing yet offers them anywhere else.

**Why it bites.** There is no way to shut the machine down from the desktop.

**Proper fix.** A power submenu opened from that row: a small popup listing the
`Category::System` entries, which resolves to the same `ShellAction::Launch`
the application rows produce. `gui/desktop/src/power.rs` already models power
actions and confirmation prompts and should be the thing that renders it,
rather than a second list inside `render_start_menu`. Needs the same
geometry-shared-with-the-hit-test treatment as the rows above it — see the
`Rect` documentation in `main.rs`.

**Where.** `gui/desktop/src/main.rs` — `render_start_menu`'s footer,
`DesktopShell::hit_test`; `gui/desktop/src/power.rs`.
