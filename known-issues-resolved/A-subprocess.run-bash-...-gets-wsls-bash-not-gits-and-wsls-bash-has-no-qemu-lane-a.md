## `subprocess.run(["bash", ...])` gets WSL's bash, not Git's — and WSL's bash has no QEMU (lane A)

**Status:** FIXED 2026-08-19 (in `scripts/layout-sweep.py`; the environment
fact itself is permanent and affects any script that spawns bash)

This machine has two bashes:

| | `pwd` of `D:\...\os-lane-a` | QEMU visible? | Opens `D:\...\boot-test.sh`? |
|---|---|---|---|
| `C:\Program Files\Git\usr\bin\bash.exe` | `/d/visual studio projects/os-lane-a` | yes | yes |
| `C:\Windows\System32\bash.exe` (WSL) | `/mnt/d/visual studio projects/os-lane-a` | **no** | **no** (exit 127) |

Windows `CreateProcess` does **not** search `PATH` first. It searches the
application directory, the current directory, **`System32`**, the Windows
directory, and only then `PATH`. So `subprocess.run(["bash", ...])` launches
WSL's bash whenever WSL is installed, no matter what `PATH` says.

`shutil.which("bash")` does not warn you, because it answers a different
question. Measured here:

```
shutil.which('bash')                        -> C:\Program Files\Git\usr\bin\bash.EXE
subprocess.run(['bash','-c','pwd']).stdout  -> /mnt/d/visual studio projects/os-lane-a
```

`which` implements a `PATH` search; the launcher does not. They disagree, and
the one that decides is the one that does not tell you.

WSL's bash is effectively a different machine: its own filesystem view, its own
`PATH`, and **no QEMU, no MSVC, no Windows Rust toolchain**. It parses
`boot-test.sh` perfectly and then cannot run it.

### One root cause, two symptoms, and a fix that only looked like one

This killed two release layout sweeps in succession:

1. **First sweep**: `bash: D:\...\scripts\boot-test.sh: No such file or
   directory`, exit 127, from a file that was plainly present — WSL's bash
   cannot open a drive-letter path with backslashes. Diagnosed as an absolute-
   vs-relative path bug and fixed by naming the script relatively. The error
   went away, which read as the bug being solved.
2. **Second sweep**: `ERROR: qemu-system-x86_64 not found`, one second in. Same
   root cause. The relative-path fix moved the failure two lines further down
   `boot-test.sh` and changed nothing else.

The relative name was, and remains, correct — it just was never the disease.
Note also that the first diagnosis blamed *MSYS*; it was WSL. Git's bash
handles the absolute Windows path fine. Any comment in this tree claiming
otherwise predates 2026-08-19.

### The fix

`layout-sweep.py` resolves bash **explicitly**, via `find_bash()`, most-specific
candidate first with bare `bash` last (it is the correct answer on a real Linux
host and the wrong one here, so it is a fallback rather than a preference). Each
candidate must demonstrate two things before it is accepted:

- `bash -n scripts/boot-test.sh` parses — proving this bash can open the exact
  argv string the sweep will pass. Not `os.path.exists`: the file existed the
  whole time.
- `extract_dependency_probe()` — the boot test's **own** `# Find QEMU` and
  `# Find OVMF` blocks, lifted out verbatim and run under that candidate. Both
  bashes pass the parse; only one passes this. Extracted rather than restated,
  because a Python translation of those candidate lists is a second opinion
  about where QEMU is, and a preflight that says "found" while the boot test
  says "not found" is worse than no preflight — it certifies the sweep and then
  the sweep dies anyway. It raises rather than returning an empty fragment if
  the block moves.

The chosen bash is printed, so which one ran is on the record rather than
inferred.

### Consequence for tests

`test-boot-test.py` used to run the dirty-check fragment under bare `bash` —
i.e. under WSL, while `boot-test.sh` in production runs under Git's bash. It
was validating a check that a *different git* would actually perform, on the
very axis (`core.autocrlf`, pathspec handling) the check is sensitive to. It
now enumerates every distinct bash on the machine via `available_bashes()`
(de-duplicated by each one's own `pwd`, not by path, since several candidates
are frequently the same binary) and asserts the check gives the **same verdict
under all of them**. That is strictly stronger than picking the right one, and
needs no coupling to `layout-sweep.py`.

### If you are writing a script that spawns bash

Do not pass the bare string `"bash"`. Resolve it, and verify the resolved one
can do the thing you need — not merely that it exists.
