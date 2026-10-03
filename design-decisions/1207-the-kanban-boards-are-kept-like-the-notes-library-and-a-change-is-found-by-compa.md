## 1207. The kanban boards are kept like the notes library, and a change is found by comparing the boards' text

**Date:** 2026-09-25
**Lane:** E
**Decided by:** Claude (autonomous) -- Claude's to revisit

**In short:** The kanban app now keeps every board in one file in the
settings folder (`kanban/boards.txt`), written after every key or click that
changed one; until now every card was gone when the window closed. It is the
same kind of file as the notes library (§1205). What is different is how the
window knows a board changed: after each key or click it writes the boards out
as text and compares that with what it last wrote.

### Finding a change

| | Compare the text (chosen) | Count changes where they happen (§1206) | Mark each change (§1205) |
|---|---|---|---|
| A change missed | impossible: whatever changed, the text differs | only by a method added later that forgets to count | by any place that forgets to mark |
| Cost per key or click | the boards written to a string | nothing | nothing |
| Fits this app | the boards are a few hundred cards at most; every change goes through `active_board_mut` *or* `boards.push`, and the view code reaches into boards in forty places | a store type the app does not have | the forty places |

Notes and contacts can hold far more (a note keeps fifty past versions), so they
count; a board is small, so it compares. Cards are written sorted by id so the
same boards are always the same text -- a board's cards live in a hash map,
whose order is not stable.

### Cards once, columns by id

A card is written once and a column lists the ids of its cards. A column
nesting its cards would have had no place for a card in no column (which the
model allows), and the archive is a list of ids already. The reader refuses a
card named twice, a list naming a card the board does not have, and a card in
two places; the JSON import, the one path that could make such a board, now
drops them on the way in -- the first place a card is named is where it is.

**Where it lives:** `apps/kanban/src/main.rs`: `boards_path`, `boards_text`,
`parse_boards`, `KanbanApp::{from_settings, load_boards, keep, unkept,
keeping_line, request_close}`, `JsonImporter::import_board`.

**How to reverse:** the format is `boards_text`/`parse_boards`; `keep` is where
the comparison is.
