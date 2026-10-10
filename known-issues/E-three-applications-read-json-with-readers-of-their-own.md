# Three applications read JSON with readers of their own

**Status:** OPEN (lane E, found 2026-10-09)

**In short:** lane E's applications have one shared JSON reader now
(`apps/jsonvalue`), which the backup tool and System Restore use and the
weather app's forecasts will. Three programs still carry a reader of their own:
the JSON viewer, the
kanban board and the log viewer. Two readers of one format disagree somewhere
-- one accepts what the other refuses, or reads a number or an escape
differently -- and each one's tests can only show it agreeing with itself. A
user sees it as a file one program opens and another calls broken.

| Program | Where | What its reader does differently |
|---|---|---|
| JSON viewer | `apps/jsonviewer/src/main.rs` (`enum JsonValue`, `parse_json`) | keeps each error's position, to point at it in the text |
| Kanban board | `apps/kanban/src/main.rs` (`enum JsonValue`) | its own board file format's reader and writer |
| Log viewer | `apps/logviewer/src/main.rs` (`parse_json_object`, `parse_json_string`, `parse_json_value`) | a flat reader of one object per line, values as text, for JSON-lines logs |

**Proper fix:** move each onto `jsonvalue`. The JSON viewer needs an error
that says where it happened -- the byte offset, or line and column -- which
`jsonvalue` should gain rather than the viewer keeping a second reader. The log
viewer reads one object per line and wants its values as text, which is
`json_parse` on each line and a value's own display. The kanban board's file
must keep reading what existing boards hold: compare the two readers over the
board tests' files before switching. Each move runs its program's own tests
unchanged, plus a test that a document the old reader accepted still reads.
