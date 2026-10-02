## TD-C-THE-IMPORTER-THAT-WAS-NOT-THERE -- FIXED 2026-09-16, AND THIS ENTRY WAS ITSELF STALE

**In short:** a tracking entry in this file said `apps/kanban` has "a complete
JSON importer" that only needs a file chooser to become useful. It does not.
What exists is the *pieces* of one -- a string parser, a number parser, an
escape decoder -- and nothing that turns parsed JSON back into a board. Anyone
who read the entry and budgeted an afternoon for "add a file chooser" would
have found half a parser missing. This entry corrects that one, and records
why the mistake was easy to make.

Corrects: `TD-C-KANBAN-HAS-AN-EXPORTER-AN-IMPORTER-AND-SWIMLANES-NONE-REACHABLE`.

**Status 2026-09-16: all four steps are done, and this entry became wrong in
the other direction.** `JsonValue`, `parse_value`, `parse_object`,
`parse_array` and `import_board` all exist; `import_board` has a caller; the
file picker is wired. `validate_export` -- the validator that could not return
false -- is deleted, with a note where it stood and a real round-trip test in
its place.

**Read this before trusting the next entry of its kind.** This entry was
written to correct an earlier one that overstated the importer ("a complete
JSON importer needing only a file chooser"). It then outlived its own subject
and overstated the *absence*. A reader who believed it -- and I did, this
morning, far enough to scope four slices and start writing a JSON value parser
that already existed -- would have spent an afternoon rebuilding working code.

The stop was reading the source, not re-reading the entry. The specific near
miss is worth recording: I had staged a "correction" changing the `dead_code`
reasons from `"import needs a file chooser"` to `"no value parser above it,
and no file chooser"`, which would have replaced a nearly-right reason with a
definitely-wrong one, on this document's authority.

**Two defects the correction turned up, both invisible to the entry:**

  * Six `#[allow(dead_code, …)]` attributes that outlived their reason.
    Removing all six leaves the crate compiling cleanly. `check-dead-code-allows`
    reports "0 new" for these, correctly -- a stale allow is not a new one, and
    the gate is built to catch additions rather than survivals.
  * `KanbanApp::export_json`, superseded by `write_board` and kept alive by a
    test asserting its output was non-empty and contained the board name. Both
    true, neither able to fail. **A test on the wrong function is how a
    superseded function survives being superseded.**

### What is actually there

`JsonImporter` has exactly six functions:

    parse_string  parse_unicode_escape  parse_hex4  parse_number
    skip_ws       validate_export

There is no `parse_value`, no `parse_object`, no `parse_array`, and nothing
with `Board` in its return type. The export side is genuinely complete --
`export_board` writes name, columns, cards, labels, swimlane flags and names --
so the round trip is missing exactly one half, and it is the harder half.

### Why it read as finished

**The tests are real, and thorough, and they test the wrong scope.** Ten of
them exercise `parse_string` and `parse_number` against genuinely awkward
input: escaped quotes, `\uXXXX` escapes, surrogate pairs, an unpaired high
surrogate. That is careful work. It is also work on the tokeniser, and a
tokeniser is not a parser.

This sweep keeps finding that **polish is what makes something read as
complete**: a fixture with plausible dates and reserved phone numbers reads as
meant rather than invented. This is the same effect one level up -- **a
well-tested part reads as a finished whole**, and the better the part's tests
are, the more finished the whole looks.

The comment above the type says so in as many words, and is wrong:

    /// Minimal JSON parser for board import (handles the structure exported above).
    // The reader for what the exporter writes. Same position, plus a file
    // chooser it would also need.

It does not handle the structure exported above. It handles the strings and
numbers inside it.

### `validate_export` verifies nothing

```rust
/// Validate that we can round-trip a board through export.
fn validate_export(board: &Board) -> bool {
    let json = JsonExporter::export_board(board);
    !json.is_empty()
}
```

There is no round trip here: it exports and asks whether the result is a
non-empty string. `export_board` always writes at least
`{"name":"","columns":[],...}`, so **this function cannot return false.** Its
name, its doc comment and its return type all promise a check, and it performs
none -- the same shape as `apps/remotedesktop` recording `success: true` before
the attempt it describes.

It is worse than absent, because a future session wiring up the importer would
reasonably call it and read a passing result as evidence.

### What the `dead_code` reasons say, and what is true

Every unreachable item in this file carries a scoped
`#[allow(dead_code, reason = "…")]` naming what it waits for -- a good practice,
and the reason on the importer is `"import needs a file chooser"`. That is
true of `parse_string` in the sense that a chooser is *one* of the things
standing between it and use. It is misleading as a description of the feature,
and the reason strings are what someone greps to size the work.

### The proper fix

1. Delete `validate_export`. A validator that cannot fail is not a weaker
   check than a real one; it is a false statement about the code.
2. Correct the comment on `JsonImporter` to say it is a tokeniser.
3. Change the `dead_code` reasons to name both missing pieces.
4. Write `parse_value`/`parse_object`/`parse_array` over the existing
   primitives, and a `Board` reconstructor over that; then the door.

Until (4), **kanban's door would be export-only**, and an export you cannot
read back is not a backup. That is a defensible thing to ship if it is said
plainly -- JSON is readable and portable, so the file is not a dead end -- but
it must be said, and the app must not imply otherwise.
