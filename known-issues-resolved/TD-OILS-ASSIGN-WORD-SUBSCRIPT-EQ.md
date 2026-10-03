### TD-OILS-ASSIGN-WORD-SUBSCRIPT-EQ. `a[x=3]=1` is taken for a command name because assignment-word detection stops at the first `=` — 2026-07-28 — ✅ RESOLVED 2026-07-28

**Where:** `userspace/oils/src/parser.rs` — the assignment-word test applied to
the first word of a simple command.

**What.** A subscript is arithmetic, so it may contain an `=`. bash scans the
`[ … ]` as a unit before looking for the assignment's `=`; osh does not, so the
word is not recognised as an assignment at all:

```
$ bash -c 'a[x=3]=1; declare -p a x'
declare -a a=([3]="1")
declare -- x="3"
$ osh -c 'a[x=3]=1'
osh: line 1: a[x=3]=1: command not found
```

**Proper fix.** In the assignment-word scan, skip a balanced `[ … ]` immediately
after the name before searching for the `=`. `attr_assignment_split` already does
exactly this for `readonly`/`export` operands and is the model.

**Impact.** Any assignment whose subscript contains an `=` — an assignment
operator (`a[x=3]`), a comparison (`a[x==3]`) or a compound one (`a[x+=1]`).
Kept out of `tests/corpus/arith-subscript-quoting.sh`.

**✅ RESOLVED 2026-07-28.** Fixed in `parser.rs::try_assignment`, but not by
adding a bracket-skip to the existing scan — by **anchoring the whole
recognition at the name**. The old code asked "where is the first `[`?" and
"where is the first `=`?" of the *word*; the new code measures the identifier
the word begins with (`name_prefix_len`) and then looks only at what
immediately follows it: a `[` there opens a subscript (matched with
`balanced_subscript_end`, which counts nesting), and the operator can only be
the `=`/`+=` right after the name or right after that subscript. Anything else
means the word is not an assignment.

Anchoring turned up a **second bug of the same shape, in the other direction**:
`foo=a[b` was being routed to `spanning_subscript_assignment`, because the word
contains a `[` with no `]` after it — the signature of a subscript continuing
into the next segment (`m[$k]=v`). But that `[` is in the *value*, where it is
plain text, so bash stores `foo=a[b` and osh reported "command not found".
Scanning from the name makes both readings fall out of the same test, which is
why one change fixes both.

**A third divergence fixed alongside it: `a[]=1`.** An empty subscript is not a
reason to stop recognising the assignment — bash recognises it and *then*
refuses it (`a[]: bad array subscript`), which discards the rest of the parse
unit, where "command not found" would have let the unit run on. The parser now
builds the assignment with an empty index word and `interp.rs`'s element-assign
path rejects it before the assoc/indexed split, since bash refuses it without
regard to the array's type. The test is on the subscript's *source* being empty
(`unparse::word_src`), not its expansion: `a[""]=1` and `a[$unset]=1` are
arithmetic zero and store at index 0 in both shells. This also repaired
`declare "a[]=1"`, which osh had been accepting **silently** (status 0, nothing
stored) where bash fails the builtin with status 1.

**Coverage.** New corpus case `tests/corpus/assignment-word-shape.sh`: the `=`
inside a subscript in all three spellings (`x=3`, `x==3`, `x+=1`), brackets and
a second `=` in the value, every working subscript spelling as a control
(spanning, whitespace-keyed assoc, nested, arithmetic-variable), the empty
subscript as command and as a `declare` operand, and the empty *expansion* that
must still index. Full differential corpus: 108 matched, 0 failed.
