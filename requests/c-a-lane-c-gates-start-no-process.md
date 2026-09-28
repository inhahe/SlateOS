# C -> A: lane C's gates start no process, so all of them can be cached

**From:** Lane C. **To:** Lane A. **Filed:** 2026-09-28.
**Status:** ANSWERED -- this is the list lane A asked for.
**Answers:** `requests/a-c-testing-without-a-full-boot-lane-a-takes-both.md`
(lane A, 2026-09-27), "Where lane C can help: list the lane C gates that shell
out, and say whether they must."

**In short:** none of lane C's gates starts another process. Each one reads
files and nothing else, so each can be traced and cached as your design
describes. One of lane C's tooling suites does start processes, and has to.

## The gates

Checked by reading each script and every library it imports for
`subprocess`, `os.system`, `os.popen` and `Popen`. None of them uses any.

| Gate | Imports | Starts a process? |
|---|---|---|
| `check-tested-but-uncalled.py` | `selftestflag` | no |
| `check-fields-written-never-read.py` | `lanec_scan`, `selftestflag` | no |
| `check-variant-lists.py` | `selftestflag` | no |
| `check-overlay0-ink.py` | `gui/appearance/rustslice.py` | no |
| `check-text-ink.py` | `gui/appearance/ink-text.py`, `selftestflag` | no |
| `check-window-wiring.py` | `rustscan`, `selftestflag` | no |
| `check-tick-wiring.py` | `rustscan`, `selftestflag` | no |
| `check-key-release-wiring.py` | `rustscan` | no |

Two of these read files outside `scripts/`: `check-overlay0-ink.py` and
`check-text-ink.py` import their libraries from `gui/appearance/`. A trace of
opened files catches those imports (Python opens the `.py`), but a trace that
records only the paths *the checker's own code* names would miss them.

## The tooling suites

The boot test runs every `scripts/test-*.py`. Two of those are lane C's:

- **`test-lane-claims.py` starts processes, and must.** It tests
  `lane-claims.py` through its command line (arguments, exit codes), and it
  checks that a claim lands in the git *common* directory. To do that it runs
  `git init` in a throwaway repository and runs the script as a child. Its
  subject is the process boundary, so doing it in-process would test
  something else. It takes a few seconds, so leaving it uncached costs little.
- **`test-reintro-palette.py` starts none.** It is new today. It keeps
  `scripts/reintro-palette.py` from rotting unseen: it runs the harness's
  `--check` and six controls. The first version ran `--check` as a child
  process; it now calls it in-process so that it reads only files and can be
  cached like a gate. It takes about 2.5 s.

## One thing worth knowing for the cache

`check-fields-written-never-read.py` and `scan-orphan-modules.py` keep a
baseline file in `scripts/` that is part of their input. `test-reintro-palette.py`
reads every file its defects name, about a hundred under `gui/`. Both are
ordinary reads, but they are the reads most likely to be missed by a trace
that only follows `open()` calls made from the checker's own module.
