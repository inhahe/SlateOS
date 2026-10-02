## A-A-CRLF-CORRUPTION-IS-INVISIBLE-TO-EVERY-GIT-COMMAND-ANYONE-RUNS (lane A) — FIXED 2026-09-03

**In short:** thirteen files in this worktree had Windows line endings
(CRLF — a carriage return before every newline) where the project requires
Unix ones (LF alone). The boot test went red on them 45 minutes in. The
entire time, every command you would run to ask "is my working tree dirty?"
answered **clean** — `git status`, `git diff`, `git diff --quiet`, and
`git add`. That is not a bug in git and it is not something I failed to
check; it is what those commands are specified to do, and it means a whole
class of corruption is invisible where everyone looks for it. A new gate,
`scripts/check-eol.py`, now checks the thing git will not.

**Why git says clean.** `.gitattributes` declares `*.sh`, `*.py`, `*.yaml`,
`*.yml`, `*.md` and `*.txt` as `text eol=lf`. That declaration installs a
**clean filter** — a normalisation git applies to a file's bytes *before*
comparing it to the stored version. The filter strips CR. So the comparison
that `git status` and `git diff` perform is not "do the bytes on disk match
the bytes in the index?", it is "do the bytes on disk **after normalisation**
match?" — and a wholly-CRLF file normalises to exactly the stored content.

The proof, and it is worth keeping because it is counter-intuitive: I
repaired all thirteen files and staged the repair. `git diff --cached`
was **zero bytes**. Thirteen files rewritten, nothing to commit. The CRLF
had never reached a commit in the first place, which is also why `main` was
never red and why the initial hypothesis — "this came in through a merge, so
every lane is blocked" — was wrong.

| command | sees raw bytes? | verdict it gave |
|---|---|---|
| `git status` | no (filters) | clean |
| `git diff` | no (filters) | empty |
| `git diff --quiet` | no (filters) | exit 0 |
| `git add` + `git diff --cached` | no (filters) | empty |
| `git diff-files` | **yes** | modified |
| `git ls-files --eol` | **yes** | `i/lf w/crlf` |

The last two are the only ones that disagree, and nobody runs them.
`git ls-files --eol` is the one to remember: it prints the index encoding
and the working-tree encoding side by side.

**The deeper point.** `text eol=lf` is a promise git keeps *at checkout*.
It is not an invariant git enforces afterwards. Anything that writes a file
after checkout — a script, an editor, a code generator — can violate it
freely, and git's answer to "did anything change?" is designed to say no.
So the declaration reads like a guarantee and is a one-time conversion.

**What was caught, and what was not.** `check_shellcheck` found this,
because shellcheck reads raw bytes and chokes on the CR. But it covers
`*.sh` only: **1 of the 13** corrupted files. The other twelve were `.py`
and `.md`, and nothing in the tree looked at them. A whole-tree corruption
presented as a one-file typo — which is why the new gate covers every
declared file rather than the extension that happened to catch it. What
matters about a finding here is not the file; it is that some tool in the
tree is writing text in the wrong mode.

**The fix (`scripts/check-eol.py`, wired second in the boot-test sweep).**
Asks `git check-attr` which paths are declared `eol=lf` — rather than
matching suffixes, so the policy has one home and cannot drift — reads each
one from disk, and reports any CR. 1438 files, ~32s with a 16-thread pool.
It runs second, after `check_requests_not_deleted`, because the cost of
learning this late was 45 minutes.

**Still open — the actual root cause.** Roughly 180 further files are
wholly CRLF on disk *and not declared* `eol=lf`, so the new gate ignores
them by construction: most of `kernel/src/fs/*.rs`, plus
`kernel/src/crypto.rs`, `kernel/src/syscall/handlers.rs`, `kernel/build.rs`,
`bench/baselines.toml`, and `kernel/ada/*`. `.gitattributes` declares no
rule for `*.rs`, `*.toml`, `*.ads` or `*.atp`.

That distribution is the evidence: whole directories of generated-looking
source, in one mode, is what a Python script opening files with
`open(path, "w")` on Windows produces — text mode translates newline to
CRLF silently. **Finding that writer is the fix that matters**; repairing
files without it just schedules the next occurrence.

**The mechanism is no longer a hypothesis — it was reproduced the same day,
by me, in this repository.** Hours after the gate was written I edited
`scripts/check-gates-can-refuse.py` with a short Python script ending in
`p.write_text(s, encoding="utf-8")`. `Path.write_text` opens in **text
mode**, and on Windows text mode rewrites every newline to CRLF. That one
call put 649 carriage returns into one script and 461 into another.
`git status` called the tree clean, exactly as documented above, and
`git commit` accepted it without complaint — the clean filter normalised the
content on the way in, so the *commit* is correct and only the working tree
was wrong. The new gate caught it on its first day, in the field, against
real corruption rather than a fixture.

Two lessons worth carrying:

- **`Path.write_text()` and `open(p, "w")` are the bug.** Any script here
  that writes a tracked file must pass `newline=""` or write bytes. This is
  not a Windows quirk to catch at review time; it is the default behaviour of
  the most obvious API, which is why it keeps happening.
- **`git commit` will not stop you.** The filter makes the committed object
  correct, so the corruption never appears in history and lives only on disk
  — where the next tool that reads raw bytes (shellcheck, a compiler, a
  parser) trips over it.

An aside on the repair, because it costs time every occurrence: after
rewriting the bytes correctly, `git status` reports the file as ` M` with a
**zero-byte diff**. That is the same disagreement pointing the other way —
the stat cache sees changed raw bytes, the content comparison sees none.
`git add` or `git update-index --refresh` settles it; nothing is actually
staged, because there is nothing to stage.

Two candidate follow-ups, in order:

1. Find the writer. Search the tree for text-mode `open()` calls without an
   explicit `newline=""` (or a binary `"wb"` mode) in anything that emits
   `.rs`, and check the generators under `kernel/` first, since that is
   where the residue is.
2. Decide whether `.gitattributes` should declare `*.rs`/`*.toml` too. It
   probably should — but that is a whole-tree normalisation touching all
   three lanes, so it wants a request rather than a unilateral commit.

### Follow-up 1 is done: the writers, found — 2026-09-04

Method: parse every `scripts/**/*.py` and report each `Path.write_text(...)`,
`p.open("w"/"a")` and `open(p, "w"/"a")` that does **not** pass an explicit
`newline=`. **79 sites in 28 scripts** — 40 `write_text`, 39 `open()`.

47 of the 79 write into a `tempfile` directory. Of the remaining 32, three
write files that are actually in the repository:

| Site | Target | Tracked? | Declared `eol=lf`? |
|---|---|---|---|
| `check-selftest-reinit.py:327` | `scripts/selftest-reinit-baseline.txt`, on `--pin` | **not yet** | **yes**, on creation |
| `scan-orphan-modules.py:602` | `scripts/orphan-modules-baseline.txt`, on `pin` | yes | **yes** |
| `strip-workspace-sections.py:80` | `{apps,userspace,gui,init,net}/*/Cargo.toml`, **in place** | yes | **no** |

*Corrected 2026-09-04.* The first row originally read "Tracked? yes". It is not:
`git ls-files scripts/selftest-reinit-baseline*` is empty and the path does not
exist on disk — nobody has run `--pin` yet. `.gitattributes:41` (`*.txt text
eol=lf`) covers it in advance, so `git check-attr` already answers `text: set,
eol: lf` for a file that does not exist. Verified by direct query, not inferred.

That distinction changes what happens, so it is worth stating rather than
patching over:

| | `orphan-modules-baseline.txt` (tracked) | `selftest-reinit-baseline.txt` (not yet) |
|---|---|---|
| born CRLF by `write_text` on Windows | yes | yes |
| in `check-eol.py`'s subject set | **yes** — declared ∩ tracked | **no** — declared but untracked |
| so a CRLF rewrite is | caught on the next run | invisible until someone `git add`s it |

Which means row 2 is the one this entry's fix already covers, and row 1 is a
**latent** instance: it is fine precisely because the feature has never been
used. The first `--pin` writes CRLF; the first `git add` of that output hides it
(the clean filter normalises the index copy, so `git status`/`git diff` stay
silent); and only then does the file enter `check-eol.py`'s subject set and the
gate start reporting it — after the corruption, not before. Being caught late
still beats not being caught, but the ordering is worth knowing: this gate
detects, it does not prevent.

Both rows are the exact signature of this entry — a declared-LF file rewritten
after checkout with git silent about it — and both are `--pin` paths, run
deliberately then committed, which is precisely how a corrupted worktree
survives a commit and still looks clean.

The third is worse in kind and lower in risk. It is a `read_text` → `write_text`
round trip, so it is **not idempotent on Windows**: the read normalises CRLF to
`\n` and the write turns every `\n` back into CRLF, converting an LF file to CRLF
whenever the script changes anything at all. It aims at `*.toml`, which is *not*
declared `eol=lf` — `git check-attr text eol -- apps/calc/Cargo.toml` answers
`unspecified` for both, checked 2026-09-04 — so its targets are tracked but
outside `check-eol.py`'s subject set, and the gate cannot see the damage it
does. That is the third row's real hazard: unlike rows 1 and 2, nothing would
ever report it. It is also
**orphaned** — no reference to it exists anywhere under `scripts/`, nor in any
tracked `.md`, `.txt`, `.sh` or `.toml` — and it is a one-shot migration script
from the original workspace setup.

**Another lane hit the same mechanism a fortnight earlier and did not find the
writer either.** See
`B-THE-FIXTURE-STAMP-HASHED-WORKTREE-BYTES-SO-IT-DID-NOT-SURVIVE-A-CHECKOUT`
(lane B, 2026-08-16) above, under "How widespread the CRLF is, since the next
reader will ask": a survey of 13,145
tracked files in lane B's worktree found 70 CRLF files, *every one* a
`services/*/build.py`, all attributed to "whatever generated those files in text
mode". Lane B fixed the **consequence** — the hash rule — and the writer went on
running. That is the argument for fixing writers instead of consequences: the
consequence-fix protected hashing and left every other reader of raw bytes
exposed, and on 2026-09-03 `shellcheck` was the reader that tripped.

**One correction to lane B's reasoning, since it bears on follow-up 2.** That
entry declines `.gitattributes` on the grounds that "a config that governs
checkout cannot fix a file rewritten post-checkout". The premise is right and the
conclusion does not follow, because prevention is not what the declaration buys.
A declaration is what makes a file **visible to `check-eol.py`** — it is the
difference between damage that is detected on the next build and damage that is
detected never. `*.toml` being undeclared is exactly why
`strip-workspace-sections.py` could corrupt every sub-crate manifest in silence.
Declaration does not prevent; it makes prevention checkable.

### The fix is a gate, not 79 edits

Adding `newline=""` at 79 sites is correct at each site and protects nothing at
site 80. The defect is not that 79 authors were careless — it is that **the most
obvious API in the language is wrong by default on this platform**, so the fix
has to be something that keeps being true. Planned, in this order:

1. `scripts/check-text-mode-writes.py` — refuses any text-mode write under
   `scripts/` that lacks an explicit `newline=`. The census above is already the
   analysis; it only needs a verdict, a discovery floor and a `--self-test`.
2. The 79 edits, to make that gate green.
3. **Delete** `strip-workspace-sections.py` rather than repair it. It is orphaned
   and one-shot; fixing a script nobody runs adds a maintained file for no
   benefit, and leaving it fixed-but-unrun leaves a loaded gun with the safety on.

The gate is deliberately **blanket** rather than scoped to sites that write
tracked files, for three reasons: the destination usually cannot be resolved
statically; a temp-dir write today becomes a repo write the moment someone reuses
the helper; and a CRLF *fixture* makes a self-test behave differently on Windows
from Linux, which is a real portability defect even when no tracked file is
touched. `newline=""` costs nothing anywhere, so there is no scope worth arguing
about.

### All three steps are done — 2026-09-04

`scripts/check-text-mode-writes.py` exists, 87 sites across 29 files carry an
explicit `newline=`, and `strip-workspace-sections.py` is deleted. Corrections
to the numbers recorded above, since they were the input to the plan and two of
them were wrong:

| | recorded above | actual |
|---|---:|---:|
| sites needing an edit | 79 | **87** |
| `write_text` | 40 | 38 |
| `open()` | 39 | 40 |
| `os.fdopen()` | — | **8** |
| `NamedTemporaryFile(mode=…)` | — | **1** |
| mode is computed, so undecidable | 41 | **0** |

The `actual` column is the gate's own count, taken by running its `analyse`
over the *pre-edit* blobs (`git show 2785dd1c4:<path>`) rather than over the
diff, and it sums: 40 + 38 + 8 + 1 = 87. That distinction is worth writing
down, because counting the diff is what produced the two wrong numbers this
paragraph replaces. `git show --stat` reports 89 changed lines, and two of
those are continuation lines from calls that had to be split across two lines
to fit the new keyword — so the line count overstates the site count by two,
and the commit message of `37ebb7a49` says "89 sites" where it should say 87.
A changed line is not a call site, and a gate that can count call sites should
be asked rather than a diff.

Two errors, in opposite directions. The census missed `os.fdopen` and
`tempfile` entirely — nine sites, all real, all in the same class. And the "41
calls pass no literal mode" figure was a miscount: those 41 are `open(p)` with
**no mode argument at all**, whose default is `"r"`. They are reads, not
undecidable writes. The tree has *zero* statically-unreadable modes, so the rule
the plan said the gate would need most turns out to cost nothing today — it is
in the gate anyway, because a rule that is free now and correct later is worth
having, and because a gate that shrugs at what it cannot decide reports no
findings, which reads like a clean tree.

**Reads are deliberately not graded.** 107 text-mode reads pass no `newline=`
and nearly all are correct: reading in text mode *normalises* CRLF to `\n`,
which is what a reader wants. It is only a defect in a read-modify-write round
trip, and that is caught at the write end, which is graded. Adding ~107 findings
that are each individually fine to buy no new coverage is how a gate becomes one
nobody believes.

### The blanket scope earned itself on the first run — 2026-09-04

The third justification for the blanket scope was speculative when it was
written: "a CRLF *fixture* makes a self-test behave differently on Windows from
Linux". The sweep found an instance of exactly that, in
`scripts/test-ctest-fixtures.py:137`, in a test written *for this bug family*:

```python
# What git does: rewrite the same bytes, leaving a fresh mtime.
ps1.write_text(ps1.read_text(encoding="utf-8"), encoding="utf-8")
```

The fixture is created LF (`write_bytes(b"# flags\n")`, :101). In text mode the
read folds CRLF to `\n` and the write turns every `\n` back into CRLF, so on
Windows that line did not rewrite the same bytes — it converted the fixture to
CRLF. **The test passed anyway**, because `compute_sysroot` hashes text inputs
with CRLF folded to LF (lane B's 2026-08-16 fix) and so could not see the
difference.

Which means one line of source was asserting two different claims: *"a
CRLF-ified file is not drift"* on Windows, *"a byte-identical file is not drift"*
on Linux. Only the second is the claim its docstring makes, and only the second
is the bug it was written for. Fixed to `write_bytes(read_bytes())` — which is
what the comment above it always said it did, and which no longer depends on the
CRLF-folding rule staying as it is.

This is the seventh sighting of the shape this tree keeps finding, in its
subtlest form yet: not a gate that discovered nothing, but a gate that
discovered *something else* and reported it in the same words.

**If the residue is never addressed:** it is presently harmless — `rustc`
accepts CRLF — so nothing is red. The cost is that the tree carries two
line-ending conventions with no rule stating which is intended, and the
writer that produced them is still running.
