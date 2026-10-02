## `TD-C-THE-DESKTOP-CRATE-CARRIES-A-SECOND-LAUNCHER-NOTHING-USES` (lane C, 2026-09-27) -- **FIXED 2026-09-27**

**Status:** FIXED 2026-09-27, the same day: `LauncherState`, its frecency,
`LauncherAction`, the dialog's constants and the category colour helpers are
deleted, with their tests -- 2,510 lines to 608. What the shell uses stays:
the program database, the three program paths, `search_score` and
`program_started`. `scripts/reintro-palette.py` lost its 60 entries for the
dialog (see the entry below for the rest of that harness).

**In short:** `gui/desktop/src/launcher.rs` holds `LauncherState`, a
search-as-you-type program launcher with its own key handling, fuzzy ranking
and drawing -- and nothing constructs it. The shell's start menu has its own
search, and the standalone launcher program (`apps/launcher`, lane E) keeps a
separate copy of the same state. Found while giving every list the Page and
Home/End keys (§1416): this one was left out rather than taught keys it will
never receive.

**Where:** `gui/desktop/src/launcher.rs`, `LauncherState` and what only it
uses. The same module's `builtin_app_database`, `FILE_MANAGER` and the other
program constants *are* used, by the start menu and the hotkeys, and stay.

**Proper fix:** delete `LauncherState` and its private helpers and tests, after
checking with a grep and `scripts/check-tested-but-uncalled.py` that nothing outside
the module names them; or, if the shell should have a launcher dialog after
all, wire this one and delete lane E's copy -- a question for the operator, not
a cleanup. Deleting is the default, since the start menu's search does the job.
