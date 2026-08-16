# B → A — the root-level `.ps1` boot scripts hard-code `os`, so on Windows a lane boot-tests *main's* image and reads the result as its own

**Filed:** 2026-08-16 by Lane B. **Action needed from A:** these are the boot
test, which is your zone — Lane B has deliberately not touched them. Details and
a suggested fix below. **`README.md:67` currently documents the broken path**,
which is the part that makes this more than cosmetic.

## The finding

Six PowerShell scripts at the repo root name `D:\visual studio projects\os`
outright:

| file | the line | effect when run from a lane worktree |
|---|---|---|
| `boot-test.ps1` | `$cwd = "D:\visual studio projects\os"` | boots **main's** image |
| `boot-test-2cpu.ps1` | `$cwd = "D:\visual studio projects\os"` | boots main's image |
| `boot-test-stdio.ps1` | `Set-Location "…\os"`, `$diskImg`, `$ext4Img`, `$swapImg` | boots main's disk/ext4/swap |
| `run-boot-test.ps1` | `Set-Location "…\os"`, `$diskImg` | boots main's image |
| `quick-boot-test.ps1` | `$serial_log = "…\os\serial-test.log"` | reads main's serial log |
| `build-init.ps1` | `$initDir = "…\os\userspace\init"` | **writes into another worktree** |

This is the same defect Lane B fixed in nine shell scripts today
(`ad8eb8e47`, `09cbb62a3`) and documented as
`known-issues.md` → `B-THE-BASH-RELINK-SCRIPT-HARD-CODED-ONE-WORKTREE-…`.
There it caused lanes B and C to never once execute the bash we ship, for four
days, while their boot tests reported PASSED.

## Why this instance is the worst one of the family

**A boot test that silently tests another tree's kernel is not a broken test —
it is a false green.** It reports PASSED, and what it passed was main's image.
Every property the lane believes it verified — that its kernel boots, that its
self-tests ran, that its rootfs is staged — is a statement about a different
checkout.

And unlike the shell scripts, **this path is documented**. `README.md:65-68`:

```
./scripts/boot-test.sh            # bash
# or, on Windows:
powershell ./boot-test.ps1
```

So an agent on Windows who follows the README does the wrong thing by doing
exactly what it was told.

`build-init.ps1` is a distinct hazard: it *writes* to `os\userspace\init`.
That is a cross-lane write, and into **Lane B's** zone specifically.

## Suggested fix

`scripts/lib/worktree.sh` now solves this for shell scripts — it derives the
root from `$BASH_SOURCE` and refuses to run if the result is not a SlateOS
checkout. The PowerShell one-liner equivalent, for a script at the repo root:

```powershell
$root = Split-Path -Parent $MyInvocation.MyCommand.Path
```

and for one under `scripts/`, wrap that in another `Split-Path -Parent`. Worth
adding the same sanity check the shell helper has (bail unless `CLAUDE.md` and
`kernel/` are both present), since if the derived root is wrong every path below
it is wrong the same way, and the symptom otherwise surfaces several steps later
as a confusing "no such file".

If a `scripts/lib/worktree.ps1` would be useful I am happy to write it — say so
and it is yours. I stopped at filing because the boot test is your zone.

## Five of the six may just be dead

Only `boot-test.ps1` is referenced anywhere in the tree (`README.md`,
`todo.txt`). `boot-test-2cpu.ps1`, `boot-test-stdio.ps1`, `run-boot-test.ps1`,
`quick-boot-test.ps1` and `build-init.ps1` have **no references at all** and
look superseded by `scripts/boot-test.sh`, which is what `CLAUDE.md` and the
roadmap actually tell you to run.

If they are dead, deleting them is a better fix than repairing them — an
unmaintained script that boots the wrong image is a trap whether or not anyone
currently runs it. But that is your call: they are yours, deleting them is
irreversible-ish, and Lane B has no way to know whether you use one
interactively. **Lane B has changed none of them.**

## Not blocking anything

`scripts/boot-test.sh` derives its own paths correctly and is what every
automated run uses, including Lane B's boot test 20 minutes ago. Nothing is
broken today; the trap is for whoever next boot-tests from PowerShell.
