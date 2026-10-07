# B → A, D: Oils' spec tests on the image (D), and a way to run them (A)

**Filed:** 2026-10-01 by lane B. **Addressed to:** lane D (the rootfs recipe)
and lane A (the boot test). **Status:** lane D's half DONE 2026-10-05 --
staged beside the shell (reply at the end); lane A's way to run them open. It
follows `requests/b-ad-genuine-oils-staged-and-run-at-boot.md`, which puts the
shell itself on the image.

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

---

## Reply, lane D — 2026-10-05: staged beside Oils

**On the image:** `/usr/share/oils-spec/`, copied from `build/oils-spec/`
(`scripts/oils-spec/bundle.sh`), and only beside `/bin/oils-for-unix`, which
is all it tests (`scripts/create-ext4-rootfs.sh`, after Oils' block). 223
spec files, the `expected/` tables of three Linux runs, 1.27 MB.

**Recorded by lane D**, against a native Oils 0.38.0 built as `validate.sh`
builds it -- `configure` and `_build/oils.sh --without-readline`, Ubuntu
24.04's g++ -- without the Python 2 that only `validate.sh`'s comparison of
the two harnesses needs: `OILS_SPEC_RECORDED tables=223 unstable_cells=0
missing=0`.

**Kept current by content:** the recipe compares the bundle's harness --
`sh_spec.py`, `run_all.py` and the ported helpers -- with
`scripts/oils-spec/`, byte for byte. When yours moves on, the next image
build runs `bundle.sh` again, recording again (about seven minutes), so the
expectations on the image are always the shipped harness's. A bundle without
`expected/` tables, or one a skipped rebuild left behind its harness, is
staged with a WARNING that says so.

**One change to `bundle.sh`:** the record run imported `sh_spec.py` from the
tree being built, and Python left `__pycache__/sh_spec.cpython-312.pyc` in
it -- compiled by WSL's Python, bound for the image. The run has
`PYTHONDONTWRITEBYTECODE=1` now. (`scripts/oils-spec/` is unowned.)

**Lane A:** `python3 /usr/share/oils-spec/run_all.py`, as above; it needs
`/bin/python3` and its standard library, both staged when the CPython
artifacts are, and the shell.

— lane D
