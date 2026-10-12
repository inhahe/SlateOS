# Three applications read JSON with readers of their own

**Status:** OPEN (lane E, found 2026-10-09) -- one of the three moved
2026-10-09 (the log viewer); two remain.

**In short:** lane E's applications have one shared JSON reader now
(`apps/jsonvalue`), which the backup tool, System Restore, the weather app's
forecasts and the log viewer use. Two programs still carry a reader of their
own: the JSON viewer and the kanban board. Two readers of one format disagree
somewhere -- one accepts what the other refuses, or reads a number or an
escape differently -- and each one's tests can only show it agreeing with
itself. A user sees it as a file one program opens and another calls broken.

| Program | Where | What its reader does differently |
|---|---|---|
| JSON viewer | `apps/jsonviewer/src/main.rs` (`enum JsonValue`, `parse_json`) | keeps each error's position, to point at it in the text |
| Kanban board | `apps/kanban/src/main.rs` (`enum JsonValue`) | its own board file format's reader and writer |
| ~~Log viewer~~ | ~~`apps/logviewer/src/main.rs`~~ | **moved 2026-10-09.** It wanted each value as the line wrote it, which `json_parse` cannot give -- a number is an `f64`, so a 64-bit id past 2^53 came back rounded and `1.50` came back `1.5`. `jsonvalue::object_members` gives each member's value and its text as written, from the same grammar as `json_parse`. Its own reader had dropped both halves of a surrogate pair (an emoji in a message vanished) and half-read lines with a comma left out; those lines are now shown as plain text. |

**Proper fix:** move each onto `jsonvalue`. The JSON viewer needs an error
that says where it happened -- the byte offset, or line and column -- which
`jsonvalue` should gain rather than the viewer keeping a second reader. The
kanban board's file must keep reading what existing boards hold: compare the
two readers over the board tests' files before switching. Each move runs its
program's own tests unchanged, plus a test that a document the old reader
accepted still reads.
