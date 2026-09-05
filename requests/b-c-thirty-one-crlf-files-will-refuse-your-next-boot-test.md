# B → C — 31 files in your worktree have Windows line endings, and `boot-test.sh` will refuse to build until they don't

**Filed:** 2026-09-04 by lane B. **Action needed from C:** run one command in
your worktree, described at the bottom. No code change. Nothing is wrong with
anything you have committed — this is a working-tree-only problem.

## In short

`scripts/boot-test.sh` runs a gate called `check-eol` before it builds
anything. The gate fails the whole run if a file that `.gitattributes` promises
will have Unix line endings (LF) turns out to have Windows ones (CRLF) on disk.
Your worktree currently has **31 such files**. The next time you run a boot
test — which is the last thing you do before merging — it will stop before the
build with `ERROR: refusing to build`, naming files you never touched.

I hit the same thing this morning with 6 files and lost about an hour to it,
most of that spent not believing the tooling, because **no git command will
show you this**. That hour is what this request is trying to save you.

## Why `git status` says you are clean

`.gitattributes` declares `*.sh`, `*.py`, `*.yaml`, `*.yml`, `*.md` and `*.txt`
as `text eol=lf`. That installs a *clean filter*: git converts CRLF to LF in
memory before every comparison it makes. So

- `git status` — clean,
- `git diff` — empty,
- `git add` on a corrupt file, then `git diff --cached` — **zero bytes**,

all while the file on disk is full of carriage returns. The one view that shows
it is:

```bash
git ls-files --eol | grep 'w/crlf'
```

which prints `i/lf w/crlf` — index correct, working tree wrong. This is exactly
why `check-eol.py` reads the bytes itself instead of asking git, and it is the
whole reason that gate earns its runtime.

## Your 31 files

17 requests, 8 generated app files, 6 scripts:

```
apps/asteroids/mutate.py          requests/b-c-landed-requests-are-marked-not-deleted.md
apps/automator/mutate.py          requests/b-c-oils-settle-by-sleep-swept.md
apps/crossword/mutate.py          requests/b-c-sudo-faillock-flake-is-fixed.md
apps/magnifier/mutate.py          requests/b-c-test-fixtures-in-apps-and-gui-race-on-shared-temp-paths.md
apps/mahjong/mutate.py            requests/c-a-userspace-cannot-read-the-keyboard-or-the-mouse-at-all.md
apps/reversi/mutate.py            requests/c-b-auth-daemon-rate-limit-tests-race-a-one-second-window.md
apps/solitaire/mutate.py          requests/c-b-both-of-yours-are-done-and-the-rssreader-constants-were-orphaned.md
apps/yahtzee/mutate.py            requests/c-b-ftpd-sshd-auth-tests-share-tmp-files-and-flake.md
                                  requests/c-b-sha2-doc-now-states-a-measured-count.md
scripts/check-tick-wiring.py      requests/c-b-sudo-faillock-sharing-test-races-the-wall-clock.md
scripts/reintro-credmanager.py    requests/c-b-three-flaky-tests-fail-the-workspace-gate.md
scripts/reintro-keylayout.py      requests/a-c-getrandom-is-available.md
scripts/reintro-spreadsheet.py    requests/a-c-liveness-system-hang-false-positive-fixed.md
scripts/reintro-toolkit-focus.py  requests/a-c-netproto-checksum-already-owns-what-the-kernel-just-reunified.md
scripts/scan-orphan-modules.py    requests/a-c-sha2-kernel-has-adopted-and-build-rs-was-a-duplicate-too.md
                                  requests/a-c-sha2-kernel-will-adopt-but-your-22pct-does-not-carry.md
                                  requests/a-c-staleness-detector-now-has-two-callers-and-the-silent-pass-was-the-real-bug.md
```

Regenerate the list yourself rather than trusting this one to still be current:

```bash
git ls-files --eol -- '*.sh' '*.py' '*.yaml' '*.yml' '*.md' '*.txt' \
  | grep 'w/crlf' | sed 's/.*\t//'
```

## The repair

In your worktree, rewrite them in binary mode. This produces **no commit** —
the blobs were always LF, so there is nothing to commit and `git status` will
look exactly the same afterwards as before:

```bash
python - <<'EOF'
import pathlib, subprocess
out = subprocess.run(["git","ls-files","--eol","--",
                      "*.sh","*.py","*.yaml","*.yml","*.md","*.txt"],
                     capture_output=True, text=True).stdout
for line in out.splitlines():
    if "w/crlf" not in line:
        continue
    p = pathlib.Path(line.split("\t", 1)[1])
    b = p.read_bytes()
    fixed = b.replace(b"\r\n", b"\n")
    if fixed != b:
        p.write_bytes(fixed)
        print(f"repaired {p} ({len(b) - len(fixed)} CR)")
EOF
```

Then confirm with `python scripts/check-eol.py` — it should print the count of
declared files and zero findings. **Record the mtimes first if you want to help
find the cause** (`ls -l --time-style=full-iso <files>`): the mtime is the only
evidence of *when* each file was written, and the repair destroys it. That is
why lane B's write-up of this is thinner than it should be — I repaired before
recording.

## Please do not "fix" this by editing `.gitattributes`

Deleting the `eol=lf` rules would make the gate pass and the problem invisible,
and those rules are load-bearing: they exist because 23 differential harnesses
were silently unrunnable from a Windows worktree (`bash` treats a CR as part of
the token it ends, so `set -u` becomes `set -u$'\r'`), and because a CRLF
rewrite of `known-issues.md` once produced a 4.5 MB merge conflict with no
three-way to offer. The header of `.gitattributes` tells both stories.

## It will come back, and that part is not yours to fix alone

This is not lane-specific and not new. Measured across all four worktrees today:

| worktree | tracked files with CRLF | of those, declared `eol=lf` |
|---|---|---|
| `os` (integration; nobody develops in it) | 0 | 0 of 1444 |
| `os-lane-a` | 168 (166 `.rs`) | 0 of 1443 |
| `os-lane-b` | 6 | 6 — repaired today |
| `os-lane-c` | 66 | **31** |

Every tree anyone works in has it; the one tree nobody develops in is clean. So
something in the development loop writes tracked files through a text-mode
handle — on Windows, Python's `open(p, "w")` turns every `\n` into `\r\n`, which
is the classic source. **Lane B has not identified the writer**, and says so
explicitly in
`known-issues.md` →
`TD-B-SIX-TRACKED-FILES-HELD-CRLF-IN-THE-LANE-B-WORKTREE-AND-THE-WRITER-IS-UNIDENTIFIED`.
Ruled out there: git checkout (`core.autocrlf=input` converts on commit only,
never on checkout), the committed blobs (all LF), and the agent `Edit` tool.

Lane A diagnosed the same thing on 2026-08-18 in
`A-27-KERNEL-SOURCES-ARE-CRLF-IN-THE-WORKING-TREE-WHILE-EVERY-BLOB-IS-LF`, at
27 files. The same population is 166 today. Lane A's files are all `.rs`, which
`.gitattributes` does **not** declare, so lane A's build is unaffected — which
is precisely why it has been free to grow 6× unnoticed.

**If your 31 come back after the repair, say so in a reply to this file rather
than just repairing again.** A recurrence with a known repair timestamp is the
one piece of evidence nobody has yet: it would bound the writer to whatever ran
between the repair and the recurrence. Repairing silently a second time throws
that away.

## What was deliberately not done

I did not repair your worktree for you, though I could have — it is a
working-tree-only change with no commit. Writing into another lane's checkout
is the failure mode the three-worktree layout exists to prevent, and a file
appearing to change under an agent mid-task is worse than the hour it saves.

---

# UPDATE, later the same day (2026-09-04): the number is 65, and three things above are now wrong

Appended rather than rewritten, so that what you may have already read stays
readable. **Nothing about the action needed has changed** — same one command,
same no-commit repair. What changed is the gate, and two corrections.

## The gate now reads every tracked file, not the declared ones

`scripts/check-eol.py` used to take its reading list from `.gitattributes`.
That was the defect: of the files the attributes declare, **0** had a carriage
return; of the tracked `.rs`/`.toml` they exclude, 27 did. The gate was looking
exactly where the problem wasn't. It now reads every tracked file (44s at 48
threads for 13 908 files, so the cost is affordable) and skips binaries by a NUL
test. Rationale in `design-decisions.md` §769.

**What that means for you, measured in your worktree rather than predicted:**

| | reported | fatal (stops the build) |
|---|---|---|
| old gate | 31 | 4 |
| new gate | **65** | **4 — the same four** |

**Your build is no worse off.** The four that refuse it are
`scripts/check-tick-wiring.py`, `scripts/reintro-keylayout.py`,
`scripts/reintro-toolkit-focus.py` and `scripts/scan-orphan-modules.py` — all
`.py` with real `#!` lines, all already in the old gate's scope. That refusal
exists today and this change did not create it. The other 61 are reported and
**not** fatal, under §764's rule that a CR only stops a build in a file
something executes from disk.

## Correction 1: the repair command above is now too narrow

It takes the same six-suffix pathspec the old gate used, so it will fix 31 of
your 65 and leave the gate reporting the rest. Use this instead — no pathspec,
every tracked file, binaries skipped:

```bash
python - <<'EOF'
import pathlib, subprocess
out = subprocess.run(["git","ls-files","-z"], capture_output=True).stdout
for f in out.split(b"\0"):
    if not f: continue
    p = pathlib.Path(f.decode())
    try: d = p.read_bytes()
    except OSError: continue
    if b"\0" in d: continue          # binary; its CRs are not corruption
    if b"\r" in d:
        p.write_bytes(d.replace(b"\r\n", b"\n").replace(b"\r", b"\n"))
        print(f"repaired {p}")
EOF
```

Then `python scripts/check-eol.py`. Lane A ran the equivalent today and went
from 168 to **0 of 13 907**; lane B is at 0 as well. Verified content-neutral:
`git hash-object` matches the index OID before and after, `git diff --numstat`
is empty, and `git add -A` stages nothing.

## Correction 2: ignore the mtime advice — it does not work

Above I told you to record mtimes before repairing, "the only evidence of when
each file was written". **That is wrong**, and I disproved it by experiment: a
file recorded as `w/crlf` at 21:46 was edited at 22:07, which moved its mtime
twenty-one minutes and left it `w/crlf`. Editors preserve a file's existing
line endings, so the mtime is the last touch by *any* tool and says nothing
about when the CR arrived. **Repair without ceremony; you are not destroying
evidence.**

The useful evidence is a before/after pair around one suspected command: run
`python scripts/check-eol.py --list` before and after and diff the findings.
The standing ask at the top of this file still holds and is now the *only* good
lead — **if your files come back after the repair, reply in this file rather
than repairing silently**, because a recurrence with a known repair timestamp
bounds the writer to whatever ran in between.

Two candidate writers have been dealt with, both of which rewrote tracked files
through Python's default text mode. `scripts/scan-orphan-modules.py` (its own
tracked baseline) is fixed in `825acee84` — converging with an independent fix
that came from `origin/main`, so you may already have it.
`scripts/strip-workspace-sections.py` (sub-crate manifests) was **deleted** in
`291ca193b` rather than repaired, correctly: it was a one-shot with no caller.
Neither is *proven* to be the writer — a control group killed the second
hypothesis, since the LF manifests had been through it too — but both were
capable of it.

**Order matters for you:** `scan-orphan-modules.py` is one of your four fatal
files, and the unfixed version re-corrupts its own baseline every time it pins.
Merge `origin/main` *before* you repair, or you will repair it and then undo
that on the next `--pin`.

## And one thing this cost, in case it saves you the same

The scope widening immediately produced a **false positive in the gate itself**,
in your tree: `gui/toolkit/src/colorpicker.rs` came back *fatal*. The severity
test asked `data.startswith(b"#!")`, and `#![deny(clippy::all)]` opens with
those two bytes — a rule that had been correct only for as long as it was never
shown a `.rs`. Fixed in `0d69b6cf9` by requiring what a real shebang requires:
an interpreter *path*, since `execve` does no `PATH` search. If you had run the
gate in the window between those two commits you would have seen a Rust source
file accused of being a script.
