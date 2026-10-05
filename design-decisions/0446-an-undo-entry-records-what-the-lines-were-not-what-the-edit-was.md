## §446 — An undo entry records what the lines were, not what the edit was

**Date:** 2026-08-16
**Lane:** C
**Decided by:** Claude (autonomous)

**In short:** The editor's undo used to store a description of each edit —
"inserted this character at this column" — and work out how to reverse it. It
now stores the affected lines as they were before the edit and as they are
after, and undo simply puts the old ones back. This costs memory (two copies of
each changed line) and buys the guarantee that undo cannot be wrong, which the
old design failed at in four separate ways at once.

**What was wrong.** The four-variant enum was `Insert`/`Delete` (line, column,
text) and `InsertLine`/`DeleteLine` (line, text). Reversing a description
requires the description to keep agreeing with what the code actually does, and
nothing in the language or the tests enforced that agreement. It had drifted:
pressing Enter recorded `Insert { text: "
" }`, but by the time undo ran, no
line contained a newline — the split had moved the tail into a new entry — so
undo removed a byte that was not there and left the document split; Enter's
auto-indent was not recorded at all; `InsertLine` was matched by both `undo` and
`redo` and constructed by nothing; and only one of the three edit entry points
cleared the redo stack.

None of those are individually deep. That is the point: they are four instances
of one failure mode, they accumulated silently, and a fifth would have followed
the next time someone added an editing operation.

**The trade.**

| | Memory per entry | Can it disagree with the buffer? | Cost of a new editing operation |
|---|---|---|---|
| Describe the operation | O(text changed) | yes, and silently | write a variant, an undo arm and a redo arm, and get all three consistent |
| Record the lines *(chosen)* | O(lines touched) | no | wrap the operation in `record_edit` |

The memory difference is the whole argument against recording, and it is
smaller than it looks. A single-character insert on an 80-column line stores
two 80-byte strings rather than a one-byte one. Across the 1000-entry cap that
is on the order of 100 KiB for a text editor — less than one screen of the
font atlas it is already carrying. An operation that rewrites a thousand lines
at once would store two thousand line copies, but such an operation is already
touching that much text.

Against that: undo becomes one `Vec::splice` in each direction and has no
per-operation code at all, so a new editing operation cannot get its undo wrong
by construction — there is nothing to get wrong. Call sites go through
`record_edit(line, before_count, |doc| …)`, and even the one thing they do
state — how many lines they are about to touch — is only used to size the
"before" snapshot; the number of lines *afterwards* is derived from how the
buffer's total length changed, so it is observed rather than claimed.

**What was deliberately not done.** A hybrid — record for multi-line edits,
describe for single-character ones — would recover most of the memory. It was
rejected because it reintroduces the failure mode for exactly the operations
that are most numerous, and because the memory it saves is not memory anyone
was short of. Compression (storing a diff of the two line sets) is available
later if a profile ever asks for it, and would not change the interface.

**Where it lives.** `apps/editor/src/main.rs` — `EditAction`,
`Document::record_edit`/`snapshot`/`splice_lines`/`undo`/`redo`/`clamp_cursor`,
and the `undo_tests` module.
