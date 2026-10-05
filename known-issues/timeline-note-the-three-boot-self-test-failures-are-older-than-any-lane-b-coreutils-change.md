## Timeline note — the three boot self-test failures are older than any lane-B coreutils change

Recorded 2026-08-22 because the question "did my merge break the boot test?"
comes up every time, and answering it took long enough to be worth writing down.

`bench/boot-history.jsonl` shows `SELFTEST_FAIL` on **every** full-length boot
back through 2026-08-21 14:35 (`2b8b1a536`), across both lane A and lane B,
dirty and clean trees alike. The two `PASS` rows in that stretch are not
counter-examples:

| Row | Serial lines | Wall | What it actually is |
|---|---|---|---|
| `0e4e927de` 17:17 PASS | 7,041 | 88 s | a short run that never reached the Path-Z tests |
| `558d5fc1e` 19:12 PASS | 31,286 | — | the *same* serial log as the `SELFTEST_FAIL` row 33 s earlier, re-scored |

A full-length boot is ~31k–42k serial lines and 380–850 s. Anything much
shorter did not get far enough to fail these tests, so it must not be read as
evidence that they once passed.

Second, independent check that lane B's coreutils work is not implicated: the
three failing tests are `fastpy-minishell` (a fastpy fixture ELF), `real make`
and `make-drives-tcc` (staged `/bin/make`, `/bin/sh`, `/bin/tcc` from the Debian
rootfs plus `build/spike/make-slateos.elf`). None of them loads anything built
from `userspace/coreutils/`, which is not staged on `rootfs.ext4` at all yet.
The artifact sets are disjoint.
