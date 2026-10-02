## B-SEVEN-COMMENTS-SAID-THE-KERNEL-CANNOT-DO-SOMETHING-IT-CAN (lane B, 2026-09-13)

**In short:** libc is full of comments explaining that a function does nothing
*because the kernel cannot do the thing*. Seven of those were out of date, and
each one was pointing the next reader at the wrong repair.

### Why it was worth sweeping

Two of them cost me real time the same day before I thought to look for the
pattern. `_start`'s "the kernel jumps here with no arguments on the stack
(argc/argv not yet supported)" reads, next to a disassembly that matches it
instruction for instruction, as proof that no SlateOS program can see its
arguments — it can, one call further in. `siginterrupt`'s "our OS doesn't
deliver signals" is the reason its flag was ignored, and signals have been
delivered for a long time.

The search was for the shape rather than any one instance: comments matching
*our OS doesn't*, *we don't have*, *no kernel support*, *the kernel doesn't*.
**60 candidates in `posix/src`. Seven were stale**, and four of those had a
code fix behind them.

| claim | reality | outcome |
|---|---|---|
| `_start`: kernel doesn't provide argv | `SYS_PROCESS_GET_ARGS` does, and the kernel also builds a SysV stack | comment fixed; the retry bug beneath it fixed |
| `siginterrupt`: OS doesn't deliver signals | trampoline registered in `crt.rs`; kernel reads `SA_RESTART` | **flag now honoured** |
| `__stack_chk_guard`: no `/dev/urandom` | `SYS_GETRANDOM` wired; kernel supplies `AT_RANDOM` | led to the **zero-canary** fix in `tls.rs` |
| `access()`: no permission system | ACLs and capability file-tags are enforced; mode bits stored | comment fixed; real gap recorded |
| `readahead`: no page cache | `fs/cache.rs` is a write-back LRU block cache | comment fixed |
| `posix_fadvise`: kernel ignores hints | `fs/fscache.rs` manages per-device read-ahead policy | comment fixed |
| `sync_file_range`: no writeback cache | the block cache *is* write-back and `fsync` reaches it | comment fixed |

### What the three remaining no-ops are actually missing

Not machinery — a **syscall**. The kernel has the block cache, the read-ahead
policy layer and a working sync; what `kernel/src/syscall/number.rs` does not
have is anything exposing prefetch or a *range* sync. `SYS_FS_SYNC` (641) takes
no fd and no offset, so `fsync`, `fdatasync` and `sync_file_range` all
necessarily collapse onto a global flush.

That has a consequence worth knowing for the `RWF_DSYNC` work landed earlier
today: honouring it costs a **global** sync per write. Correct, and expensive.
The fix is a per-fd sync syscall, not a weaker promise in libc, and the policy
doc now says so rather than leaving the next reader to discover it under load.

### The rule

A comment saying *we cannot* is a claim about another subsystem, and it is the
kind that rots silently — the subsystem gains the capability and nothing tells
the file that assumed otherwise. A comment saying *we do not* is a claim about
this file, and stays true until this file changes. Where the distinction is
available, prefer the second.
