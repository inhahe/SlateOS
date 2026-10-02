### [A] BLOCKED: WSL is down on this host, so every lane-A boot fails at gate 50 -- 2026-09-22
**Status:** RESOLVED 2026-09-24 — WSL answers again (`wsl -d Ubuntu -- bash -s` returned `5.2.21(1)-release`); nothing in the tree changed. The request about telling a quiet WSL from a broken checker still stands on its own merits.

**In short:** the boot test asks a real bash, running inside WSL, whether the
kernel's shell-quoting rules match bash's. WSL on this machine now answers
*"Catastrophic failure"* to everything, so that gate cannot reach a verdict and
the build is refused. No boot can complete until WSL works again.

```
$ wsl -d Ubuntu -- echo ok
Catastrophic failure
```

Checked four times over ~1.5 hours; it is not recovering on its own. Distros
registered: `Ubuntu`, `docker-desktop`.

**What is and is not affected:**

| | state |
|---|---|
| `cargo check`, `cargo clippy`, the Python gates | unaffected -- all pass |
| `git push` | unaffected -- the hook's WSL gates decline cleanly |
| `check-shellquote-vs-bash.py --self-test` | unaffected, 82/82 -- it needs no bash |
| the same checker's **main** run | **fails**, exit 1, and `_bash_oracle_disagreed` refuses the build |

**Three ways out, none of which I took, and why:**

| option | why not |
|---|---|
| `wsl --shutdown` or `--terminate Ubuntu` | a blanket restart of a runtime **all three lanes share**, and `CLAUDE.md` forbids blanket action on a shared runtime. If another lane is mid-`create-ext4-rootfs.sh` it could corrupt their image |
| patch `scripts/bashprobe.py` to exit 2 (skip) when WSL answers nothing | **outside lane A's write scope** -- `which-lane.py` gives me `kernel/**`, `bench/**`, the toolchain json and `scripts/boot-test.sh` only. Filed as `requests/a-b-bashprobe-cannot-tell-a-quiet-wsl-from-a-broken-checker.md` instead |
| pre-check WSL in `boot-test.sh` (mine) and skip the bash-oracle gates when it is unavailable | defensible -- `--may-skip` already means "this gate may decline when its oracle is absent", and a broken WSL is an absent oracle. But it changes boot behaviour for **all three lanes**, unilaterally, to unblock one. That is an operator call |

**What the operator can do:** repair WSL (a `wsl --shutdown` from a human who
knows no lane is mid-build usually suffices), or tell me the third option above
is acceptable and I will implement it in `boot-test.sh` behind a loud notice.

**State of the work this blocks.** Six of eight per-file metadata tables are
converted and committed; `flock`'s identity rung, both range self-tests, the
brightness ordering fix and the DNS hosts table are **verified by boot**. Four
identity rungs -- `sealing`, `reclock`, `immutable`, `acl` -- are written, wired,
and have never executed, because they sit downstream of the `ctest-pty` hang
recorded above and that boot never reached them.
