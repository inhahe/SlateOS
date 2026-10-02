## B-BOOT-TEST-FREE-SPACE-FLOOR-IS-BLIND-TO-THE-TEMP-VOLUME (lane B, 2026-08-19) — filed to lane A

**In short:** the boot test refuses to run when the *project* disk is nearly
full, which is the right idea, but it only ever looks at that one disk. The Rust
compiler also writes scratch files to the Windows temp folder, which on this
machine is on a different disk. When that other disk filled up, the boot test
announced "Free space OK: 47 GiB" and then died with `rustc-LLVM ERROR: out of
memory` — a message about RAM, for a problem that was entirely about disk.

**Where:** `scripts/boot-test.sh:898` (`measure_free_gb`), which measures
`df -Pk "$PROJECT_ROOT"` and nothing else. Lane A owns the boot test; filed as
`requests/b-a-free-space-floor-does-not-check-the-compiler-s-temp-volume.md`.

**How it presented.** Two builds, one cause, two diagnostics that share no
words:

```
Free space OK: 47 GiB on the build volume        <- D:, genuinely fine
rustc-LLVM ERROR: out of memory                  <- C:, at zero bytes free
Allocation failed
[run-timeout] child exited: FAIL (exit 101), 227s elapsed
```

```
warning: failed to save last-use data ... database or disk is full
rustc-LLVM ERROR: IO failure on output stream: No space left on device
error: failed to write `C:/Users/.../Temp/slate-cu/.../root-output`
  Caused by: There is not enough space on the disk. (os error 112)
```

The second is self-explanatory; the first is actively misleading, and it is the
one the boot test produces. An hour can go into looking at parallelism and RAM
before anyone thinks to run `df` on a volume the run never mentioned.

**Why it is the guard's problem rather than just bad luck.** The floor exists
(Q47) so that a build cannot fill the tree and take the editor and git down with
it. That reasoning applies unchanged to the volume holding `$TMP`: if it fills,
the build dies too, and takes rather more with it. The guard's ok/refuse/unknown
trichotomy is right and its comment is careful to call 20 GiB a floor rather
than an estimate — this is a volume it did not have in view, not a design error.

**Proper fix** (detail and rationale in the request): resolve `$TMPDIR`/`$TMP`/
`$TEMP`/`/tmp`, check it against the floor too when it is on a different
filesystem from `$PROJECT_ROOT`, say which volume failed, and use a smaller
floor for scratch than the 20 GiB sized for four full worktree rebuilds.
Deliberately *not* part of the ask: letting `--reclaim-space` delete anything on
that volume — here it is the operator's system drive.

**Workaround until then:** when a build reports an LLVM OOM, run `df -h` across
all volumes before believing it.

**Two incidental findings, recorded because they cost time to establish:**

- MSYS `truncate(1)` creates genuinely sparse files on NTFS — a 5 TB file costs
  zero bytes — but Python's `file.truncate()` on Windows does **not**; it
  reserves, and will fill the volume. This is how the disk got full in the first
  place. `scripts/gen-human-fixture.sh` depends on the sparse behaviour and
  guards it: it stands up one 5 TB probe, re-reads free space, and refuses to
  run if the probe cost more than a GiB.
- NTFS rejects `truncate` past roughly 16 TB with `Invalid argument`, which caps
  how far a sparse-file-based sweep can reach.
