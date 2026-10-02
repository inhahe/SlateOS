### [E] The NUL-visibility gate ran in WSL by accident, and stopped the boot test whenever WSL was slow to start -- 2026-09-25 -- FIXED 2026-09-25
**Status:** FIXED 2026-09-25 -- `scripts/check-cp-diff-sees-nul.py`, with a
paired suite, `scripts/test-check-cp-diff-sees-nul.py` (which the pre-push hook
is meant to run when the checker is pushed, and does not yet on most pushes:
see the next entry). Four launches of the same shape in lane A's boot-test
suites are open: `requests/e-a-bare-bash-in-boot-test-suites.md`.

**In short:** one of the boot test's early gates checks that `cp-diff.sh` can
still see a difference made only of NUL bytes. It started its probe with
`subprocess.run(['bash', ...])`, and on Windows that is WSL's bash --
`CreateProcess` searches System32 before `PATH` -- so every boot test quietly
started a Linux VM. When the VM did not come up in time, the gate's self-test
said it "fails its own cases" and the boot test refused to build a tree that
nothing was wrong with.

**How it was found.** A lane-E boot test on `731de9b3e` stopped at "refusing
to build. The NUL-visibility gate fails its own cases", quoting a UTF-16
message: "The operation timed out because a response was not received from the
virtual machine or container. Error code:
Bash/Service/CreateInstance/CreateVm/HCS_E_CONNECTION_TIMEOUT". Measured
afterwards in the same tree: `subprocess.run(['bash', '-c', 'uname -s; pwd'])`
prints `Linux` and `/mnt/e/...`, while `proctree.find_unix_shell()` answers
Git's bash.

**Three faults, fixed together:**

1. **The shell.** The probe runs under `proctree.find_unix_shell()` -- the
   project's one resolver: `SLATE_BASH`, then `PATH` minus the WSL shim, then
   the known Git and MSYS2 paths -- and prints the shell it ran under.
2. **The tools.** It puts that shell's own `/usr/bin` first. A Git bash
   started from a PowerShell parent inherits that `PATH` and finds
   `C:\Windows\System32\find.exe` and `sort.exe`, and no `sha256sum` at all
   (measured with `PATH=C:\Windows\System32`).
3. **The exit codes**, as `scripts/run-checker.sh` reads them. A probe that
   could not run is 2, no verdict; it was 1, a finding, printed directly under
   the sentence "A gate that cannot run must say so, not report a finding".
   Its two skips -- no shell, no `sha256sum` -- are 3, listed as skipped; they
   were 0, counted as passes.

The comments that blamed MSYS for two earlier faults -- a `C:/Users/...` path
that would not resolve, and a function defined by `bash -c` that was invisible
inside `$(...)` -- were written from inside the VM. The second reproduces
exactly under WSL's launcher (`declare -F f` succeeds, then
`/bin/bash: line 1: f: command not found`) and not at all under Git's bash;
the comments now say what was measured.

**Still open, filed rather than edited:** `scripts/test-boot-test.py` (three
harness launches) and `scripts/test-boot-history-commit.py` (one) pass the
bare word too, so they test extracted `boot-test.sh` functions under WSL's
bash and git while production runs them under Git's. They run only when
`boot-test.sh` changes, they are lane A's suites, and `scripts/**` is unowned
(A-Q11) -- hence the request.
