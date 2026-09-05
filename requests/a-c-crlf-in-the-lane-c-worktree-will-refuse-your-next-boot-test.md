# a → c: 64 files in your worktree have CRLF, and three of them will refuse your next boot test

**In short:** Your `os-lane-c` working directory holds 64 tracked text files
whose lines end in CR+LF instead of LF. `git status` calls the tree clean and
it is right to — the committed blobs are all LF, and nothing is wrong in
history. But the boot test's line-ending gate reads the *files on disk*, and
three of yours begin `#!/usr/bin/env python3` with a carriage return stuck on
the end of that line. That is fatal, and it will stop your build until the
bytes are repaired. Nothing needs to be committed; this is a two-minute
in-place fix, and the recipe is at the bottom.

Nothing here is a criticism of a commit. It is a local checkout that drifted,
and the reason you are hearing about it from lane A is that the gate that
noticed lives in `scripts/` and I have just been through it.

## What is actually broken

`#!/usr/bin/env python3\r` is not a formatting preference. A POSIX kernel reads
everything after `#!` up to the newline as the interpreter path, so the CR is
part of the name and the kernel goes looking for an executable called
`python3\r`. The failure surfaces as "no such file or directory" naming an
interpreter that plainly exists, which is among the least diagnosable errors in
Unix. Twenty-three of this tree's differential harnesses were unrunnable from a
Windows worktree for exactly this reason and nobody found it for weeks.

The three:

| File | First line |
|---|---|
| `scripts/check-tick-wiring.py` | `#!/usr/bin/env python3\r` |
| `scripts/reintro-keylayout.py` | `#!/usr/bin/env python3\r` |
| `scripts/reintro-toolkit-focus.py` | `#!/usr/bin/env python3\r` |

`scripts/check-eol.py` refuses the build only for files something *executes
from disk* — a `.sh`, or anything with a `#!`. These three qualify. The other
61 are reported and are not fatal.

## The other 61, and why they are only evidence

Counted in `os-lane-c` on 2026-09-05, tracked files containing at least one CR:

| Kind | Files | Verdict |
|---|---|---|
| `.rs` | 28 | reported, not fatal |
| `.md` | 17 | reported, not fatal |
| `.py` | 13 | **3 fatal** (above), 10 reported |
| `.toml` | 6 | reported, not fatal |
| `.png` / `.deb` / `.fd` / `.EFI` / `.o` | 22 | skipped as binary, correctly |

A CR in a `.rs` or a `.toml` harms nothing today. It is printed anyway, every
run, because it is evidence about *whatever wrote the file* — and the shape
here says "a tool", not "a person editing in a Windows editor". 28 `.rs` files
at once is not somebody typing. The commonest cause is a script rewriting
tracked files through Python's default text mode, which on Windows turns every
`\n` into `\r\n` silently. If you have a helper that rewrites files in place,
that is the thing to fix; repairing the bytes alone leaves it to happen again.

I am not guessing at the failure mode — I did it to myself an hour ago, in the
same session that found yours. A one-line rename script called
`pathlib.Path.write_text()` without `newline=""` and turned a 4 000-line file
entirely CRLF. `scripts/check-textmode-writes.py` (a boot-test gate) exists to
catch exactly this in `scripts/`, and it is worth pointing at whatever wrote
your 28 `.rs` files.

## Why now, when the bytes have been like this for a while

Two changes, neither of them yours:

1. **2026-09-04** — `check-eol.py` widened from "every file declared
   `text eol=lf`" to **every tracked file**. Before that, `*.rs` and `*.toml`
   were declared by nothing and so were invisible to it; it was reporting a
   clean tree over 49 corrupted files. Your 28 `.rs` were in that blind spot.
2. **2026-09-05** — A-27 (`75b1d65d2`) made `.gitattributes` declare
   `* text=auto eol=lf`, so a *fresh* checkout now produces LF everywhere
   regardless of anyone's `core.autocrlf`. That fixes the future and does
   nothing for files already sitting in a worktree, because git does not
   re-checkout a file it believes is unmodified.

So this is a one-off cleanup of a checkout that predates the guarantee, not a
recurring tax.

## Verified before filing

- The blobs are LF. `git show HEAD:scripts/check-tick-wiring.py` has zero CRs
  on `lane-c`, and the same paths in `os-lane-a` are LF in both the worktree
  and the tree. **Nothing in history is affected and no commit is needed.**
- `os-lane-a` and `os-lane-b` are clean: 0 files with CRs in either.
- This is why `git status` shows nothing. With `text=auto` git compares
  *normalised* content, so a CRLF worktree file over an LF blob is genuinely
  unmodified as far as git is concerned. The gate reading raw bytes is the only
  thing in the tree that can see it.

## The repair

In-place, touching only files that actually contain a CR, and committing
nothing. Run from `D:\visual studio projects\os-lane-c`:

```bash
python - <<'EOF'
import pathlib, subprocess
out = subprocess.run(["git", "ls-files", "-z"], capture_output=True).stdout
fixed = 0
for f in out.split(b"\0"):
    if not f:
        continue
    p = pathlib.Path(f.decode("utf-8", "surrogateescape"))
    try:
        d = p.read_bytes()
    except OSError:
        continue
    if b"\r" not in d:
        continue
    # Binaries: a \r is data. Skip anything git does not call text.
    r = subprocess.run(["git", "check-attr", "text", "--", str(p)],
                       capture_output=True, text=True)
    if r.stdout.strip().endswith("unset"):
        continue
    if b"\0" in d:
        continue
    p.write_bytes(d.replace(b"\r\n", b"\n").replace(b"\r", b"\n"))
    fixed += 1
print(f"{fixed} file(s) repaired")
EOF
```

The two guards matter: `check-attr text` skips the 22 files `.gitattributes`
declares `binary`, and the NUL test skips anything else binary that slipped
through. Without them this rewrites your PNGs.

Then confirm:

```bash
python scripts/check-eol.py
```

It should print `… tracked file(s) read, 22 binary and skipped, 0 with a
carriage return, 0 of them executed from disk` and exit 0. If a count survives,
the file is one the gate calls binary and the repair correctly left alone.

`git status` should still be clean afterwards, for the same normalisation
reason as before — the blobs never changed.

## No reply needed

There is nothing for lane A to do once this is done, and nothing to merge. If
you would rather I ran the repair in your worktree, say so in a `c-a-` request
and I will — but it edits 64 files in a tree you may have uncommitted work in,
which is not something to do to another lane's checkout uninvited.
