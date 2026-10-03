## 757. The shared remover takes a projection of `rm`'s options, not `rm`'s options — and the three fields it cannot see are exactly the three that mean "operand"

**Date:** 2026-09-04
**Decided by:** Claude (autonomous)
**Lane:** B

**In short:** deleting a folder and everything in it was written out twice in
our command-line tools — once in `rm`, once in `mv` (a move to another disk is a
copy followed by deleting the original). Two copies of the same thing drift, and
these two had already disagreed within a day of the second one being written.
The two are now one shared piece of code. The decision recorded here is the
narrow one about *how* the shared piece is told what to do: `rm` hands it a
smaller struct containing six of its nine settings, rather than handing over the
whole command line and trusting the shared code never to look at the other three.

**Date:** 2026-09-03. Closes
`known-issues.md` → `TD-B-TWO-RECURSIVE-REMOVERS-NOW-EXIST-IN-COREUTILS`, and
supersedes the "keep two walks for now" half of §751.

### The three options

| | what the walk receives | what stops it reading a field it must not |
|---|---|---|
| **A. the parsed command line** (`&Rm` / `&Options`) | all nine options | a comment |
| **B. a projection** (`remove::Opts`, six fields) | only the six | the type |
| **C. a policy trait** (`dyn RemovePolicy`) | callbacks | the type, plus a vtable |

**B was taken.** The argument is not "smaller is nicer"; it is that the three
fields left behind are not an arbitrary six-of-nine cut. `preserve_root`,
`preserve_all_root` and `presume_tty` are each decided **per command-line
operand**: `--preserve-root` refuses `/` *as typed*, `--preserve-root=all`
compares an operand against its own parent, and `-I` asks one question about the
*batch* the user named. An entry discovered by walking is not an operand and
never can be, so those three have no meaning below the top level. The other six
— `-r`, `-d`, `--one-file-system`, `-v`, the interactive mode and `-f` — are
tested at every level.

So the projection is not a filter applied to a struct; it is the seam the
options already had. That it comes out as exactly the fields `Rm` retains is the
check: between `Rm` and `remove::Opts` every parsed option appears once and once
only, and a future option lands on one side or the other by answering a single
question — is it about the thing the user typed, or about a thing found by
walking?

### Why not A, which is what the two-walks state effectively was

A is one comment away from correct, and the comment does not hold. §751 recorded
the reason the second walk was written rather than the first one reused: `Rm`
carries eight knobs `mv` has no answer for, so reuse means either inventing
eight values or splitting the walk off the struct. Passing `&Rm` is the
"inventing values" branch wearing the other branch's clothes — `mv` would
construct an `Rm` with three fields set to whatever is inert and a comment
saying the walk does not read them. The entire cost of the duplication being
paid off here was that a rule lived in one copy and not the other; a comment
asserting a struct field is unread is the same class of guarantee that failed.

### Why not C, which is the shape §750 chose for the copy engine

C is genuinely better where the *policy differs*. `copy.rs` took a trait object
for its run's stdout because `mv` really does need a different sink from `cp`'s.
Here the policies do not differ: `mv`'s options are `mv.c:87`'s
`rm_option_init`, a fixed struct, with `verbose` copied from `x.verbose` at
`mv.c:238` — one varying bit out of six. A vtable to express one bit is an
indirection charged on every entry of every tree for a difference a `bool`
already states, and it moves the six knobs from a place where they can be read
in one glance into six methods that must be traced to their implementations.
Upstream agrees by construction: `remove.c` takes a `struct rm_options *`, not
a callback table.

### What the decision costs

`rm::Options` and `remove::Opts` name six fields each, so an option added to the
walk is added in two files and forwarded in a third place (`walk_opts`). That is
real, and it is the price of the guarantee: the forwarding is where the compiler
gets its chance to notice. The alternative pays nothing at the definition and
everything at the next divergence.

One knock-on: `rm.rs`'s private `Interactive` enum was deleted rather than kept
alongside `remove::Interactive`. Keeping both would have meant a conversion
function between two enums with identical variants — a second place for the
`WhenTty`/`Once`/`Always`/`Never` rules to disagree, in a change whose whole
purpose was to delete such places. The rename it forced (`Default` → `WhenTty`)
touched a dozen lines and no behaviour.
