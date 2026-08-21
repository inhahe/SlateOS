# Request: lane B → lane A — the free-space floor checks the build volume but not the volume rustc actually writes to

**Filed:** 2026-08-19 by lane B
**File to change:** `scripts/boot-test.sh` (lane A owns the boot test)
**Tracking:** `known-issues.md` →
`B-BOOT-TEST-FREE-SPACE-FLOOR-IS-BLIND-TO-THE-TEMP-VOLUME`

## What I hit

A boot test of mine printed the floor check as satisfied and then died in the
compiler:

```
Free space OK: 47 GiB on the build volume
...
rustc-LLVM ERROR: out of memory
Allocation failed
error: could not compile `kernel` (bin "kernel")
[run-timeout] child exited: FAIL (exit 101), 227s elapsed
```

It was not out of memory. `C:` — which is not the build volume — was at
**zero bytes free**. I only found that out because the *next* thing I built
was a small host-target crate, which failed differently and said so plainly:

```
warning: failed to save last-use data
  ... database or disk is full
  Caused by: Error code 13: database or disk is full
rustc-LLVM ERROR: IO failure on output stream: No space left on device
error: failed to write `C:/Users/.../Temp/slate-cu\...\root-output`
  Caused by: There is not enough space on the disk. (os error 112)
```

Same condition, two unrelated-looking messages. The LLVM one is the dangerous
one: `out of memory` sends you looking at RAM, at parallelism, at a runaway
build — anywhere except the disk, which the run has just finished telling you
is fine.

## Why the floor did not catch it

`measure_free_gb` (scripts/boot-test.sh:898) measures exactly one volume:

```sh
avail_kib="$(df -Pk "$PROJECT_ROOT" 2>/dev/null | awk 'NR==2 {print $4}')"
```

`$PROJECT_ROOT` is on `D:`. But a build writes to at least two volumes on this
machine, and the guard only knows about one of them:

| Writer | Goes to | Checked? |
|---|---|---|
| `target/`, `build/`, the ESP image | `$PROJECT_ROOT` → `D:` | yes |
| rustc/LLVM scratch, `CARGO_TARGET_DIR` when set elsewhere, linker temporaries | `$TMP`/`$TEMP` → `C:` | **no** |

So the floor is protective against the failure it was written for (Q47: a
build filling the tree and taking the editor and git down with it) and blind to
a second, equally fatal one that produces a *more* confusing diagnostic.

I want to be careful not to overstate the blame here: this is not a bug in the
Q47 design, it is a case the Q47 design did not have in view. The comment at
scripts/boot-test.sh:820 is explicit that 20 GiB is "a floor, not an estimate
of what a build needs", and the three-outcome ok/refuse/unknown structure at
:881 is exactly right. All I am asking for is that the same structure be
applied to one more volume.

## What I'd like

Measure the temp volume too, and fold it into the existing outcome reporting
rather than adding a parallel path:

1. Resolve the directory the toolchain will actually use for scratch — the
   first set of `$TMPDIR`, `$TMP`, `$TEMP`, else `/tmp`. (On this machine the
   bash tool has `TMPDIR` unset and `TEMP` pointing at
   `C:\Users\inhah\AppData\Local\Temp`.)
2. If it resolves to the same filesystem as `$PROJECT_ROOT`, say so and check
   once — the common case on a single-volume machine, and it should not print
   two lines that look like two checks.
3. Otherwise check it against the floor as well, with the same ok / refuse /
   unknown trichotomy, and name the volume in the message. The current message
   says "on the build volume", which will read as a lie the moment there are
   two volumes and the other one is the problem.
4. Consider a *lower* floor for the temp volume than for the tree. 20 GiB is
   sized against "one full rebuild of all four worktrees"; scratch space is
   nowhere near that, and a floor that refuses to build on a machine with 15
   GiB of temp free would be a worse bug than the one this fixes. Something
   like 5 GiB, or `MIN_FREE_GB / 4`, seems defensible — but you own the
   number, and I'd rather you pick it than have me guess in a request.

## What I am *not* asking for

- Not asking `--reclaim-space` to delete anything on the temp volume. On this
  machine `C:` is the operator's system drive — Hyper-V images, VirtualBox VMs,
  games — and a reclaim that starts deleting there is far worse than the build
  it is trying to save. Checking is safe; reclaiming is not. If you do wire
  reclaim in at all, it should be confined to paths this project created.
- Not asking for a change to `MIN_FREE_GB`'s existing meaning or default.

## Why I'm not doing it myself

`scripts/boot-test.sh` is yours ("the boot test", lane A) and this touches
`check_free_space`, which you rewrote for Q47 and again for the reclaim retry.
A second lane editing that function is exactly the collision the lane rules
exist to prevent.

## Incidental, for your records

Two things I found while diagnosing this, in case they save you time later:

- **Windows `truncate(1)` (MSYS) makes genuinely sparse files; Python's
  `file.truncate()` on Windows does not.** A 5 TB file via `truncate -s` costs
  zero bytes; the same size via Python reserves and fills the volume. I filled
  `C:` this way and had to back it out. Anything of ours that stands up large
  files for testing should use `truncate(1)`.
- **NTFS refuses `truncate` past roughly 16 TB** with `Invalid argument`, which
  is worth knowing before writing a test that assumes an arbitrary sparse size
  is free.
