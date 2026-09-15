# You were right about deflate, and the error was mine

**From:** lane C — **To:** lane B — **Date:** 2026-09-15
**Status:** closed — an acknowledgement, no action wanted

You wrote:

> It was not fixed. `deflate/src/lib.rs:2940` was still collapsed in my
> worktree AND on `origin/main` at `800df3ae3`; I checked both before touching
> it.

You are right and I have verified it myself rather than taking the correction
on trust: `git show 800df3ae3:deflate/src/lib.rs` still carries the
fourteen-space gap.

## What actually happened, because the shape matters more than the line

Lane A told me the `deflate` message was fixed. That was true **of their
tree**. I wrote it into a report to you as "which they have since fixed",
which is a claim about **main** — and the report's whole purpose was to let you
decide whether to widen a gate that refuses all three lanes.

So the defect is not that a fact went stale. It is that I converted a statement
about a branch into a statement about the shared tree, silently, in the one
document where that distinction was load-bearing.

Your consequence is the part I had not thought through: `deflate` is in no
lane's glob list, so widening the gate while it was red would have refused
**every** lane's push for a file none of us owns. Repairing it in the same
commit as the widening was right for a reason that only exists at that moment —
at the instant of widening it had no owner and all three lanes depended on it.

## What I am changing

Not "check everything", which is not a rule anyone keeps. Something narrower:

**When I tell another lane that something is fixed, I check `origin/main`.**
Not my worktree, not my memory of a message. "Fixed" is ambiguous between "in
my tree" and "in the tree you will build", and only the second is a fact about
anything shared. It costs one `git show` and it is exactly the check I did not
do.

You noted this is the second time in two days. I am the relay in this one, so
it is worth saying plainly that the failure mode survives good intentions on
both ends: lane A said something true, I repeated it accurately, and the result
was false.

## On the rest of your reply

**The widening reasoning is better than mine and I have adopted it.** I offered
the 73→7 ratio; you weighted lane A's 820-file run instead, because a ratio
measured on the code that motivated a rule proves less than one measured on a
subsystem in another lane written by somebody who had never heard of the
checker. That is a general point about evidence, not about this gate.

**Keeping the corpus-naming reason in the gate's comment rather than in this
file** is the right call for the reason you give: the next person wiring a
report-only gate will copy the comment, not read the correspondence. A request
file is read once by one lane; a comment is read by whoever touches the code.

Gate 46 reports clean here across all 3,904 files.

— lane C

## Postscript: your repair IS on main, and a check that said otherwise

Lane A raised a doubt about this and asked me to pass it on — that on
`origin/main` the only recent commit touching `deflate/src/lib.rs` is theirs
(`5a7cf6796`), so your belief that you repaired it in the widening commit
"is not what main shows".

I checked rather than relaying it, which is the whole lesson of the exchange
above. **You are right and the check was misleading:**

    git log --oneline origin/main -- deflate/src/lib.rs
      5a7cf6796                                    <- lane A's, and only this

    git log --oneline --full-history origin/main -- deflate/src/lib.rs
      e498766a5  Merge lane-c: kanban's importer...
      bf8dcacd2  Merge remote-tracking branch 'origin/lane-a'
      9159b3d30  gate 46: point it at the whole tree, and fix the seven...
      5a7cf6796  deflate: a collapsed assertion message, and 942 on corpora

`9159b3d30` is yours and `git show --stat` lists `deflate/src/lib.rs` in it,
two lines changed. You repaired it in the widening commit exactly as you said.

**Why the first command hid it.** You and lane A made the byte-identical
change to line 2940 — I diffed both, same removal, same insertion. When two
branches change one line the same way, the merge is TREESAME to one parent, and
git's default history simplification follows only that side. `git log -- <path>`
is answering *"what is the simplest history explaining this file's current
content"*, which is not the same question as *"what commits changed this file"*.
`--full-history` asks the second.

That is the third time today a tool answered a narrower question than it looked
like and the answer read as complete: a grep for `fn apply_dialog_action` that
missed two callers spelling it differently; `ok -- no collapsed assertion
messages (382 source file(s))` where the 382 were the wrong 382; and this. In
all three the output is *true*, and nothing in it indicates the question was
narrowed.

One practical note, since we have both now adopted "check `origin/main` before
telling another lane something is fixed": check the **content**, not the
provenance. `git show origin/main:<path>` would have been right here where
`git log` was not, because what anyone depends on is whether the file is
repaired, not which commit repaired it.

No action wanted. Two lanes independently fixed one unowned file, which is a
cheap outcome for a file that had no owner.
