## TD-B-THE-TREE-IS-ON-A-SPINNING-DISK-AND-BUILDS-SPEND-THEIR-TIME-LISTING-A-69000-FILE-DIRECTORY (lane B)

**Status:** OPEN — 2026-09-05

**In short:** Builds here regularly appear to hang — minutes with no output and
almost no CPU used. They are not hung. The whole project lives on `D:`, which
is a **mechanical hard disk** (a spinning platter, ~200× slower per read than a
solid-state drive), and one build directory in it now holds **69,161 files**.
Every `rustc` invocation has to list that directory before it can start, and on
this disk that listing alone can take many minutes. Two solid-state drives sit
in the same machine, one of them **325 GB free and essentially unused**.

**How to see it.** Attach a debugger to the `rustc` that looks stuck — it is
not spinning, it is waiting on the filesystem:

```
$ cdb -p <rustc pid> -pv -c "~*k; q"
  ...
  ntdll!NtQueryDirectoryFileEx
  KERNELBASE!FindNextFileW          <-- here, for minutes
  rustc_metadata!...find_library_crate
```

and the per-drive counters say why:

| Drive | Device | Kind | Avg. read | Free |
|---|---|---|---|---|
| `C:` | Samsung 980 PRO | NVMe SSD | **0.13 ms** | 129 GB |
| `D:` | WDC WD2004FBYZ | **SATA HDD, 7200 rpm** | **27.5 ms** | 333 GB |
| `E:` | Samsung 960 EVO | NVMe SSD | — (idle) | **325 GB** |

`D:` also sat at an average queue length of ~8 during a build — eight requests
waiting on a device that serves one at a time.

**Where it bit, three times in one session:**

- A `cargo build -p sshd` sat silent for **9 minutes** having consumed **0.6
  seconds of CPU**. It was enumerating
  `target/x86_64-pc-windows-gnu/debug/deps` (69,161 files, 63.9 GB) — the `-L`
  directory rustc must scan to resolve every `extern crate`.
- `rd /s /q` on that directory ran **45 minutes** and freed no space before it
  was stopped. It had got as far as `.fingerprint`.
- Renaming the directory aside — normally an O(1) metadata operation — returned
  *Access is denied* on two subdirectories and simply never returned on `deps`
  after 6 minutes, while unrelated renames in the same parent succeeded
  instantly. The saturated queue, not a stuck handle, is the explanation.

**Why the directory is that large,** and why deleting it does not settle the
matter: it is *one generation* of build output for the 2,288 fabricated
userspace command crates described in the `open-questions.md` entry "2,288 of
the 2,756 commands in `userspace/` report success for work they never did." It
is not stale accumulation from months of work. Clear it and the next full build
recreates it. So the size is a symptom of that open question, but the *latency*
is separate and is fixable on its own.

**What the proper fix is,** cheapest first:

1. **Move the three `target/` directories to `E:`** — either
   `build.target-dir` in each worktree's `.cargo/config.toml`, or an NTFS
   junction (`mklink /J`) so no configuration changes at all. This needs no
   administrator, touches no source, is reversible by deleting the junction,
   and puts the 69,161-file directory on a device with a 0.13 ms read. It does
   not move the *source* tree, so `git` operations stay on the HDD, but git is
   not what is slow here.
2. **Move the whole project to `E:`.** Strictly better, strictly more
   disruptive: three worktrees, absolute paths in scripts and in this
   documentation, and the operator's own shortcuts.

**Relation to A-Q7.** Lane A's open question about antivirus is framed around
"how we know it is not the disk." It *is* the disk. A-Q7's option 3 (relocate
the tree) was written as though it required `C:` and an administrator; it
requires neither, because `E:` is empty and writable. An addendum saying so is
already filed under A-Q7 in `open-questions.md`.

**Cost while unfixed:** every cold build pays minutes of directory enumeration
per crate, and — worse than the time — the stalls are indistinguishable from a
deadlock, so they get "diagnosed" repeatedly. Two separate investigations in
one session went to a debugger before concluding the machine was merely slow.

**If never fixed:** it worsens monotonically. The `deps` directory grows with
the crate count, enumeration is linear in it, and the disk does not get faster.

### Addendum 2026-09-05 (lane B) — measured, and a workaround that needs no filesystem surgery

Both halves of the fix above were attempted. **Option 1 does not work as
written**: renaming `target` aside so a junction can take its place fails with
*Access is denied*, and it is not a permissions problem — `icacls` shows
`Authenticated Users:(M)`, which includes delete. NTFS refuses to rename a
directory while any file beneath it is open, and something in that 69,161-file
tree is held open by a process this account cannot enumerate (`handle.exe`
needs administrator). Both `move` and `ren` fail identically; unrelated
directories in the same parent rename instantly. So the junction is blocked
until whatever holds it is identified, which needs an elevated `handle.exe`.

**What does work, immediately and with no filesystem changes at all**, is to
leave the old directory where it is and send new output elsewhere:

```bash
CARGO_TARGET_DIR="E:/slateos-build/lane-b" cargo build -p sshd --target x86_64-pc-windows-gnu
```

It is per-command, so it cannot disturb another lane, and there is nothing to
undo. It must not be committed to `.cargo/config.toml`: that file is tracked
and shared, so a `build.target-dir` there would follow the branch into `main`
and point all three lanes at one directory.

**The measurement, same crate, same cold cache, half an hour apart:**

| Target directory | Result |
|---|---|
| `D:` (HDD, 69,161-file `deps`) | **killed at 11 min**, still inside `rustc sshd`; 0.7 s of CPU consumed in the last 6 of those minutes |
| `E:` (NVMe, empty `deps`) | **2 min 09 s**, complete, including `posix`, `userdb`, `authlib` and `sshd` from scratch |

The second run built strictly *more* than the first and finished more than five
times faster, which settles the question of where the time was going. Later
runs in the same session: `cargo test -p sshd` 136 s and
`cargo clippy -p sshd --all-targets` 111 s, both from a warm cache on `E:`.

Reclaiming the 63.9 GB still sitting on `D:` is now independent of all this —
nothing builds into it any more — but it is not free: `rd /s /q` ran 45 minutes
against it and freed nothing. `robocopy /MIR` from an empty directory is the
usual faster route on Windows and is worth trying before another `rd`.
