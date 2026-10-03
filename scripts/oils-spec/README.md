# oils-spec — Oils' spec tests, runnable on SlateOS

design-decisions.md §1043 makes genuine Oils the default shell once its spec
tests have run on SlateOS. Upstream's harness for them (`test/sh_spec.py`) and
the helper programs its cases call (`spec/bin/*.py`) are Python 2; SlateOS's
only Python is CPython 3.12. This directory is the port, the proof that the
port judges every case as upstream's does, and the means of running it there.

| file | what it is |
|---|---|
| `sh_spec.py` | upstream's harness and the parts of `test/spec_lib.py` it uses, ported to Python 3 (Apache 2.0, `LICENSE.txt`); the module docstring lists what changed and why |
| `bin/*.py` | the five Python helpers, ported so their output is byte-for-byte Python 2's -- `argv.py` alone is called by 772 cases, which compare against Python 2's list repr |
| `validate.sh` | runs upstream's harness (Python 2) and the port (Python 3) over every spec file against one native Oils build and requires identical results tables |
| `run_all.py` | the driver for SlateOS: every spec file, in one Python process, each cell compared with the Linux results |
| `bundle.sh` | builds `build/oils-spec/usr/share/oils-spec/` for the image: the two scripts above, upstream's spec files and test data, the helpers, and the Linux results as `expected/` |

Everything upstream is pinned: the release tarball by SHA-256, the source (for
the spec files, which the release tarball does not carry) by the commit at the
head of upstream's `release/0.38.0` branch and the archive's SHA-256.

## Validating the port

From WSL:

```bash
bash scripts/oils-spec/validate.sh              # every spec file
bash scripts/oils-spec/validate.sh smoke alias  # just these
```

It needs a Python 2.7 for upstream's harness, which conda makes without root
(`~/miniconda3/bin/conda create -p /tmp/py27 -c conda-forge python=2.7`; or set
`PY2`). It builds Oils natively once, runs each harness with the same `PATH`,
`$TMP`, environment and 20-second timeout, and prints
`SPEC_FILES=… CASES=… IDENTICAL=… DIFFERENT=…`; any difference is listed, and
is a defect in the port or a helper.

## Running on SlateOS

`bash scripts/oils-spec/bundle.sh` (after `validate.sh`, so the Linux results
travel with it), then, on the machine:

```sh
python3 /usr/share/oils-spec/run_all.py
```

One line per spec file, then `OILS_SPEC_SUMMARY files=… cases=… failed=…
differ_from_linux=…`. A cell that differs from Linux is a case that behaves
differently here: a bug in SlateOS's C library or kernel, a program a case
runs, or a platform difference to explain.
