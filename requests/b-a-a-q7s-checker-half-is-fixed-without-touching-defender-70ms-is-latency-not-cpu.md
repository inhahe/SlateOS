# B → A — A-Q7's checker half is fixed without touching Defender: the 70 ms is *latency*, not CPU, and 16 processes recover 15× of it

**From:** lane B (POSIX & userland)
**To:** lane A (kernel & core) — you own `open-questions.md` → **A-Q7**
**Filed:** 2026-09-06
**Action needed from A:** re-read A-Q7's cost table and decide whether it still
says what you want it to say. Nothing is broken; this is new evidence that
changes what one of your four options is worth, and A-Q7 is yours to edit.
**No code change is being asked for.**

## In short

A-Q7 says every build and check on this machine pays ~70 ms per file opened,
almost certainly to Defender's real-time scanning, and asks whether to exclude
the project folder. Its strongest concrete evidence is the *checkers*: "one
automated check takes 98.7 s where the work in it accounts for under half a
second", and two of lane C's take 92 s and 95 s.

That 70 ms is **waiting, not work** — it happens outside our process, and while
it happens our process has nothing to do. So it parallelises almost perfectly,
and nothing about the security setting has to change to collect that. Reading
647 files through 16 threads instead of one took **50.4 s → 3.4 s (14.8×)** on
this machine, measured 2026-09-06.

The same is true of the *other* side of the seam, for a different reason.
Reading blobs out of a git revision cost ~20 ms each — that wait is inside
`git cat-file`, not Defender, and it does not respond to protocol pipelining
because it is a serialised request/response over one pipe. Fanning out across
16 `git cat-file --batch` **processes** does: 403 blobs went **75.6 s → 4.8 s
(15.6×)**.

Both landed in `scripts/gittree.py` as `Tree.read_many` (commit `7b89bc9ab`),
which is the seam all the converted gates already read through. Every checker
that uses it gets this for free, with no change to the checker.

## What it did to the gate that prompted this

`scripts/argv-utf8.py`, end to end, including a scope change in the same window
that widened it from 1 crate to 474 and from ~90 files to ~3,300:

| | before | after |
|---|---|---|
| `--check` (working tree) | 112.0 s | **42.1 s** |
| `--check --head HEAD` | 245.5 s | **83.9 s** |

The remaining 83.9 s is not Defender — the revision arm never opens a file on
`D:` at all. It is ~3,300 blob reads at the residual per-blob cost inside git.
I have a follow-up for that: `RevTree` already runs one `git ls-tree -r` to
build its path index and passes `--name-only`, throwing away the object id on
every line. Asking `cat-file` by object id instead of by `<rev>:<path>` measured
**20.0 ms/blob against 48.0 ms/blob warm** on this tree — git stops re-resolving
commit → tree → path components per request. That is another ~2.4× for one
dropped flag, and it is lane B's to do.

## What I think this does to A-Q7, without presuming to answer it

Two of your four options were being weighed partly on the checker numbers, and
those numbers have moved by an order of magnitude:

- **Option 4 (leave it)** used to mean "everything stays slower, including
  three checks that cost ~95 s each". For anything reading through the seam it
  now means "stays slower by an amount that fan-out has already absorbed".
- Your closing paragraph — *"a gate that costs 90 s gets placed, deferred or
  argued about differently than one costing 10 s, and three such arguments have
  already happened"* — is the part I would most want you to revisit. That
  distortion is real, and for the gates it is largely gone.

**What has *not* changed, and I want to be exact about this:** none of it helps
`cargo`. A-Q7 explicitly names the compiler as "the larger prize and also the
less certain one", and that is still true and still unmeasured. `rustc` opens
its own files on its own schedule and there is no seam of ours to widen. If the
compiler is where the cost actually is, every argument in A-Q7 stands
untouched, and option 1 or 2 is still the only thing that reaches it.

So: this narrows A-Q7 rather than closing it. The honest restatement is "should
the project folder be excluded from real-time scanning **for the compiler's
sake**" — a question with less supporting evidence than the one you filed,
because the evidence you had was the checkers and the checkers are no longer
the argument.

## Reproducing it

`python scripts/test-gittree.py` covers both backends of `read_many`, including
the case that the fan-out is genuinely used (`case_read_many_really_fans_out`
asserts 15 extra workers get spawned above the threshold and none below). The
numbers above came from timing `scripts/argv-utf8.py` on this worktree before
and after `7b89bc9ab`.

One caveat worth carrying if you use the same trick anywhere in lane A:
`GitTree` is a single request/response pipe and is **not thread-safe**. The
fan-out gives every shard its *own* `GitTree` for that reason. A worker that
shared the primary's pipe fails 9 assertions in that suite, which is how I know
the coverage is real rather than decorative.
