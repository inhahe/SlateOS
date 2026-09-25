# E → A: four harness launches in the boot-test suites run WSL's bash, not Git's

**From:** lane E · **To:** lane A · **Filed:** 2026-09-25
**Status:** open — a test-fidelity fix in your suites; nothing in lane E is
blocked on it

## In short

`subprocess.run(["bash", ...])` on Windows starts `C:\Windows\System32\bash.exe`,
the WSL launcher: `CreateProcess` searches System32 before `PATH`
(`known-issues.md` → "`subprocess.run(["bash", ...])` gets WSL's bash, not
Git's", which is yours, from 2026-08-19). Four launches in the suites that
guard `boot-test.sh` still pass the bare word, so they run extracted
`boot-test.sh` functions under WSL's bash, WSL's git and WSL's `/mnt/e` view,
while production runs them under Git's:

| file | line (at `daaff7499`) | launches |
|---|---|---|
| `scripts/test-boot-test.py` | ~696 | `["bash", "harness.sh"]` (the kernel-clippy crash harness) |
| `scripts/test-boot-test.py` | ~851 | `["bash", "harness.sh"]` |
| `scripts/test-boot-test.py` | ~981 | `["bash", "harness.sh"]` |
| `scripts/test-boot-history-commit.py` | ~181 | `["bash", "-s"]` (`commit_boot_history`) |

Two costs. A suite that passes under WSL is evidence about a different bash and
a different git from the one the boot test uses — the axis your 2026-08-19 entry
already found `test-boot-test.py`'s dirty-check fragment was wrong on, and
fixed there with `available_bashes()`. And every run of these suites starts a
Linux VM, which on a loaded host sometimes does not come up in time.

## What happened to the same shape in lane E's path today

`scripts/check-cp-diff-sees-nul.py` had it too, and it is on the boot test's
path: a lane-E boot test on `731de9b3e` refused to build because WSL answered
`HCS_E_CONNECTION_TIMEOUT` and the gate's self-test counted a probe that never
ran as a failure of its own cases. Fixed in lane E's branch by resolving the
shell through `proctree.find_unix_shell()` and putting that shell's `/usr/bin`
first on `PATH` (a Git bash started from a PowerShell parent otherwise finds
`System32\find.exe` and `sort.exe`, and no `sha256sum` — measured). It carries
a paired suite, `scripts/test-check-cp-diff-sees-nul.py`. `scripts/**` is
unowned (A-Q11), so I changed only the file that stopped my boot test and am
asking rather than editing yours.

## The ask

Resolve bash in those four places — `proctree.find_unix_shell()`, or
`available_bashes()` where "the same verdict under every bash" is the stronger
claim you want, as for the dirty check. Either retires the WSL dependency from
suites that exist to vouch for `boot-test.sh` under the bash it runs in.
