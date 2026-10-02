### TD-C-KANBAN-HAS-AN-EXPORTER-AN-IMPORTER-AND-SWIMLANES-NONE-REACHABLE — 2026-09-04 — OPEN

**Status 2026-09-27 (lane E):** everything in the table below is reachable now except swimlanes -- kept in
the boards file and never drawn, which is the one row left open. The export and import had doors already;
this adds the keys: the open card's E, D, C and L, the checklist's Tab and Space, the archive's Enter
(restoring to the column the card came from), the board's R, Z, Shift+T and Shift+Delete, the board list's N,
and the filter bar's Ctrl+P, Ctrl+U and Ctrl+L. **A larger hole turned up under it:** nothing in the running
program ever chose a card -- Up and Down only scrolled, and every test that pressed Enter, P, M, B, Ctrl+D or
Ctrl+A set `selected_card` itself first -- so every card operation worked in the tests and did nothing in the
window. The arrows choose now. `apps/kanban/mutate.py` covers both.

> **Correction, 2026-09-15.** "A complete JSON importer" below is wrong. What
> exists is a tokeniser -- string, number and escape parsing -- and nothing that
> reconstructs a `Board`. See `TD-C-THE-IMPORTER-THAT-WAS-NOT-THERE`.

**In short.** About half of the kanban app's model is written, tested, and
cannot be reached from the keyboard. A complete JSON exporter and a complete
JSON importer (ten tests each), swimlanes modelled end to end, three sort
orders, un-archiving, column removal, checklist ticking, and six of the eight
text-input modes. None of it is broken; none of it is reachable.

**How it stayed invisible.** The file opened with `#![allow(dead_code)]`. That
one line is the reason twenty-odd unreachable items accumulated without anyone
noticing — the compiler had the answer the whole time and had been told not to
say it. Deleting it on 2026-09-04 is what produced this list.

**What was done instead of putting the line back.** Every item now carries a
*scoped* `#[allow(dead_code, reason = "…")]` naming the specific thing it is
waiting for, so a **new** piece of dead code still warns. The reasons group into
five:

| waiting for | items |
|---|---|
| somewhere to write a file | the whole `JsonExporter`, `KanbanApp::export_json` |
| a file chooser to read one | the whole `JsonImporter` |
| a lane layout mode in the board renderer | `Card::swimlane`, `with_swimlane`, `swimlane_cards`, `swimlanes_enabled`, `swimlane_names` |
| a control to pick one | the three `SortBy` variants and its `label`/`all`, `Priority::all`, `Column::collapsed` |
| a UI to type into | six `InputMode` variants — board name, card description, comment, checklist item, card title edit, column rename |
| a key | `unarchive_card`, `remove_column`, `toggle_checklist_item` |

**One was fixed rather than annotated.** `switch_board` had a test and no
caller, so `Alt+4` opened a list of every board and no key chose one — a chooser
that was only a list. Arrows and Enter now work there. That is the shape to look
for in the rest: several of the entries above need one call and a key, not a
design.

**Ranked by what a user would miss first**, if this is picked up later: export
(a board you cannot back up), checklist ticking (drawn and inert, so it reads as
broken rather than absent), un-archiving (archiving is one-way), then the input
modes. Swimlanes are the largest and the least missed — nobody expects them
unless they are drawn.

**Nothing here loses data**, because nothing here writes any. The cost is that
the app looks more finished from the outside than it is, and that the next
person to open the file sees twenty scoped allows rather than one blanket — the
scoped ones being the point.
