### TD-C-RUN-HISTORY-CANNOT-HOLD-A-PATH-THAT-IS-NOT-UTF-8 — 2026-09-03 — **FIXED 2026-09-03**

**In short:** the Run box remembers the commands you have run so you can press
Up to get them back. It remembers them as text. Almost every filename is text,
but SlateOS filenames are allowed to be byte sequences with no text spelling at
all — and if you Browse to such a file and run it, what gets *remembered* is a
mangled version of its name, with the unspellable parts replaced by `�`. Press
Up to run it again and you are asking for a file that does not exist. Nothing
starts, and nothing says why.

**Where.** `gui/desktop/src/run_dialog.rs`. `RunDialog::history` is
`Vec<String>`; `add_to_history` is fed `self.input.text`, which is the *display*
form. `command_exact: Option<PathBuf>` carries the real bytes to
`execute_current`, but it is not carried into the history entry, and could not
be as the type stands.

**Why it fails quietly rather than loudly.** `resolve_command` short-circuits:

```rust
// Absolute paths pass through directly.
if command.starts_with('/') {
    return true;
}
```

so the mangled path is accepted without being checked to exist, the box hides,
`RunDialogEvent::Execute` is posted, and the launcher gets a path to nothing.
Whether the user sees an error then depends on the launcher, not on the box.

**Severity while open:** very low. It needs a file whose name has no UTF-8
spelling *and* the user to re-run it from history rather than re-Browsing. The
failure direction is safe — it starts nothing rather than starting the wrong
file. But it is silent, which is the part worth fixing.

**The proper fix**, in the order the value falls off:

1. Make `history` a `Vec<OsString>`. Display is still lossy; only what is
   *stored* becomes exact, and recall then sets `command_exact`, so re-running a
   browsed file works.
2. Independently: make `resolve_command` check that an absolute path exists
   instead of waving it through, so a command that cannot possibly start says so
   in the box's own error line rather than appearing to work. That is worth
   doing on its own merits — a typo'd absolute path today gets the same silent
   nothing.

**Not a regression.** History was `Vec<String>` from the start; before
2026-09-03 there was simply no way to get a non-UTF-8 name into the box, because
Browse did nothing.

### Fixed 2026-09-03 — step 1. Step 2 is split out below, still open.

`history` is `Vec<OsString>`; `add_to_history` takes `&OsStr` and
`execute_current` hands it the same exact bytes it puts in
`RunDialogEvent::Execute`, rather than the trimmed field text.
`a_browsed_name_that_is_not_utf8_survives_a_trip_through_the_history` drives the
whole round trip through the shell — Browse, choose, run, reopen the box, Up,
Enter — and fails with the `U+FFFD` spelling if the history holds text.

**The autocomplete was the same hole by another route, and is fixed with it.**
The dropdown offers history entries, and accepting one used to write
`Suggestion::text` — a `String` — into the field. So a completion would *show*
one file and *enter* another. `Suggestion` now carries `exact: OsString`
alongside the text it displays: the fuzzy match still runs over the rendering,
because a score against bytes the user cannot see would be a score against
nothing they could have typed, but the entry's own bytes travel with it.
`accepting_a_history_suggestion_fills_in_its_exact_bytes` pins it.

Every route that fills the field from something the user did not *type* now goes
through one private `fill_exact(&OsStr)` — Browse, both history-recall
directions, and the accepted completion. Each was previously a separate
`input.set_text` and so a separate opportunity to drop the bytes. Stepping
*past* the newest entry deliberately does not use it: that text is the user's
own, typed a character at a time, and has no exact bytes behind it.

**A dead field went with it.** `history_path: Option<String>` was set by
`with_config` and read by nothing — the history has never been written anywhere,
despite the module doc claiming "history (with persistence)". The field and the
`with_config` parameter are gone and the doc now says what is true. A parameter
that only looks like it turns persistence on is worse than no parameter. What
persisting it would take is `C-RUN-HISTORY-IS-NOT-PERSISTED` below.

Mutation-checked: making `execute_current` remember the rendering failed exactly
`a_browsed_name_that_is_not_utf8_survives_a_trip_through_the_history`, and making
`accept_suggestion` enter the rendering failed exactly
`accepting_a_history_suggestion_fills_in_its_exact_bytes`. Nothing else moved in
either round.
