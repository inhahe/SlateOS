## 1490. A drag is one model at both ends, and keys asking for the impossible drop nothing

**Date:** 2026-10-06 &middot; **Decided by:** Claude (autonomous) &middot; **Lane:** C

**In short:** the toolkit's drag-and-drop (`guitk::dnd`) is the model a drag
between programs is built on, and it had four faults: moving straight from
one drop target to the next never told the first it was left, leaving a
target forgot what the source allows, a drop was reported where it could do
nothing, and the drag's start was never said. It is rewritten as one drag
state between a press and its release, and it now keeps *both ends* of a
drag between programs too -- a drag begun in this window, carried out by the
window system, and another program's drag brought over this window -- so a
program handles a drag from elsewhere exactly as one from itself. The window
system's half is asked of lane F
(`requests/c-f-carry-a-drag-from-one-program-to-another.md`). Three choices
had alternatives.

**Where:** `gui/toolkit/src/dnd.rs` -- `negotiate`, `DragKeys`,
`DragDropManager` (the window-system calls: `take_outgoing`,
`take_withdrawn`, `outgoing_data`, `outgoing_ended`, `offer_entered`,
`offer_status`, `offer_dropped`, `offer_data`, `offer_failed`,
`pointer_left`), `DataFormat::mime`/`from_mime`.

| Choice | Instead of | For | Against |
|---|---|---|---|
| **Keys asking for an effect the source or the target cannot have make the drop do nothing** (the target shows "not allowed") | falling back to an effect both can have, as Windows' sample drop targets do | A user holding Ctrl to keep the original must not find it moved; the mark on the pointer says why nothing will happen, and letting go of Ctrl gives the choice back. | One more way for a drop to do nothing; a user who holds Ctrl out of habit over a target that only moves sees "not allowed" where Windows would move. |
| **With no keys, the target's order of preference, within what the source allows** | a fixed order (copy, then move, then link) for every target | A folder moves what is dropped on it and a document copies it in -- only the target knows which is natural for it; the fixed order made every target copy. | Each target must order its effects deliberately; the default order of a careless one is whatever it wrote first. |
| **The topmost target at the point decides, even one that does not take the data** | falling through to a target under it that does | A drop lands on what the user sees under the pointer, as a window hides the windows behind it; falling through would drop into something covered. | A small target that refuses a format inside a large one that takes it is a hole in the large one. |
| **One manager holds both ends of a drag between programs** | a separate drag-source and drop-target object per side, as OLE and Wayland have | A window's code handles one stream of events whoever began the drag: enter, leave, effect, drop; the window system's glue is a table of calls (in the request), not a second model. | The manager is larger, and a drag begun here and another program's drag share one slot -- which matches the window system's one drag at a time, and is why a new one ends the old. |

**The names between programs** are MIME types (`DataFormat::mime`), as every
desktop's clipboard and drags name theirs. A list of paths as bytes has no
MIME type, so it is the toolkit's own, `application/x-slateos-file-paths`;
`text/uri-list` can carry paths too, percent-encoded, and a source may offer
both for a program ported from elsewhere. Plain text is
`text/plain;charset=utf-8` exactly -- `text/plain` without the charset is
not taken for UTF-8, since it need not be.

**Revisit** when lane F answers the request: its event names and the way its
loop finds a program's manager may move the calls' shapes, not their
meaning.
