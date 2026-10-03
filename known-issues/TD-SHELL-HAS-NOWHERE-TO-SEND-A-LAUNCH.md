## TD-SHELL-HAS-NOWHERE-TO-SEND-A-LAUNCH

**What.** `DesktopShell::handle_mouse` now answers a click on a start-menu row
with `ShellAction::Launch("/usr/bin/settings")`, but nothing starts a process
from it. `main()` prints the path; there is no caller in a real event loop
because there is no real event loop — the shell has no channel to the process
server, and `launcher::LauncherAction::Launch` has had the same dangling end
since it was written.

**Why it bites.** Every application in the start menu and in the search
launcher is one unimplemented step away from actually running. The routing,
the hit testing and the app database are all in place and tested; what is
missing is exactly one edge — shell to process server.

**Proper fix.** When the shell gains its compositor/IPC event loop, that loop
consumes `ShellAction` and forwards `Launch(path)` to the process server over
a channel. The intent deliberately stays a value rather than a `Command`
spawned in the window manager: policy about *how* a program starts (namespace,
capabilities, cgroup, environment) belongs to the service that owns process
creation, not to the shell.

**Update 2026-09-13: a launch is now executed, and this entry stays open**
**for the half that is not.** `gui/desktop`'s binary drains
`ShellSession::take_launches` and spawns, so the start menu starts programs
and §818 became implementable — a queue nobody drains makes every
feature downstream of it unfinishable, which is what that entry was stuck
behind.

**The rule this entry states is kept.** Nothing in `DesktopShell` or
`ShellSession` starts anything; the intent is still a value everywhere in
the library, and the `Command::spawn` is in the binary, which is already
outside the boundary being protected. `design-decisions.md` 843 records the
reasoning and, more importantly, what it does not claim: `Command::spawn`
inherits this process's environment and privileges, which is exactly the
policy-by-omission described above. That is acceptable on a development
host and is not acceptable on SlateOS.

**So the proper fix below is unchanged** — it becomes a channel send to
the process server. What has changed is that the swap is now one function,
`drain()` in `main.rs`, rather than a missing edge.

**Where.** `gui/desktop/src/main.rs` — `ShellAction` and `handle_mouse`;
`gui/desktop/src/launcher.rs` — `LauncherAction::Launch`.

**Progress 2026-08-21 — narrowed: the loop exists and collects launches, so the
missing edge is now the process server alone.** "There is no real event loop" is
no longer true — `gui/desktop/src/session.rs` (`TD-C-THE-SHELL-CAN-DRAW-ITSELF-
AND-NOBODY-CAN-ASK-IT-TO`, resolved) pumps input, and
`ShellSession::pointer` answers `ShellAction::Launch(path)` by pushing it onto a
queue that `take_launches()` drains. A start-menu click therefore produces a
named program in a caller's hands, under test
(`a_start_menu_row_comes_out_as_a_program_to_start`, plus
`an_ordinary_press_produces_no_launch` to hold the negative).

The session deliberately does **not** spawn it, for the reason "Proper fix"
already gives: the queue is the shell doing its whole job and stopping. What
remains is one edge and no longer a subsystem — a channel to whatever owns
process creation, which lane C does not own. When that service exists, the
change is confined to whoever drives `ShellSession::run`; nothing in
`gui/desktop` needs to move.

**Where (updated).** `gui/desktop/src/lib.rs` — `ShellAction` and `handle_mouse`
(the file was `main.rs` when this was written); `gui/desktop/src/session.rs` —
`ShellSession::pointer` and `take_launches`; `gui/desktop/src/launcher.rs` —
`LauncherAction::Launch`, which still has the same dangling end.
