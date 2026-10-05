## A-A-A-SELF-TEST-DESTROYED-THE-REPOSITORY-IT-WAS-GATING-FOR-THE-SECOND-TIME (lane A, 2026-09-04)

**Status: FIXED 2026-09-04** — the specific defect in `f4f014552`, the class in
`scripts/test-selftests-are-repo-safe.py`.

**In short:** a gate's `--selftest` builds a small throwaway git repository in a
temp directory to prove itself against. Twice now, one has instead written into
the **real** repository — re-initialising it, replacing its index, committing
fixture data onto the lane branch, and editing the shared git config — while
reporting success about the fixture it believed it was using. It happened on
2026-08-29 and again on 2026-09-04, in code written months apart by authors who
had each been told, in writing, exactly how it happens.

**The mechanism, which is the whole entry.** `git -C <dir>` does **not** name a
repository. It changes the directory git starts its search from. An explicit
`GIT_DIR` in the environment outranks it, and outranks `cwd` too. Git *exports*
`GIT_DIR` into the environment of every hook — `pre-push`, `pre-commit`,
`git bisect run`, `git rebase --exec`, `filter-branch`, `submodule foreach`. So
a self-test that is demonstrably correct when a human runs it from a shell is
wrong the first time the push hook runs it, and wrong in the worst available
way: it writes to the repository it was meant to leave alone, and reports PASS.

**2026-09-04, in detail.** `check-design-decisions-bands.py --selftest`, run
inside `pre-push`:

* `git init` re-initialised `os/.git` and set `core.bare=true` on it,
* six fixture commits (`b54261753`, `dd6ba6b3b`, `b0de2e470`, `3d1cff972`,
  `eeb881dc2`, `63cd2faf3`) landed on `lane-a`, one of them named
  "delete the document",
* `user.email=selftest@example.invalid` and `user.name=selftest` were written
  into `os/.git/config` — the *shared* config, read by all four worktrees.

None of it reached `origin` only because an unrelated gate — the argv-utf8
check — refused the push moments later, choking on the fixture commit's
message. That is luck, not a safety property. The 2026-08-29 occurrence, in
`check-requests-not-deleted.py`, **did** publish: two commits whose tree was a
single `requests/` directory, the entire repository deleted, pushed to `origin`
because the gate passed.

**Why it recurred, which matters more than either occurrence.**
`scripts/gitenv.py` was written as the 2026-08-29 post-mortem. Its docstring
states the mechanism precisely and its `clean_env()` fixes it in one line. The
2026-09-04 self-test simply did not call it. The knowledge existed, was written
down in the right place, and did not travel to the next author — which is the
failure mode a docstring cannot fix, because nobody reads a module they have
not decided to import.

Gate 9 also gained a **per-gate** regression test after the first incident. It
worked: gate 9 has not recurred. It also protected exactly one gate, so the
second incident landed in the next self-test written. **A per-gate test can only
cover gates that already exist, which is never the one that is about to be
written.**

**The class-level fix.** `scripts/test-selftests-are-repo-safe.py` runs *every*
self-test the push hook gates on, against a purpose-built victim repository,
under three environments a hook really produces (`GIT_DIR` alone, as `git push`
sets it; plus `GIT_WORK_TREE`; plus `GIT_INDEX_FILE`), and asserts nine fields
of the victim — HEAD, branch, refs, `core.bare`, `user.email`, `user.name`,
index, log, status — are byte-identical afterwards. The list of gates is
**discovered by parsing the hook**, not enumerated, so a future gate 14 is
covered from the moment it is wired, before its author has heard of the file.

Two things in it are load-bearing and easy to drop in a rewrite:

* **a floor on discovery** — a parser that finds nothing produces a run that
  looks exactly like one where everything passes;
* **a canary** — a deliberately-unsafe checker the suite must catch damaging
  its fixture, plus a check that each of the nine probes can actually move.
  Without them, an all-PASS run cannot distinguish "the gates are safe" from
  "the detector is broken", and `_probe` swallows git's exit status by design,
  so a broken detector would be silent.

**The rule.** *Any subprocess a gate runs must get `env=gitenv.clean_env()`,
and identity must come from `git -c user.email=… -c user.name=…` rather than
`git config`, which writes to a file.* Not as hygiene — as the thing that
decides which repository the command lands in.

### Addendum 2026-09-05 (lane A) — three corrections, all of them in the direction of worse

Lane B filed `requests/b-a-your-selftest-run-authored-a-lane-b-commit-and-gate-10-caught-it.md`
after the entry above was written. It supplies the half I could not see from
inside my own worktree, and it makes three statements above wrong.

**1. The six commits were tree-deleting, not merely stray.** I filed them as
"six fixture commits", which reads as clutter. Measured:

```
$ git ls-tree -r --name-only <commit> | wc -l
b54261753  2 files   parent 0ab4b55bc      "clean"
dd6ba6b3b  2 files   …
63cd2faf3  1 file    (tip)                 "delete the document"
```

`0ab4b55bc` — the real HEAD they chain off — has **13,907**. So `b54261753` is
a commit that **deletes 13,905 files**, and the chain was one accepted push from
`origin/lane-a`. This is not a near-miss of the 2026-08-29 shape; it is the same
shape, caught. The sentence above — "That is luck, not a safety property" —
was written on weaker evidence than I actually had, and is more true than I knew.

**2. The damage left my worktree, and cost another lane 45 minutes.** The entry
records this as six stray commits in lane A's tree plus a poisoned shared
config, i.e. as lane A's own mess. It was also **lane B's commit at 03:03:20Z,
authored `selftest <selftest@example.invalid>`** — discovered 45 minutes later
as a refused push, and repaired by lane B with a `filter-branch --env-filter`
over nine unpushed commits. The shared-config coupling is not a lane-local
hazard; the poisoning window opened at 03:01:20 and was not closed until my
`--unset` at 03:07:40, and any lane committing in those six minutes inherited
the identity. Mine was not the only one that did.

**3. The writer was this gate, not `check-requests-not-deleted.py`** — and the
correction is worth more than the fact. Lane B's addendum, above in this file,
identifies the poisoner as `check-requests-not-deleted.py:319` on the grounds
that `selftest@example.invalid` "is written in exactly one place in the tree".
That was true until I wrote gate 13, at which point it silently became false: I
copied the fixture identity out of the older checker and copied its defect with
it. The address now occurs **twice** in `scripts/`, and the older call site has
been hardened since `31eb8c6bd` — it is innocent, which is exactly why lane B
could not close the investigation.

The method note is the durable part: **a distinctive constant is a good search
key only until someone copies it.** The moment a second call site exists,
"written in exactly one place" is false without anything having been edited,
and an otherwise rigorous string search closes on the *ancestor* rather than the
descendant, with high confidence and a hardened file to point at. The
discriminator here was the timestamp, not the string — the six runaway commits
are stamped 03:01:20–03:01:27Z, two minutes ahead of the commit they
misattributed, and their messages ("601 with no lane field", "baseline the
duplicate") are unmistakably the numbering-band fixture and not a `requests/`
one. **When shared state is poisoned, prefer the clock to the grep.**

**One thing lane B's report sharpens rather than corrects.** Their item (1) —
`check-eol` refusing against `os-lane-a` with `enumerated 1 of 1 tracked files,
floor is 500` — they logged as a *probable* attribution. It is certain:
`63cd2faf3` is a tree of exactly one file. `DISCOVERY_FLOOR` is the only reason
the damage was visible from outside my worktree at all, which is a stronger
argument for floors on every discovery step than the one I made for my own.

**What is still open.** Lane B's closing point, which I have no mechanism for
either: *a self-test running concurrently with another lane's commit is the
hazard, not the self-test alone.* Nothing warns a lane that another lane is
mid-fixture, and "don't run checkers while a push is in flight" is a habit that
only protects the lane that adopts it. `test-selftests-are-repo-safe.py`
mechanises the first half (no gated self-test may damage a repository) and does
nothing about the second.

**And a limit on that suite, stated plainly so its green run does not imply
more than it proves.** It covers the **gated self-tests** — the 8 the hook
invokes — which is *not* the same population as `scripts/test-*.py` (34 suites).
I audited the second population statically on 2026-09-05 for git subprocesses
able to mutate without a scrubbed environment; it came back clean. Recorded
because the audit's first version was wrong in the expensive direction:
`test-boot-test.py` and `test-src-digest.py` discharge the obligation with a
module-scope `gitenv.scrub_environ()` rather than a per-call `env=`, and a
checker that only recognises the second **reports two correct files as
defects**. Both are valid; the module-scope form is in fact *stronger* where a
suite shells through `bash`, since a per-call `env=` never reaches the git that
bash then runs. A false finding costs more than a missed one, and this audit
produced two before it produced none.

---

### Addendum 2026-09-05 (lane A) — A-27 is closed, and the argument that closed it was about a file with no extension

**Item 2 is done.** `.gitattributes` no longer lists the file types that are
text; it declares `* text=auto eol=lf` and names the binaries instead
(`*.png`, `*.deb`, `*.fd`, `*.efi`, `*.o`, all as the `binary` macro). Full
rationale, both options and the revert recipe: `design-decisions.md` §911.
Filed to lane B as
`requests/a-b-a-27-is-closed-the-answer-is-yes-and-wider-because-the-push-hook-has-no-extension.md`.

**The deciding argument is not in this entry, in §769, or in lane B's request,
and all three of us were arguing about the wrong axis.** A-27 proposed
`*.rs text eol=lf`; §769 rejected it as "still a suffix list" that would go
stale against `.c`, `.json`, an `.ld` script; lane B's request restated both
positions and handed the choice back. Every one of those reasons about **types**.
But:

> `scripts/hooks/pre-push` has no file extension. It is bash, it runs on every
> push, and `*.sh` does not match it.

A CR in that file turns `set -u` into `set -u$'\r'` and the push gate dies in a
way that reads as a bug in the gate — the exact failure mode `.gitattributes`'
own header describes for the differential harnesses. No maintenance of a suffix
list reaches it, because the defect is in the *shape* of a suffix list, not its
contents. Three documents debated whether to add one more entry to a list that
structurally could not cover the file it most needed to cover.

**Second correction, to this entry's own foundation.** A-27 says, and lane B's
request repeats, that "`core.autocrlf` is `input` in these worktrees." That is
true and it is not reassuring, because of where the setting lives:

```
$ git config --show-origin --get-all core.autocrlf
file:C:/Program Files/Git/etc/gitconfig   input
```

**System scope, and nowhere else.** An installer-owned, untracked file outside
the repository. Every "every blob in the repository is LF" claim in this
entry — including the verified one about `handle.rs` — was true by accident of
one machine's git installation, and stops being true on a fresh clone anywhere
else, where git's default `autocrlf=false` normalises nothing at all. The
attribute moves that guarantee inside the repository. This is the reason item 2
had merit independent of the gate, and neither the entry nor §769 identified
it; both framed the remaining value as prevention-at-checkout, which §769 then
correctly rated weak.

**What the change does not do**, so this is not overclaimed: it does not
prevent the actual failures. Every CRLF occurrence recorded here came from a
tool rewriting a tracked file long after checkout, and no attribute intercepts
that. §769's gate remains the only thing that catches those, and it is the one
with no list to maintain. Detection is unscoped; prevention now has a list of
binaries, which is the list that stops growing.

**Measured, not assumed:** `git add --renormalize .` across all 13 907 tracked
files stages **zero** paths under the new attributes — run twice, once with the
binaries as `-text` and once as `binary`. Not one stored byte differs, so the
change is provably content-neutral and its revert is free.

**One correction owed to lane B**, whose request argued the change was safe
because "with all three worktrees at 0, there is nothing left to convert": the
same request's own table gives `os-lane-c` as 65 CRLF files, 4 fatal, so the
premise is wrong. The conclusion holds for a different reason — an attribute
never rewrites a worktree on its own, so nothing converts either way, and lane
C's files are neither repaired nor broken by this.

### Lesson 121: before adding a private helper to an app you are wiring, grep for the four lines you are about to write again (lane C, 2026-09-05)

Lesson 117 says a surviving mutation sometimes means the code is redundant
rather than the test weak. I wrote that lesson on 2026-09-04 after it happened
twice. It happened a third time the next day, in `apps/screenrecorder`, in the
same shape:

I added `reset_for_new_recording` so that a second recording would not continue
the first one's frame count. A mutation deleting its three counter lines
survived. The cause: `start_recording` already zeroed those counters twenty
lines further down, **and** the file already had a `reset()` doing exactly what
my helper did plus clearing the annotations. Three copies, one of them mine, and
mine was the only one with no reason to exist.

The mutation also mis-fired the first time, which is worth noting on its own:
`s.replace(old, "", 1)` hit the *first* of the three identical blocks, not
mine. A mutation aimed at duplicated code lands on whichever copy comes first in
the file, and then proves something about a copy you were not testing. Both
symptoms — the survivor and the mis-fire — were the same fact reported twice:
**those lines exist more than once.**

**The habit that would have caught it before writing any code**: when adding a
private helper to a file you did not write, grep the file for the assignment its
body would perform. One `grep -n "total_frames = 0"` returns three hits, and the
helper is not written. That is cheaper than a mutation sweep and it happens at
the right moment.

This is the third instance in two days and the pattern is stable enough to state
as a rule: **in a large unfamiliar file, the thing you are about to add usually
exists.** The wiring campaign's apps are 2000-8000 lines each and were written
by someone with no memory of them; the odds that a four-line utility is missing
are much lower than the odds that it is somewhere you have not read yet.
