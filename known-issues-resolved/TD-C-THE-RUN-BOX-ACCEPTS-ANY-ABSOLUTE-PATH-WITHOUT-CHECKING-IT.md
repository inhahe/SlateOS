### TD-C-THE-RUN-BOX-ACCEPTS-ANY-ABSOLUTE-PATH-WITHOUT-CHECKING-IT — 2026-09-03 — FIXED 2026-09-27

**Fixed 2026-09-27, by the proper fix below's "cheaper alternative", which
turned out to be the better one:** whoever starts the programs the shell
names reports a launch that could not start (`ShellSession::report_failed_launch`,
called by `gui/desktop/src/main.rs` beside the error it already printed), and
the shell answers it (`DesktopShell::launch_failed`). A launch the Run box
asked for brings the box back on the line as it was typed, with why under
it -- `"/usr/bin/fierfox" could not be started: there is no such program.` --
and anything else, a pin or a start menu row or an icon, says so in a
notification. The box still passes an absolute path through unchecked, and
should: the launcher is what knows whether it runs, and asking the disk
first would answer a different question (whether it exists, not whether it
starts).


**In short:** type a path into the Run box that starts with `/` and press Enter
and the box always closes, whether or not anything is there. Mistype
`/usr/bin/fierfox` and the box behaves exactly as if it had worked: it hides,
and you are left looking at a desktop where nothing happened. Any other kind of
mistyped command gets a plain error line — *"…" is not recognized as an
application or command.* — inside the box, which stays open so you can fix it.

**Where.** `gui/desktop/src/run_dialog.rs`, `RunDialog::resolve_command`:

```rust
// Absolute paths pass through directly.
if command.starts_with('/') {
    return true;
}
```

Everything below that line checks *something* (a known-apps list, a plausible
program name). The absolute-path arm checks nothing, so `execute_current` takes
the success branch, posts `RunDialogEvent::Execute` and hides the box. Whether
the user ever learns depends on what the launcher does with a path to nothing —
which today is nothing visible.

**Why it has not been fixed with the rest.** Answering it means asking the
filesystem whether the path exists, and `DesktopShell` performs no filesystem
I/O — the property that keeps ~3037 shell tests offline (see
`TD-C-THE-RUN-BOX-BROWSE-BUTTON-HAS-NOWHERE-TO-GO` for why that is worth
keeping, and for the pattern that answers it).

**The proper fix.** The same pull model the chooser's listing uses: the box asks,
the session answers, the answer comes back in. Concretely — on Enter with an
absolute path that is not already known to exist, post the launch as it does now
*or* hold it pending one `stat`, and have `ShellSession` answer. The cheaper
alternative, and probably the right first move, is to let the **launcher** report
failure back to the box: the shell already gets a `ShellAction::Launch(path)`
answered by whoever starts programs, and a launch that could not start is a fact
that side already has. That turns a silent close into a message without teaching
the shell to read a disk at all.

**Severity while open:** low, and it is a usability failure rather than a
correctness one — nothing wrong is *started*; the box just stops saying that
nothing was. It is the reason a mangled history entry used to fail invisibly.

**Not a regression.** True since `resolve_command` was written.
