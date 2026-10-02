## 736. The "have I copied this inode?" table is one table shared by the walk, not two split by who asks — the split was hiding a directory the walk could not see

**Date:** 2026-08-30
**Decided by:** Claude (autonomous)
**Lane:** B

**In short:** `cp` has to remember which files it has already copied and where it
put them, so that a file named twice on one command line is not copied twice.
This `cp` kept two such notebooks: one for the "same file named twice" rule and
one for the "same *directory* named twice" rule. The second notebook was in a
place the code that walks *inside* a directory could not read. So when a
directory turned up a second time because `cp` walked into a folder that
contained it, nothing noticed: `cp -r parent/child parent d` copied `child`
twice and reported success, where GNU refuses the second one and exits 1. The
two notebooks are now one, kept where both the operand loop and the walk can
read it — which is where GNU has always kept its single one.

### What was measured

With `parent/child` a directory holding `f` and `g`, and `parent` also holding
`top`:

```
$ cp -rv parent/child parent d          # GNU 9.4
'parent/child' -> 'd/child'
'parent/child/f' -> 'd/child/f'
'parent/child/g' -> 'd/child/g'
'parent' -> 'd/parent'
cp: will not create hard link 'd/parent/child' to directory 'd/child'
'parent/top' -> 'd/parent/top'
rc=1
```

Ours, before this change, printed the four lines, then copied `parent/child`
into `d/parent/child` a second time, and exited 0. The divergence had nothing
to do with `--preserve=links`, which was the option being worked on when it was
found: a plain `cp -r` diverged identically.

Three further facts came out of the same measurement and are all reproduced:

| Command | GNU | Why |
|---|---|---|
| `cp -r tree tree/sub dst` | both copies made, rc 0 | the walk reaches `tree/sub` *first*, and a walked directory is looked up and never recorded, so the operand that names it afterwards finds an empty table |
| `cp -rL tree/sub tree dst` | both copies made, rc 0 | `-L` asks for every name to be followed, so one directory reached twice is a request for two independent copies |
| `cp -rH tree/sub tree dst` | refused, rc 1 | `-H` follows *operands* only; the walk is still `DEREF_NEVER` |

### The decision

Merge `Seen`'s `dirs` field into the `Links` table, rename the result `Copied`
after what it now holds, hang it off `Job` (which the walk can reach) rather
than off `Seen` (which it cannot), and give it GNU's two distinct entry points:
`remember` (insert on miss, return the earlier destination) for a command-line
directory, and `lookup` (no insert) for one found by walking.

This is not a new design; it is GNU's. `copy.c:2662` selects `earlier_file` from
one `src_to_dest` table for both rules, and `hash_init` is called once in `main`
(`cp.c:1284`). The comment that justified the split — "they can never collide, a
directory and a non-directory are different inodes" — was true and beside the
point. The cost of the split was never a collision; it was that one of the two
tables sat behind an interface the walk had no route to.

Three of GNU's four arms for the directory case are reproduced. The fourth,
`cannot copy a directory, %s, into itself, %s`, is reachable in GNU only because
it *also* records the inode of the first destination directory it creates
(`copy.c:2982`) and then trips over it later in the walk. This `cp` deliberately
refuses the into-itself case up front instead, which is §724's decision and is
already covered by seven harness cases; adding the recording to reach the fourth
arm would undo that.

### Why the walk must look up and not record

The asymmetry is the part that is easy to get wrong, and getting it wrong is
silent: recording walked directories as well would make `cp -r tree tree/sub
dst` — the reverse operand order, which GNU copies twice and calls success —
into a spurious refusal. There is a unit test for exactly that
(`a_walk_that_arrives_first_does_not_refuse_the_operand`) and a harness case
beside it, because the failure mode is a *refusal that should not happen*, which
no amount of testing the bug that motivated the change would catch.

The rule underneath it: only a directory named on the command line can be named
twice on the command line. A directory found by walking is found once by
construction, so recording it buys nothing and risks the false positive above.

### The comment that was wrong

`copy_entry` carried this:

> The refusals `copy_one` makes either side of this are deliberately not here,
> and none of them can arise: a source found by walking cannot have been named
> twice on the command line, and nothing this command created can be reached
> inside the tree it is filling.

Both halves of that sentence are true. The conclusion does not follow, because
the directory rule is not asking whether *this* source was named twice — it is
asking whether this *inode* has been copied before, by anyone, including an
operand. The comment has been replaced with one that separates the file rules
(which genuinely cannot arise in a walk) from the directory rule (which can),
and the two absent arms are now justified individually rather than as a class.

The general lesson, recorded because it has now cost two sessions: a comment
asserting that a case "cannot arise" is a claim about the whole program, and the
one place it cannot be checked is the function it is written in. Where such a
claim is load-bearing, it needs a harness case that would fail if it were false.
Sections 9b's six cases are that, for this one.

### What was rejected

- **Passing `Seen` down into the walk.** It would work, and it would carry three
  fields into `copy_entry` of which two provably cannot fire there, plus the
  `Option` wrapper that exists only because `Seen` is built for two-or-more
  operands. The table the walk needs is unconditional; borrowing the one that
  is not would mean making it unconditional too, at which point it is `Copied`
  with extra steps.
- **Keeping two tables and giving the walk its own.** Two tables that must never
  disagree about the same question, which is the shape that produced the bug.
- **Recording the created destination directory (`copy.c:2982`) to reach GNU's
  fourth arm.** It would replace §724's up-front into-itself refusal with GNU's
  refuse-after-copying-some behaviour, whose residue is inode-order-dependent
  and is already an accepted `xfail`. Strictly worse in both directions.
- **Adding `top_level_src_name`/`top_level_dst_name` to `Job` so the two absent
  arms could be written out.** Two mutable fields on a struct eleven signatures
  name, reset per operand, existing only to word diagnostics that cannot be
  printed.
