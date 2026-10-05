### C-RUN-HISTORY-IS-NOT-PERSISTED — 2026-09-03 — OPEN

**Status:** OPEN — repaired on lane C's branch 2026-10-05; closes when that
change reaches main after a boot test. The history is kept in `runbox.yaml`
(`history`, oldest first), each entry's bytes percent-encoded behind an
`encoding: percent` marker (design-decisions §426, as the wallpaper's path
is) rather than the NUL-separated file proposed below: the settings files
already carry paths that are not text that way, are written atomically, and
stay readable. `DesktopShell::load_run_history` at start,
`save_run_history` after each pump in which the box ran something.
`RunDialog::load_history` keeps each command once, where it ran last, and
reads only the newest few hundred entries of a long file. Tests:
`the_run_box_history_outlives_the_desktop`,
`every_kind_of_command_line_comes_back_as_it_was` (YAML's own characters,
`%`, a tab), `a_command_line_that_is_not_text_comes_back` (Unix),
`a_run_history_written_by_hand_is_read_as_it_stands`,
`a_loaded_history_keeps_each_command_once_where_it_ran_last`.

**In short:** the Run box's command history is forgotten when the desktop shell
restarts. Press Up to find the thing you ran yesterday and it is not there. It
has never been saved; until 2026-09-03 there was a `history_path` setting that
looked like it turned saving on, which is why this was easy to miss. That field
has been removed, so the code no longer claims anything it does not do.

**Where.** `gui/desktop/src/run_dialog.rs`. `RunDialog::history` is a plain
`Vec<OsString>` in memory; `history()` and `load_history()` exist and are the
right shape, and nothing calls either.

**What it would take.** The shell reads no files (see
`TD-C-THE-RUN-BOX-BROWSE-BUTTON-HAS-NOWHERE-TO-GO`), so this is another instance
of the pull model: `ShellSession` reads the history file at startup and hands it
to `load_history`, and writes `history()` back out when the box runs something.
The on-disk form must be **bytes with a NUL delimiter**, not lines of text — an
entry may be a path with no UTF-8 spelling, and `/` and NUL are the only two
bytes a SlateOS filename may not contain, so NUL is the only separator that
cannot occur inside an entry. A newline-delimited text file would reintroduce
exactly the defect that was just fixed above.

**Severity while open:** low. It is a convenience that is absent rather than
broken, and its absence is obvious the first time you look for it.

**Not a regression.** It has never worked. What changed on 2026-09-03 is only
that the code stopped implying otherwise.
