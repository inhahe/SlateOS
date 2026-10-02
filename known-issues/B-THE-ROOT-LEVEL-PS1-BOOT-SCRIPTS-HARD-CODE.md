## B-THE-ROOT-LEVEL-PS1-BOOT-SCRIPTS-HARD-CODE-`os`-SO-WINDOWS-BOOT-TESTS-THE-WRONG-TREE

**Status:** OPEN — **Lane A's zone** (the boot test). Found 2026-08-16 by Lane
B, which has deliberately changed none of these files. Filed as
`requests/b-a-ps1-boot-scripts-hard-code-the-os-worktree.md`.

Six PowerShell scripts at the repo root name `D:\visual studio projects\os`
outright: `boot-test.ps1` and `boot-test-2cpu.ps1` (`$cwd = "…\os"`),
`boot-test-stdio.ps1` and `run-boot-test.ps1` (`Set-Location "…\os"` plus
`$diskImg`/`$ext4Img`/`$swapImg`), `quick-boot-test.ps1` (`$serial_log`), and
`build-init.ps1` (`$initDir = "…\os\userspace\init"`).

**Impact.** Run from a lane worktree, they boot **main's** image and report
PASSED — a false green in which every property the lane believes it verified is
a statement about a different checkout. This is the same family as the two
entries above, but it is the worst instance of it, for two reasons:

1. **It is documented.** `README.md:65-68` offers `powershell ./boot-test.ps1`
   as *the* Windows equivalent of `./scripts/boot-test.sh`. An agent that
   follows the README does the wrong thing by doing what it was told.
2. **`build-init.ps1` writes**, to `os\userspace\init` — a cross-lane write,
   and into Lane B's zone specifically.

**Mitigating.** `scripts/boot-test.sh` derives its own paths correctly and is
what `CLAUDE.md`, the roadmap and every automated run actually use, so no
current CI path is affected.

**Proper fix.** Derive the root from the script's own location, as
`scripts/lib/worktree.sh` now does for shell:
`$root = Split-Path -Parent $MyInvocation.MyCommand.Path` (twice, for a script
under `scripts/`), plus the same bail-out check that the derived root really is
a SlateOS checkout. **Or delete them** — only `boot-test.ps1` is referenced
anywhere in the tree; the other five have no references at all and look
superseded by `scripts/boot-test.sh`. An unmaintained script that boots the
wrong image is a trap whether or not anyone runs it today.
