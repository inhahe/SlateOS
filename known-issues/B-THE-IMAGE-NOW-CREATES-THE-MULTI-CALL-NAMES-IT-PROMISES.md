## B-THE-IMAGE-NOW-CREATES-THE-MULTI-CALL-NAMES-IT-PROMISES (lane B, 2026-09-13)

**In short:** several of our programs answer to more than one name — `ar` is
also `ranlib` and `strip` — but nothing ever created the second name, so typing
`ranlib` got "command not found" from a system that contains a working one.
`scripts/create-ext4-rootfs.sh` now makes the links.

### What was already known, and what was not

The diagnosis is not mine: `scripts/multicall-aliases.py` exists precisely for
this and says it plainly — *"We wrote the dispatch about seventy times and
**never wrote the links**."* It reports **167** command names that some program
answers to and nothing produces.

What was missing is the other half: somewhere to write them. The links belong
on the image, and the image's manifest is the list of what ships, so that is
where they now live. A line

    ranlib = ar

asks for `ranlib` as a second name for an already-listed binary.

### Why only three

Of the 167, exactly **three** have a producer that is on the image today:
`ranlib` and `strip` (from `ar`) and `killall` (from `kill`). The other 164 are
unreachable for a more ordinary reason — **their producer is not staged
either**. The personality gap and the staging gap are largely the same gap seen
from two ends, and the 167 will shrink as a side effect of the manifest growing
rather than needing separate work.

`ranlib` is the one that is not cosmetic. `ar` was added to the manifest hours
earlier with the argument that an autoconf build needs it — and an autoconf
build calls `ranlib` immediately after `ar`, so staging one without the other
left that argument half-finished.

### The shape of the implementation

Aliases are collected during the manifest pass and created *after* it, because
a producer may be listed below its alias and a link needs its target to exist.
They are hard links (falling back to a copy): both give the program the argv[0]
it reads, and a hard link cannot dangle.

Three refusals, each tested:

- an alias whose producer is **not staged** is **named**, not dropped — a
  manifest promising a name the image lacks is an error, and silence is how it
  stays one;
- an alias colliding with a real binary does **not** overwrite it, the same
  rule the staging collision guard follows;
- a link that cannot be made is reported rather than counted.

Seven assertions added. Neutralising the alias loop fails five of them.

### The bug the unit tests could not see

They all passed, and the feature was broken. The first real image build printed

    [rootfs] NOTE: alias(es) with no staged producer: ranlib(->arstrip = arkillall = kill)

— three alias specs run together into one unparseable line. The accumulator
joined them with `$(printf '<newline>')`, and **command substitution strips
trailing newlines**, so the separator was the empty string.

Every one of the seven assertions used **a single alias**, so not one of them
could distinguish a working separator from no separator at all. *One alias is
not a test of a list.* The regression pin now stages three and asserts all
three exist, are counted, and that none is reported orphaned; restoring the old
expression reproduces the exact string above and fails it.

Worth noting where the fix came from as well: the edit to repair it was
attempted twice by matching the source line as a string and failed both times,
because the Bash tool collapses a doubled backslash before bash sees the
heredoc, so the Python literal arrived as a real newline and matched nothing.
Editing the line by **index** worked first time. Both halves of this — a
shell-quoting hazard in the subject and a shell-quoting hazard in the tool
editing it — are the same hazard, and the second is recorded in this file
already.

**The general point.** This is the session's own lesson applied to my own code:
a thing that has only been tested is not a thing that has been run. The unit
tests were fast, targeted, mutation-checked, and blind to the one property that
mattered. The ~3-minute image build found it immediately.
