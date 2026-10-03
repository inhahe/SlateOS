## B-THE-FIXTURE-STAMP-HASHED-WORKTREE-BYTES-SO-IT-DID-NOT-SURVIVE-A-CHECKOUT (lane B, 2026-08-16)

**Status: ✅ FIXED 2026-08-16** — stamp format v2, `scripts/ctest-fixtures.py`
(`sha256_text`, `_inputs`, `_stamp_version`) plus all nine `services/ctest-*.stamp`
re-written. Recorded rather than fixed silently because it is the *third* time
this file has caught a check that reports a healthy tree as broken, and the
first time the check's own foundation was the thing at fault.

**In short:** nine compiled test programs are each checked in beside a `.stamp`
file holding a fingerprint of the source that built them. The fingerprint was
taken of the bytes sitting in one developer's folder, and on Windows those
bytes can differ from another folder's copy of *the identical committed file*,
because Windows and Linux end lines differently. So all nine programs reported
"stale, rebuild me" in a checkout where nothing whatsoever was wrong — and
because the disk-image build treats that report as fatal, the image could not
be built anywhere except on the one machine that wrote the stamps.

### How it presented

Immediately after merging `lane-b` into `main` — a **fast-forward**, so the
tree was byte-identical to the one where all nine had just passed:

```
$ python scripts/ctest-fixtures.py check      # in the `os` integration worktree
[ctest] ERROR ctest-ctty: STALE - the ELF does not match its inputs.
[ctest]          input build.py: recorded 765ac1c53ad802b0... but on disk 77d760d6ca55933f...
   … all nine, on `build.py` …
```

`main.c` did *not* drift; only `build.py` did. That asymmetry is the tell —
a genuine source change would not confine itself to one of two tracked files
in all nine directories at once.

### Root cause

`git config core.autocrlf` is **`input`**: normalise CRLF→LF on commit, do not
convert on checkout. A clean checkout therefore yields LF. But any tool that
rewrites a tracked text file in Python/text mode on Windows writes CRLF back,
and `git status` still calls it clean, because `input` normalises again when
reading the worktree. Two worktrees of one commit legitimately hold
byte-different `build.py` files:

```
build.py  raw=77d760d6ca55933f  norm=77d760d6ca55933f  crlf=  0   os
build.py  raw=765ac1c53ad802b0  norm=77d760d6ca55933f  crlf= 99   os-lane-b
```

`sha256()` hashed the raw bytes, so the stamp recorded a property of *the
worktree that wrote it* rather than of the commit. This is the same class of
error as the mtime gate the stamp was built to replace — the module docstring
claims the stamp "survives a checkout because it does not depend on the
filesystem's opinion of time", and it turned out to depend on the filesystem's
opinion of line endings instead.

### Why it was dangerous rather than annoying

`create-ext4-rootfs.sh` **exits 1** on a stamp mismatch, deliberately. So a
false STALE does not warn, it blocks — and its only escape hatch,
`ALLOW_STALE_FIXTURES=1`, also disables the *real* staleness check. A check
that cries wolf in a place with one shared off-switch trains its users to
throw that switch, which is precisely how
`B-CTEST-FIXTURES-LINKED-A-STALE-LIBC` happened in the first place.

### The fix

Text inputs (`build.py`, `main.c`) are hashed with `\r\n` folded to `\n`;
`libc.a` and the output ELF stay byte-exact. The fold is *sound*, not just
convenient: both text inputs are consumed by tools that treat CRLF and LF
alike (the C preprocessor, the Python tokenizer), so two files differing only
in line endings genuinely produce the same ELF — whereas one byte of `libc.a`
does not.

`STAMP_VERSION` went 1 → 2, and a v1 stamp is now reported as a **format**
mismatch with its own message, explicitly *not* as drift: the two sides were
computed under different rules and were never comparable, so "the ELF does not
match its inputs" would have been an unfounded accusation. There is no v1
compatibility path, on purpose — it would be code that runs once, in the hands
of the person who wrote it.

### The migration was proved content-neutral before re-stamping

Re-stamping is normally the wrong repair (`check`'s own message says so: it
records drift rather than fixing it). Here it is right, and that was
*established* rather than assumed: the pre-edit script, recovered with
`git show HEAD:scripts/ctest-fixtures.py`, was run against the same tree and
reported `ok` for all nine. So under v1 rules nothing had drifted, and the only
thing v2 changes is the rule. Afterwards, the v2 hashes were confirmed to match
the *other* worktree's copies of `build.py` and `main.c` for all nine fixtures
— the property that was missing all along.

### Verified end to end, across the boundary that broke

After the fix the `os` integration worktree still failed — on
`toolchain/sysroot/lib/libc.a` alone (`on disk 4b14549d…`, recorded
`e322989b…`), with the `build.py` line gone. That remainder was real and local:
the sysroot is gitignored, so that worktree simply held an older build. Running
`powershell -File toolchain/build-sysroot.ps1` there (100 s) and re-checking
gave **`ok` for all nine, in the LF worktree, against stamps written in the CRLF
one.** That crossing is the property the v1 stamp never had, and it is now the
acceptance test for any future change to how inputs are hashed.

Worth keeping: after the fix, the surviving failure was the *correct* one. A
check that reports exactly the one thing that is genuinely wrong, and nothing
else, is the whole point of naming drifted inputs individually.

### How widespread the CRLF is, since the next reader will ask

Surveyed across all 13,145 tracked text files in lane B's worktree: **70 carry
CRLF, and every one is a `services/*/build.py`** — the nine `ctest-*` plus ~61
`fastpy-*`. Every committed blob is LF (`git show HEAD:…` on a sample counts
zero), so this is purely a worktree artifact of whatever generated those files
in text mode, not anything in history.

Only the nine `ctest-*` copies were ever hashed. The `fastpy-*` ones are an
input to nothing: `STAGED_GLOBS` fingerprints their **ELFs**, which are binary
and hashed byte-for-byte, correctly. So the exposure was exactly the nine, and
it is closed.

**No `.gitattributes` was added, deliberately.** It would not help: git already
checks these out as LF under `autocrlf=input`; the CRLF arrives *after*
checkout, written by a tool. A config that governs checkout cannot fix a file
rewritten post-checkout, which is precisely why the durable fix belongs in the
hash rule and not in git configuration — and why the fix works for any future
tool that does the same thing without anyone having to notice.
