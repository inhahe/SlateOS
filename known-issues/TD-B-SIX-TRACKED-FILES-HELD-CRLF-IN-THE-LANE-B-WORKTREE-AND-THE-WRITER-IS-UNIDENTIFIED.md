## TD-B-SIX-TRACKED-FILES-HELD-CRLF-IN-THE-LANE-B-WORKTREE-AND-THE-WRITER-IS-UNIDENTIFIED (lane B, 2026-09-04)

**In short:** the boot test refused to build because six files declared
`text eol=lf` in `.gitattributes` had Windows line endings (CRLF) on disk. The
bytes are repaired. **What wrote them is not known**, and this entry exists
because repairing bytes without finding the writer only schedules the next
occurrence — as `check-eol`'s own refusal message says.

**Read the three corrections at the end before acting on anything above.** The
count in this entry's title is wrong (six were *declared*; twenty-seven more
were invisible to the gate as it then was), the mtime table below does not mean
what it says it means, and both named suspects have since been ruled out. The
title is left alone because the entry is referenced by that ID.

**Why no git command showed it.** `text eol=lf` installs a *clean filter*: git
converts CRLF to LF before every comparison, so `git status`, `git diff` and
`git add` all called the tree clean, and staging the repair produced a
zero-byte diff. `git ls-files --eol` is the only view that shows it, and it
read `i/lf w/crlf` — index correct, working tree corrupt. This is why
`scripts/boot-test.sh`'s `check-eol` gate reads the bytes itself rather than
asking git, and it is the reason the gate is worth its runtime.

**The six, with the mtimes that bound when it happened:**

| file | CRs | mtime |
|---|---|---|
| `requests/a-b-run-ki-dupes-after-merges.md` | 105 | 2026-08-16 15:45 |
| `requests/a-b-set-credentials-right.md` | 151 | 2026-08-16 15:46 |
| `scripts/getopt-ambiguity-check.py` | 837 | 2026-08-30 08:08 |
| `scripts/check-libc-shape.py` | 1032 | 2026-09-03 21:41 |
| `scripts/check-mutation-needles.py` | 400 | 2026-09-03 22:22 |
| `scripts/check-doc-links.py` | 1505 | 2026-09-03 23:03 |

~~Four distinct dates over nineteen days: this is recurring, not one event.~~
**Struck — the mtimes do not support this. See the third correction at the end
of this entry: an mtime is the last touch by any tool, not the CRLF-introducing
write, and the experiment that shows it.**

**What has been ruled out.**

- **Not the blobs.** `git ls-files --eol` reports `i/lf` for all six; the
  repository content is correct and always was. Only the working tree was
  wrong, so nothing was ever pushed in this state.
- **Not `git checkout`.** `core.autocrlf` is `input` — set at *system* scope,
  unset at local, global and worktree — and `core.eol` is unset everywhere.
  `input` means convert on commit, never on checkout, so checkout writes the
  blob verbatim.
- **Not my edits this session.** All six mtimes predate 2026-09-04, and an
  `Edit`-tool write to one of them afterwards left it `w/lf`.
- ~~**Not a tree-wide tool.** The `os` integration worktree has **0** CRLF
  files across the same 1,065 tracked files. Whatever did this ran in the
  *lane-b worktree specifically*.~~ **Wrong — struck out, see the correction
  at the end of this entry. It is not lane-b-specific.**
- **Not the obvious text-mode write.** `check-doc-links.py:1221` does open a
  file `"w"` without `newline=""`, which on Windows is exactly the CRLF-making
  pattern — but its path is inside a `tempfile.TemporaryDirectory()`, so it
  cannot touch a tracked file. Named here so the next reader does not spend
  the same twenty minutes on it. `check-libc-shape.py` and
  `getopt-ambiguity-check.py` contain no writes at all, and
  `check-mutation-needles.py:314` also writes only into a temp dir.

**The one partial explanation.** The two `requests/` files share a commit:
`6caf6467e`, *"repair: restore the tree that the buggy `--selftest` deleted, on
lane-b too"*. A repair that restores deleted files by writing them through
Python's default text mode would produce exactly this, and it targeted the
lane-b worktree by name — which fits the lane-b-only scope. That accounts for
2 of 6 and for the 2026-08-16 pair. **It does not account for the four
`scripts/*.py` files**, and no hypothesis tested so far does.

**Struck as well.** `6caf6467e` touched 13 690 of 13 908 tracked files, so it
contains the affected ones the way it contains almost everything. See the third
correction for that measurement and for the second hypothesis that also failed.

**How to find it if it recurs.** ~~The mtime is the timestamp of the write, so
`ls -l --time-style=full-iso` on the offenders gives the minute; correlate that
against what was run then.~~ **Struck: this method does not work, and the third
correction below shows why by experiment.** What to do instead: the CR is
invisible to `git status`, so the only way to bound *when* is to catch it
narrowly — run `python scripts/check-eol.py` (which now reads every tracked
file) before and after a suspected tool, and diff the finding lists. A
before/after pair around one command is the evidence the mtimes were mistaken
for.

**If it is never fixed:** every recurrence blocks `scripts/boot-test.sh` for
whichever lane hits it, and the gate refuses the *whole build*, so it blocks
merging too. It is loud and self-repairing-by-hand, not silent — the failure
mode is lost time, not lost correctness.

### CORRECTION, before this entry was an hour old: it is not lane-b-specific

The "Not a tree-wide tool" bullet above is **struck out because it was false**,
and it is left visible rather than deleted because how it got here matters more
than the claim. It was published on a partial reading: the command that
produced it queried three worktrees, only the `os` result returned before I
drew the conclusion — the other two auto-backgrounded — and I read "os is
clean" as "only lane-b is dirty". The full result:

| worktree | tracked files with CRLF | of those, declared `eol=lf` |
|---|---|---|
| `os` (integration; nobody develops in it) | **0** | 0 of 1444 |
| `os-lane-a` | **168** (166 `.rs`, 1 `.ads`, 1 `.atp`) | **0** of 1443 |
| `os-lane-b` | 6 | 6 — the six above; now 0 of 1446 |
| `os-lane-c` | 66 | **31** of 1433 |

**The true shape is the inverse of what I wrote: every worked-in tree has it;
the one tree nobody develops in is clean.** And the corruption sits in each
lane's *active area* — lane A's under `kernel/`, which is exactly what lane A
is working on; mine in files that had just arrived from lane A. A checkout, a
merge, or one lane's script would not produce that distribution. Something in
the development loop itself writes tracked files through a text-mode handle.
Both agent file-writing tools are excluded for lane B at least: an `Edit`
applied to one of the six afterwards left it `w/lf`, and a `Write` creating a
new `requests/*.md` — a path covered by `eol=lf`, in the same worktree, the same
session — also produced `w/lf` with zero CRs. Whatever writes these is not the
agent's own editor.

### The correction needed a correction: the second column is the one that matters

Everything above the table survived; the table's first version did not. It read
`os-lane-a` = "~50, of which 44 in `kernel/src/fs`" — wrong twice over, and I
had *already flagged* a 169-vs-50 discrepancy between two measurements and
published the smaller number anyway. The real count is **168**, and the
`~50` came from a `head`-limited view of a per-directory breakdown. When two
measurements of one quantity disagree, the disagreement is the finding; picking
one and moving on is not a resolution.

The consequence I drew from it was also false, and more expensively so. I wrote
that lane A's dirty files "mean `check-eol` will refuse *their* build too."
**They do not.** `check-eol.py` does not look for CRLF; it looks for CRLF *in
files `.gitattributes` declares `text eol=lf`*, which is `*.sh`, `*.py`,
`*.yaml`, `*.yml`, `*.md`, `*.txt` — **and not `*.rs`**. All 168 of lane A's
report `attr/` empty. Lane A's build is not at risk and never was.

**The lane that is actually blocked is lane C**, which I had written off with a
bare "66". Thirty-one of those 66 are declared `eol=lf` — 17 `requests/*.md`,
8 `apps/*/mutate.py`, 6 `scripts/*.py` — so `scripts/boot-test.sh` will refuse
lane C's build at `check-eol`, and lane C has no `known-issues.md` entry and
nothing in `open-questions.md` about line endings. Filed as
`requests/b-c-thirty-one-crlf-files-will-refuse-your-next-boot-test.md`.

**The error underneath both of these is one conflation:** "has CRLF" is not
"violates the promise". The gate's scope *is* `.gitattributes`' promise, by
design and by its own docstring — so a census of CRLF files answers a different
question than the one I was asking, and ranking the lanes by it put them in
very nearly the reverse order. Counting the right population would have cost
one extra pathspec on the same command.

**This is not new, and lane A wrote it down seventeen days ago.**
`A-27-KERNEL-SOURCES-ARE-CRLF-IN-THE-WORKING-TREE-WHILE-EVERY-BLOB-IS-LF`
(2026-08-18) is the same phenomenon, diagnosed to the same cause — "written by
something that opened them in Python text mode on Windows" — and marked
**"tooling fixed; the divergence itself remains."** It listed **27** files. The
same population today is **166**. It also named the durable fix (a root
`.gitattributes` carrying `*.rs text eol=lf`) and deferred it on one stated
objection: that `.gitattributes` is a shared root file and "needs to be
coordinated, not dropped in by one lane — file a request or raise it in
`open-questions.md` first." No request was ever filed, and the objection has
since expired without anyone noticing: a root `.gitattributes` **now exists**
and already carries six rules that three lanes depend on. What A-27 was waiting
for arrived; nothing re-checked the entry that was waiting for it.

That is its own lesson, and a more useful one than the arithmetic: **a deferral
whose trigger condition is written in prose is a deferral nobody will notice
has fired.** A-27's condition ("once a shared `.gitattributes` exists") became
true and the file grew 6× behind it. This is exactly the shape
`deferred-questions.md` exists to hold — an explicit trigger, in a file that
gets re-read — and A-27 predates that file, so it never got one.

**The lesson is the same one as Lesson 112 (lane B)'s postscript, and I had already
written that postscript before making this mistake.** There the truncated view
was `| head` on a process query; here it was an auto-backgrounded command whose
first line arrived and whose remaining two did not. Both times the partial
output was *coherent* — it read like a complete answer — and both times I drew
a confident conclusion from it and acted. The rule generalises past process
queries: **when a command surveys N things, do not conclude anything until you
have counted N results.** A survey that returns 1 of 3 rows is not weak
evidence for the other two; it is no evidence at all.

### Third correction, same day: the forensic method this entry recommends does not work

Everything above rests on one unstated assumption, and it is false. **"The mtime
is the timestamp of the write"** — the first sentence of *How to find it if it
recurs* — is wrong, and the table of six mtimes it justifies cannot bear the
weight put on it.

Disproved by direct experiment rather than by argument. A census at 21:46:29
recorded `userspace/authlib/src/faillock.rs` as `w/crlf`. I edited that file at
22:07:49, which updated its mtime by twenty-one minutes. It was **still**
`w/crlf`. An editor preserves a file's existing line endings, so a write leaves
the mtime fresh and the CRLF untouched: the mtime records the last touch by *any*
tool, which can be arbitrarily later than — and entirely unrelated to — the
write that introduced the CR.

The casualty is the inference **"four distinct dates over nineteen days: this is
recurring, not one event."** That is unsupported. Six files could have been
corrupted in one event and subsequently touched on four different days; the
mtimes are consistent with that and cannot distinguish it. The conclusion may
still be true — it is simply not evidence for it. Struck, not deleted, for the
same reason as the first correction.

**Two hypotheses for the writer were tested and both failed.** Recorded because
a hypothesis that died quietly gets re-proposed:

1. **Commit `6caf6467e`** (the "one partial explanation" above). It does contain
   all the affected files — but it touched **13 690 of 13 908** tracked files, so
   containing them is not a property of the commit, it is a property of its size.
   An overlap that a coin flip would also produce is not evidence. I had the
   sentence "all 27 of 27 are in `6caf6467e`" drafted before checking the
   denominator.
2. **`scripts/strip-workspace-sections.py`.** It genuinely wrote CRLF (fixed —
   see below), and every one of the six affected manifests had been through it.
   But the **control group refutes it**: the LF manifests had been through it
   too, with the same history. A predictor that fires equally on both classes
   predicts nothing.

The one correlation that survived is weak and is left as a lead rather than a
finding: the four `scripts/*.py` mtimes cluster near the 2026-08-24 coreutils
sweep. Given the paragraph above, "cluster near" is doing very little work.

**The counts in the table above are the wrong population, and the previous
correction's conclusion is now itself superseded.** That correction ended by
insisting the *second* column — "of those, declared `eol=lf`" — was the one that
mattered, because the gate's scope was `.gitattributes`' promise. That was
correct about the gate as it then stood. §769 changed the gate: `check-eol` now
reads **every tracked file**, so the *first* column is the one that matters, and
the ranking flips back. Restated with the widened gate, measured 2026-09-04:

| worktree | tracked files with CRLF | fatal (executed from disk) |
|---|---|---|
| `os-lane-a` | **0** (was 168; lane A renormalised) | 0 |
| `os-lane-b` | **0** (was 6 declared + 27 undeclared + 22 binary) | 0 |
| `os-lane-c` | 65 | 4 |

Lane B's real figure was never 6. Six were *declared*; twenty-seven more were
`.rs`/`.toml` that the old gate had no reason to open, and a further twenty-two
are binaries whose CRs are not corruption at all. All twenty-seven are repaired,
with `git hash-object` equal to the index OID before and after — a repair with
provably zero content change, because the clean filter means the blob never
differed.

**The writers are fixed even though the writer is not identified.** Two scripts
in this tree really did emit CRLF into tracked files through Python's default
text mode. Neither is proven to be *the* writer — see hypothesis 2 — but both
were capable of it, so the population of possible causes is smaller by two
regardless:

- `scan-orphan-modules.py`, which rewrote its own tracked `.txt` baseline —
  a file `check-eol` itself reads. Fixed in `825acee84`; converged with an
  independent fix from `origin/main` on merge, both adding `newline=`.
- `strip-workspace-sections.py`, which rewrote every sub-crate `Cargo.toml`.
  I fixed it in `825acee84`; **`origin/main` deleted it instead (`291ca193b`),
  and deletion is the better answer.** It was a one-shot written for the
  2026-08-13 workspace consolidation, that migration is not repeatable, and
  `git grep` found no caller — not `boot-test.sh`, not a hook, not CI. A script
  nobody runs cannot earn the audit its two write sites would need, and leaving
  it in the tree looking like a tool is itself the hazard. The deletion was
  taken on merge and my repair discarded.

The second one is worth noting as a pattern and not just an outcome: two lanes
found the same defect within hours and picked repair versus deletion. **Repair
was the reflex and deletion was the better move**, because it also removes the
chance of a future run. The question to ask before fixing a script is whether
anything invokes it.

**A-27's "Still to do" is now partly closed.** Item 1, "normalise the 27 files",
is done for lane B, and lane A's own worktree measured 0 of 13 907 on
2026-09-04, so it is done there too. Item 2, "add a root `.gitattributes`
(`*.rs text eol=lf`)", is **superseded for visibility and still open for
prevention**: §769 makes the gate see every tracked file without touching
`.gitattributes`, which removes the reason A-27 wanted the change but not the
change's own merit at checkout time. That half remains lane A's to decide, and
it is deliberately not being made by lane B — it is a three-lane shared file, and
A-27's original objection to one lane dropping it in still holds.
