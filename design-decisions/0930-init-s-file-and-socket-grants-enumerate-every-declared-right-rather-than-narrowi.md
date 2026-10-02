## §930 — init's `File` and `Socket` grants enumerate every declared right rather than narrowing to the plausible ones

**Date:** 2026-09-11
**Decided by:** Claude (autonomous) — completes §928, whose own text named this as
the deliberately-unfinished half and said to reuse its shape.
**Lane:** A

**In short:** the kernel hands out permissions as tokens, and the first userspace
process was given "all of them" as a wildcard for files and for network sockets.
That sounds like a security problem and is really a *bookkeeping* one: the wildcard
means the instant a programmer invents a new permission, that process already holds
it, before anyone has decided whether it should. The fix replaces the wildcard with
a written-out list. The choice recorded here is what goes in the list — and the
answer is **everything that exists today**, which changes nothing about what init
can do. The only thing that changes is that the *next* permission has to be granted
on purpose.

### The decision

`(ResourceType::File, 0, Rights::ALL)` and the `Socket` equivalent become
`Rights::INIT_FILE` and `Rights::INIT_SOCKET`, each enumerating all fifteen declared
rights. The existing `const _: ()` pin on `Rights::DISTINCT.len()` now names all
three lists, so adding a right fails to compile until someone decides, per class,
whether init holds it.

### The alternative, which is tempting and was rejected

Enumerate only the rights that *mean* something for each class — drop `SIGNAL`,
`WAIT` and `DEBUG` from `INIT_FILE`, drop `EXECUTE` from `INIT_SOCKET`, and so on.
That is a genuine tightening rather than bookkeeping, and it is where this should
eventually go.

It was rejected **for this commit** because of how the check is written:
`pcb::has_capability_type(pid, type, rights)` consults no resource id, so a
class-wide grant answers any query for that class. Omitting a right from
`INIT_FILE` therefore silently denies every `File`-typed query for it — and which
queries actually occur is a question about the whole tree, answerable with evidence
rather than with a guess about which rights "obviously" do not apply to a file. A
narrowing that denies something real fails at a capability check deep in a syscall,
which is a poor place to learn you were wrong.

So: two changes, not one. This is the one that cannot break anything, and it is
worth having on its own because the wildcard-versus-list property it buys is
independent of how tight the list is.

| | wildcard `ALL` | enumerate all 15 (chosen) | enumerate a narrowed subset |
|---|---|---|---|
| init's authority today | everything | **identical** | reduced |
| a new right reaches init | automatically, silently | only if written down | only if written down |
| can break a working syscall | no | **no** | yes, at a capability check |
| needs evidence first | no | **no** | yes |

### The duplication is the mechanism

Three near-identical fifteen-line lists invite a reader to factor them into one
constant derived from `DISTINCT`. **That would re-create the exact defect**: a new
right would join the derived constant automatically and nobody would be asked. The
pin's message now says so in the place a deduplicating reader will be standing.

This is the same shape as §928's finding that enumeration alone was insufficient —
a bit added to `DISTINCT` and forgotten in a list is silently *not* granted, which
is the opposite failure and just as quiet. The pin is what makes the decision
compulsory in both directions; the lists only make it expressible.

### Verification

§928 was verified by planting a sixteenth right and watching `cargo check` refuse.
The same was done here, because **a compile-time assertion that has never fired is
indistinguishable from one that cannot** — and that distinction has been the
recurring defect in this tree all week. The planting simulates what a real author
does: declare the right, add it to `DISTINCT`, and widen the array length so that
edit compiles, leaving the pin as the only thing in the way.

Measured in three stages: clean build **exit 0** → sixteenth right planted,
`cargo check -p kernel` **exit 101** with `evaluation of cap::rights::_ failed`
and the new message naming all three classes → planted right removed, **exit 0**
again. The middle stage is the only one that proves anything and the only one it
is possible to skip.

### A note on the evidence rather than the result

The three build logs were written to `/tmp/c1.log`–`c3.log`, and `/tmp` on this
machine resolves to `C:/Users/inhah/AppData/Local/Temp` — **one directory shared by
all three lane sessions and by every other program on the box**. Reading those
files mid-run returned another process's output: a `TESTEXIT=0` line that no part
of this belongs to, and a `dev` profile line from a `--release` build.

What saved the verification was luck in two places. The exit codes were echoed
into the task's own harness-owned output file rather than read back from `/tmp`,
and `SEPARATELY FOR EACH OF THE THREE` is a string nothing else on the machine
emits, so the two pieces of evidence that mattered happened not to be the
corruptible ones. **Build logs belong under the worktree's own gitignored
`build/`, which is per-lane by construction.** A shared scratch directory is the
same hazard as a shared `HEAD`, which this project already solved with worktrees.
