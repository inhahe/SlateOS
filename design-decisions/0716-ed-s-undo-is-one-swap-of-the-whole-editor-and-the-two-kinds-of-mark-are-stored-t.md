## 716. `ed`'s undo is one swap of the whole editor, and the two kinds of mark are stored the same way but travel differently

**Date:** 2026-08-30 · **Decided by:** Claude (autonomous)
**Lane:** B

**In short:** `ed` gained `u` (undo) along with `m` (move), `t` (copy) and `k`
(set a mark on a line so you can name it later as `'x`). Two shapes had to be
chosen. First, what `u` remembers: a *stack* of past states, as most editors
keep, or a single one that is swapped in and out. Second, `ed` has two entirely
different things both called "marks" — the `k` marks a user sets by hand, and
the internal flags a global command (`g/pattern/command`) uses to remember
which lines it still has to visit — and when a command moves a line, each has
to decide whether to move with it. Getting either wrong is invisible in normal
use and produces a wrong answer in exactly the case people write `g` for.

### Decision 1: one snapshot, swapped, not a stack

`Editor::undo` is a single `Option<Snapshot>` holding the whole editor state —
buffer, both mark arrays, the current line, and the modified flag. `u` swaps it
with the live state rather than popping it.

*What changes:* `u` after `u` **redoes** rather than undoing a second change.

- **For:** it is what GNU ed does, exactly. `ed` is a program whose entire value
  is being the `ed` that scripts and fingers already know; a deeper undo would
  be a different editor, and one whose difference shows up only after someone
  has relied on the redo.
- **Against:** a one-deep undo is genuinely less useful, and a stack is barely
  more code — `Vec<Snapshot>` and a `pop`.
- **Why the "for" wins:** the "against" is an argument for a *better editor*,
  and this is not the place to have it. Every other GNU-compatibility decision
  in this file (§713 especially) resolves the same way: where we differ from
  GNU we refuse, we do not improve. Someone who wants a stack can have one the
  day `design.txt` asks for a non-GNU `ed`.

The snapshot is whole-editor rather than a delta because the commands that
modify are heterogeneous — `m` relinks, `j` replaces a range with one line, `e`
throws everything away — and a per-command delta would be seven kinds of
bookkeeping to save copying a buffer that a line editor keeps in memory anyway.
The cost is one `Vec<Vec<u8>>` clone per modifying command; inside a global it
is one clone per *global*, not per selected line, which is what makes
`g/x/s/a/b/` over a large file linear rather than quadratic.

### Decision 2: `k` marks travel with a moved line, `g` selections do not

Both are parallel arrays indexed by buffer position — `kmarks: Vec<u32>`, 26
bits per line, and `marks: Vec<bool>`. When `m` lifts a range out and puts it
back elsewhere, `Editor::cut` carries the `k` marks along inside a `Taken` and
`Editor::paste` restores them, while the `g` selection is dropped on the way out
and a clear one inserted on the way in.

*What changes:* after `1ka`, `1m$`, the address `'a` finds the line at the
*end* of the buffer. But `g/^\(one\|four\)$/4m0p` runs its command list once,
not twice — moving `four` deselects it.

- **For the asymmetry:** it is GNU's behaviour, measured both ways, and GNU gets
  it for structural reasons we do not share. GNU's buffer is a linked list and
  its `k` marks are pointers into it, so relinking a node carries its mark for
  free; its `g` selections live in a separate array of node pointers that
  `move_lines` explicitly calls `unset_active_nodes` to prune. Two mechanisms,
  two answers.
- **Against:** stated as a rule it sounds arbitrary — "these marks follow the
  line, those follow the position" is the kind of sentence that invites someone
  to "fix" it. And it is not what the fix plan in
  `TD-B-ED-IS-MISSING-EIGHT-COMMANDS` predicted; that plan reasoned from our
  own array representation and said both should travel.
- **Why the "for" wins:** the asymmetry is not arbitrary once you ask what each
  mark *means*. A `k` mark is a name the user gave to a **line**, so it belongs
  to the line and goes where the line goes. A `g` selection is a note the
  editor made about **work left to do at a position**, and a command list that
  moves a line has, by moving it, dealt with it. The alternative — carrying the
  selection — makes `g` visit lines the list has already handled, which is a
  worse failure than the surprise: it is a loop whose trip count depends on
  what its own body does.

**What this cost, and the lesson:** the plan's prediction was written from the
code and was wrong; the harness (`scripts/ed-diff.sh`, ~400 cases against GNU
ed 1.20.1) caught it in one run. Three other rules in the same batch were wrong
the same way — see the `FIXED` note on that `known-issues.md` entry. Reading a
reference implementation tells you what it does in the case you thought to look
at; running it tells you what it does.
