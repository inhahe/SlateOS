## TD-B-AN-ARGUMENT-LIST-TOO-LONG-REPORTS-EINVAL-WHERE-POSIX-SAYS-E2BIG (lane B, 2026-09-13)

**In short:** spawn a process with more arguments than the system allows and the
error says "invalid argument" instead of "argument list too long". A caller that
handles the documented error — shrink the list and retry, which is what `xargs`
exists to do — has no way to tell that is the right response.

**Measured, not assumed**, following the value from one end to the other:

| step | where | value |
|---|---|---|
| kernel refuses an oversized list | `kernel/src/proc/pcb.rs:6659` | `Err(InvalidArgument)` when `total > MAX_ARGS_BYTES` (256 KiB) |
| libc maps the native error | `posix/src/spawn.rs` | `native_to_posix_err(ret)` |
| the mapping | `posix/src/errno.rs:377` | `native::INVALID_ARGUMENT => EINVAL` |

POSIX requires `E2BIG` for exec when the argument and environment lists exceed
`ARG_MAX`. `E2BIG` exists in this tree and is returned in `file.rs`, `iconv.rs`
and `linux_bpf.rs` — just never on the path that is actually about an argument
list being too long.

**Why this is recorded rather than fixed.** The obvious fix is for libc to check
the packed length itself and return `E2BIG` before calling, which needs no
kernel change and is testable on the host. What it needs first is a decision
about *which* limit to enforce, and the two available answers disagree:

- **`ARG_MAX` (128 KiB)** — what `sysconf(_SC_ARG_MAX)` already promises, so
  enforcing it makes libc self-consistent. But it would start refusing spawns
  between 128 KiB and 256 KiB that succeed today.
- **The kernel's 256 KiB** — preserves current behaviour exactly and only
  changes the errno. But then `sysconf` still advertises a number that is not
  the one enforced, which is the same self-contradiction that made the argv
  entry above bite the careful caller rather than the careless one.

That is a user-visible behaviour choice with a real trade on both sides, so it
is not one to make while passing through. The second option is the smaller
change and the one I would take — errno correctness without a behaviour
regression — with the `ARG_MAX` mismatch left as its own question.

**Not urgent.** The current failure is a refusal with a misleading name, not a
silent loss: the spawn does fail, and the caller does get an error. That is
strictly better than the 64 KiB case in the entry above, which is why that one
was fixed on the spot and this one is written down.
