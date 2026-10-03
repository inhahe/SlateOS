### TD-HARNESS-RUN-TIMEOUT-COULD-NOT-LAUNCH-A-SHELL-SCRIPT-AND-BARE-BASH-MEANT-WSL. The documented boot-test invocation never ran the boot test — 2026-08-12 — ✅ FIXED 2026-08-12 (`scripts/proctree.py`)

**What.** `CLAUDE.md` documents the canonical hang-proof invocation as:

```bash
python scripts/run-timeout.py 60 ./scripts/boot-test.sh
```

That command could never work on this machine, and both of its failure modes
produced a *misleading* run rather than an error:

1. **A `.sh` cannot be launched by `CreateProcess`.** Windows has no shebang
   handling, so `Popen(["./scripts/boot-test.sh"])` fails with `[WinError 193]
   %1 is not a valid Win32 application`. `run-timeout.py` correctly returned
   125 — but a caller that appends `; echo EXIT=$?`, or otherwise reports the
   *wrapper's* status rather than the runner's, converts "the boot test never
   started" into a green result. That is exactly what happened: a boot test
   reported exit 0 having never booted anything.

2. **Naming `bash` explicitly is worse, because it appears to work.** From a
   native-Windows parent, `CreateProcess` searches `System32` *before* `PATH`,
   and `C:\Windows\System32\bash.exe` is the **WSL launcher**. So
   `Popen(["bash", "scripts/boot-test.sh"])` runs a *Linux* bash in a different
   filesystem namespace, where `/c/Program Files/qemu`, `cygpath` and
   `taskkill //F` do not exist. The observed symptom was
   `ERROR: qemu-system-x86_64 not found` — a failure with nothing whatsoever to
   do with the code under test, and one that invites you to go looking for a
   broken QEMU install.

   The trap is sharpened by `shutil.which("bash")` **not** reproducing it: it
   walks `PATH` and answers Git Bash. So the PATH lookup and the actual launch
   disagree, and only the launch is authoritative.

**Why it matters beyond the boot test.** `proctree.Tree` is the single launch
point behind both `run-timeout.py` and `run_captured`, and CLAUDE.md directs
*all* potentially-hanging commands through it. Any script-based harness
(`boot-test.sh`, `flake-hunt.sh`, `wedge-soak.sh`, the `p3x-check.sh` gates)
was unreachable through the one runner that guarantees process-tree cleanup —
which pushes callers back onto bare `timeout`, the orphan-leaking tool
`proctree` exists to replace.

**The fix.** `proctree.resolve_command()`, called from `Tree.__init__` so every
caller gets it:

- argv[0] a shell script (by `.sh`/`.bash` extension, or by parsing its `#!`
  line) → interpose an absolute bash. The shebang interpreter is matched on its
  **basename** after resolving one level of `/usr/bin/env`, so
  `#!/home/shared/bin/python3` is not mistaken for a shell (a substring test
  for `sh` would have been).
- argv[0] a bare `bash`/`sh` → rewritten to an absolute MSYS/Git-Bash path,
  with `System32`/`SysWOW64`/`Sysnative` rejected by **location** so the WSL
  shim can never be selected.
- No shell found → raise `OSError` with a specific message. Falling back
  silently is the failure this exists to prevent, so it must be loud.
- Everything else (`cargo`, `taskkill`, a real `.exe`, an explicitly-pathed
  interpreter, a non-list command) passes through untouched, and on POSIX the
  whole function is a no-op — the kernel honours `#!` itself.

`SLATE_BASH` overrides the search for a host whose shell lives elsewhere.

**Tests.** `scripts/test-proctree.py` (plain `python`, no pytest dependency):
29 checks covering passthrough, script/bare-shell rewriting, WSL-shim
detection, shebang parsing (including the `sh`-inside-a-path and `fish`
false-positive cases), and two end-to-end runs — one asserting the resolved
shell is really MSYS, one asserting `Tree(["...sh"])` runs a script *and
reports the script's own exit code* rather than a launch failure.
