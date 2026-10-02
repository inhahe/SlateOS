## 767. Gate 12's scope is computed from the push, not listed — because the list was wrong the day it was written

**Lane:** B
**Date:** 2026-09-04
**Decided by:** Claude (autonomous)

**In short:** Before a push is allowed, one pre-push gate re-compiles the code
you changed for Linux as well as for Windows, because a lot of our code is
written as "do it this way on unix, that way on Windows" and the Windows-only
build we normally run never even looks at the unix half. The question was
*which* crates to put through that second, slower build. The obvious answer is
to keep a list. I counted instead, and the count said a list would be wrong:
the two crates the list was going to name have no unix-specific code at all,
and thirty-two crates that do have it were not on anyone's list. So the gate
now works out for itself, on each push, which of the crates you touched
actually contain unix-specific code, and compiles only those.

### The count

The tech-debt note asked to widen the gate "to `posix`, `userspace/shell` and
the support crates". Measured against the tree on 2026-09-04:

| crate | platform-conditional arms | linux half costs |
|---|---:|---:|
| `userspace/coreutils` | 719 | 2m16s warm |
| `oils` + `cpio` + `stat` | 146 | 5m03s |
| `posix` | **0** | 6m16s |
| `userspace/shell` | **0** | 2m25s |

`posix` is not un-conditional — it has 1,845 `target_os = "none"` arms. But
`"none"` is false on Windows *and* false on Linux, and those arms are already
compiled by the default `x86_64-unknown-none` build. `userspace/shell` is a
159-line toolchain-validation stub with no tests. A second target build of
either compiles the identical source the first one did.

Thirty-two crates in this lane *do* have `cfg(unix)`/`cfg(windows)` arms:
`oils` 108, `cpio` 20, `stat` 18, `htop` 9, and a tail of twenty-eight more.
None was proposed.

### The options

| | *What changes:* |
|---|---|
| **Keep listing, but a better list** | The gate compiles a fixed set of crates. A crate added to the tree tomorrow with a `cfg(unix)` arm is unchecked until someone remembers to add it — and nobody will, because nothing fails when they don't. |
| **Every crate the push touched** | A push editing a crate with no platform-conditional line anywhere in it waits minutes for a build that can find nothing. Most pushes are that push. A gate that charges for nothing gets bypassed, and a bypassed gate checks nothing. |
| **Computed: touched, and has an arm** ✔ | A push pays only for crates where the second target can actually see something the first could not. Adding a crate to the tree adds it to the gate. Nobody maintains anything. |

### Why the filter is what makes it affordable

The middle option is the honest-looking one and it is the one that fails,
because its cost falls on the pushes that get nothing for it. The filter —
`git grep -E 'cfg\( *(not\( *)?(unix|windows)'` against the pushed commit —
answers in milliseconds and turns the common case back into a free one.

Reading it out of the *commit* rather than off the disk is not paranoia: the
gate has already established that the disk equals the push (one ref, sha equal
to `HEAD`, no modified or untracked file), so the two agree — and reading the
commit is what says so in the code.

### Two ordering facts that are easy to get wrong

- **The scope is computed *after* the working-tree-identity precondition, not
  before.** It reads `$pushed_shas` as a single commit, which is only true once
  that block has checked it. Run first, a multi-ref push computes an empty
  scope from a truncated sha and skips *in silence* — swallowing the loud
  decline the author needs in exactly the case they can act on. Getting this
  backwards converts a diagnosable refusal into an invisible non-check, which
  is the whole failure family this gate exists to end.
- **A directory is not a crate.** `userspace/coreutils/tests/` matches the path
  pattern the scope is derived from and has no manifest. Passing it to `--dir`
  is a usage error — exit 64 — and this gate calls the checker with
  `--may-skip`, which tolerates a decline. Without the `-f .../Cargo.toml`
  test, one such directory would have made the gate skip on every push,
  forever, without a word. This is the second time exit 64 has earned its
  separation from exit 2 (see §766's neighbour in the script's header).

### What it does not decide

Not the *dependencies* of a checked crate (`ere`, `quoting`, `bignum`, …): a
dependency whose API changes breaks the host build too, which everything else
already catches, and the arms this gate exists for are the touched crate's own.
Not crates outside lane B's globs — those are another lane's gate to write.
And not the tally name, which stays `coreutils-unix-half` while
`requests/b-a-check-gates-are-wired-cannot-see-a-gate-written-in-bash.md` quotes
those lines to lane A; renaming the text being quoted would make that report
read as already-fixed.
