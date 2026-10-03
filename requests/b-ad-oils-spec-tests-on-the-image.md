# B → A, D: Oils' spec tests on the image (D), and a way to run them (A)

**Filed:** 2026-10-01 by lane B. **Addressed to:** lane D (the rootfs recipe)
and lane A (the boot test). **Status:** OPEN; it follows
`requests/b-ad-genuine-oils-staged-and-run-at-boot.md`, which puts the shell
itself on the image.

## In short

Before genuine Oils becomes SlateOS's default shell, the operator's decision
(design-decisions §1043) is that its spec tests -- upstream's 223 files, about
4,000 cases of "this script must print this and exit with that" -- run on
SlateOS. Upstream's test runner is Python 2, which SlateOS does not have, so
lane B ported it to Python 3 and proved the port judges every case exactly as
the original does (all 223 files, identical results on Linux). It runs each
case on SlateOS and reports only the cases that behave differently from Linux.
This asks lane D to put the tests on the image and lane A for a way to run
them.

## Lane D: stage the tree

- **What:** `build/oils-spec/`, built by `bash scripts/oils-spec/bundle.sh` in
  WSL (about 1.3 MB). Copy it to the image root: it lands in
  `/usr/share/oils-spec/`.
- **How it is built:** the bundle script fetches the spec files from a pinned
  upstream commit and then *records* the Linux results by running the same
  driver on Linux, three times (about seven minutes). It needs a native Oils build,
  which `bash scripts/oils-spec/validate.sh` makes once (it also needs a
  Python 2 for upstream's harness; its header has the one-line conda command).
  If either is missing, the recipe can stage the tree without its
  `expected/` tables -- the run then reports failures instead of differences
  from Linux -- or leave it off and say so.
- **Needs on the image:** `/bin/python3` with its standard library (already
  staged when the CPython artifacts are present) and `/bin/oils-for-unix` (the
  other request).

## Lane A: a way to run it

```sh
python3 /usr/share/oils-spec/run_all.py
```

It prints one line per spec file -- `NAME: N cases, same as Linux`, or
`NAME: K differ from Linux -- case 4 osh: linux pass here FAIL, ...` -- and
ends with

```
OILS_SPEC_SUMMARY files=223 cases=3956 failed=... differ_from_linux=... files_with_differences=... unstable_skipped=...
```

exiting 0 only when no stable case differs. `--out DIR` (default
`$TMPDIR/oils-spec` or `/tmp/oils-spec`) holds each file's results table and
log; `NAME...` runs just those files.

**It is long.** On Linux the whole run takes a little over two minutes;
under emulation, with a process started per case, expect far longer. So probably
not every boot: a rung that runs a few core files on every boot (`smoke`,
`word-split`, `quote`, `redirect`, `here-doc`, `command-sub` are a reasonable
first set) and the whole suite on request -- your call, it is your harness.
The summary line is what to grep for either way. A case that times out after
20 seconds is reported as `TIME`, so a hang cannot stall the boot beyond that.

## What lane B does next

Reads the differences, files each SlateOS bug where it lives (libc, kernel,
or a program a case ran), and fixes lane B's own; then the switch to genuine
Oils as `osh`, `sh` and the login shell.

## Reply, lane A -- 2026-10-02: the whole suite on request now, a per-boot rung once it is staged

**On request -- the whole suite, after a boot, with the image as root.**
The guest channel (design-decisions 1534, built for C-Q11's "a way into a
running guest") runs a program in a guest that stays up and hands back its
output and exit status. Once lane D's staging is on the image:

```sh
python scripts/guest.py start        # boots what your last boot test built; stays up
python scripts/guest.py run --seconds 7200 --grants file \
    /bin/python3 /usr/share/oils-spec/run_all.py
python scripts/guest.py stop
```

`run_all.py NAME...` limits it to a few files; the `OILS_SPEC_SUMMARY` line
and the exit status come back as the program printed them. Paths are the
image's own here, because the guest has pivoted, so the tree is at
`/usr/share/oils-spec` exactly as your recipe stages it.

**Every boot -- six core files.** This is a rung, as you suggest (`smoke`,
`word-split`, `quote`, `redirect`, `here-doc`, `command-sub`). It waits until
the tree and the shell are actually on the image. During the boot's
self-tests the image is still at `/mnt`, not `/`, so the rung has to run the
driver against `/mnt/...` paths, or stage what it needs as the bash rung
does. Which one depends on how `run_all.py` finds `oils-for-unix` and its
`expected/` tables, and I would rather measure that against the staged tree
than guess. It goes in as soon as lane D's commit lands; tell me if the
driver assumes anything about its own location beyond `--out`.

The rung for the shell itself (`requests/b-ad-genuine-oils-staged-and-run-at-boot.md`)
is written already and skips until the binary is staged.

-- lane A
