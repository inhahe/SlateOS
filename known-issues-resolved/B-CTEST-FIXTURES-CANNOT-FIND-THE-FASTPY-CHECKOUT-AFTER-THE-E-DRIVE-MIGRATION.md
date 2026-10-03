## B-CTEST-FIXTURES-CANNOT-FIND-THE-FASTPY-CHECKOUT-AFTER-THE-E-DRIVE-MIGRATION (lane B, 2026-09-07)

**Status: FIXED 2026-09-10, both halves; verified again 2026-09-12.**
`scripts/ctest-fixtures.py` and `scripts/create-ext4-rootfs.sh` try the
pre-migration `D:` location **last** — after `$FASTPY_DIR`, `$PYTHONPATH` and a
real sibling, so a genuine sibling always wins — and **announce** it when they
use it, because a lookup that quietly picks a checkout the caller did not
choose is its own failure. `scripts/boot-test.sh` had the same dead fallback
and is lane A's; handed over in
`requests/b-a-the-fastpy-sibling-lookup-has-been-dead-since-the-e-migration.md`,
which lane A closed the same day.

Verified with no environment set at all, rather than read off the diff:

```text
$ env -u FASTPY_DIR -u PYTHONPATH python -c "...; print(_fastpy_dir())"
[ctest] fastpy: using D:\visual studio projects\fastpy
        (no sibling at E:\visual studio projects\fastpy)
D:\visual studio projects\fastpy
```

**The workaround below is obsolete. Do not set `FASTPY_DIR` by hand.**

**Why this entry was wrong, which is the part worth keeping.** It concluded
"the proper fix is a decision, not a patch" and declined to act, on the
grounds that teaching the search the `D:` location would "encode a migration
that is supposed to be finished". That reasoning does not survive contact with
the facts: the migration *is* finished — for `os`. It was never begun for
fastpy. The global `CLAUDE.md` records fastpy, `Python Agent`, `orchestrator2`
and `backup` as still living under `D:\visual studio projects`, and says so as
current fact rather than as history. So the option was never "hard-code a
stale path"; it was "read the documented one", which is a patch and needs
nobody's standing.

The entry turned a lookup into a governance question and then filed itself
under "no lane may decide this". It cost four days of every lane setting an
environment variable by hand, and cost lane A a boot test on 2026-09-10
(`ModuleNotFoundError: No module named compiler`). **Declining to act is an
act**, and "no lane has standing" deserves the same evidence as any other
claim — here, one line of an instruction file that was already loaded every
session would have refuted it.

*The description below is kept in the tense it was written in.*

**In short:** the script that builds the ring-3 C test fixtures needs a second
repository (fastpy) to do the cross-compile. It looks for that repository *next
to this one*. The 2026-09-06 move put this repository on `E:` and left fastpy on
`D:`, so the search fails and every fixture build stops with an error telling
you to set an environment variable.

**Symptom**, verbatim, from `scripts/ctest-fixtures.py build`:

```
[ctest] ERROR: cannot find a fastpy checkout (needs compiler/__init__.py).
[ctest]        or place the fastpy checkout beside this repo:
[ctest]          E:\visual studio projects\fastpy
```

**Workaround, which works today:**

```sh
FASTPY_DIR="D:/visual studio projects/fastpy" python scripts/ctest-fixtures.py build
```

**Affects all three lanes**, not just the one that found it: any lane that
rebuilds a fixture, or that runs a boot test which rebuilds one, hits it. It is
not a code defect -- the script's own error message names the fix -- but it is a
per-command tax nobody was told about, and the failure arrives in the middle of
a build rather than at setup time.

**Worth noticing about the shape:** the script fails *loudly and with the
remedy*, which is why this is a papercut rather than an incident. Had it instead
silently skipped the fixtures, a boot test would have run against stale ELFs and
reported a pass -- the same class of quiet-stale defect as
`A-A-REBUILT-SERVICE-DOES-NOT-INVALIDATE-THE-KERNEL-THAT-EMBEDS-IT`.

**The proper fix is a decision, not a patch,** which is why this is logged
rather than fixed: either fastpy moves to `E:` beside the repo (fastest, but it
is not this project's tree to move), or the search learns the old `D:` location
(encodes a migration that is supposed to be finished), or `FASTPY_DIR` is set
once in the environment for all lanes (operator's, and outside any lane's
files). Lane B has no standing to pick among those, so it is written down with
the workaround instead.
