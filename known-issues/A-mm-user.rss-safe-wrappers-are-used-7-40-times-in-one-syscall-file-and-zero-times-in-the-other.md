### [A] `mm/user.rs`'s safe wrappers are used 7-40 times in one syscall file and zero times in the other -- 2026-09-21
**Status:** FIXED for this class -- all 46 `vec![0` in `linux.rs` classified, the 12 user-sized ones converted, plus 7 hand-rolled blocks folded into the same helper. OPEN for the four other wrappers that still have zero uses there

**In short:** the kernel has a set of helper functions whose whole job is to
copy data safely between a program and the kernel. One of the two files that
handles system calls uses them constantly. The other has never used a single
one, and writes the unsafe version by hand every time. That is where today's
one-syscall crash came from.

**Measured.** Every `pub fn` in `mm/user.rs`, counted in both syscall files:

| helper | `handlers.rs` | `linux.rs` |
|---|---|---|
| `read_user_vec` | 40 | **0** |
| `with_user_out_buf` | 15 | **0** |
| `write_user_items` | 11 | **0** |
| `alloc_zeroed_vec` | 10 | **0** |
| `read_user_items` | 7 | **0** |

*(Counting caveat, stated because it decides what the table is worth: these
are substring counts, so the positives are inflated -- `copy_from_user`
matches inside `copy_from_user_as`. A **zero** cannot be inflated, so the
column carrying the finding is the reliable one.)*

**And the contrast is qualitative, not just a count.** `handlers.rs` shows
one hit for `vec![0` -- inside a comment, explaining why it is not used:

> Read into a kernel buffer, then copy out. Besides keeping raw user
> pointers out of the VFS, this makes the staging allocation fallible --
> `vec![0u8; buf_cap]` would abort the kernel on a large failed read.

followed by a call to `with_user_out_buf`. So that file has **zero**
aborting allocations and an author who understood the hazard well enough to
write it down at the call site. The sibling file has 46 and had never met
the idea. This is not a style difference or a tidiness gap: the same
knowledge, in the same crate, one directory apart, present on one side of a
file boundary and absent on the other.

**Why it matters rather than being a style difference.** `alloc_zeroed_vec`'s
own doc names the exact defect that shipped:

> `vec![0u8; n]` and `Vec::from(slice)` are *infallible* allocations: on
> exhaustion they call the allocation-error handler, which in a kernel means
> the whole system goes down. Since `n` here is usually derived from a
> syscall argument, that turns an OOM into a userspace-triggerable panic.

That paragraph has been sitting in `mm/user.rs` the whole time, one directory
from `syscall/linux.rs`, which contains **46** `vec![0` and had **zero** uses
of the helper. The knowledge did not cross a file boundary. `poll`, `select`,
`memfd`, `pread` and `getdents64` all live on the wrong side of it.

**This is a third shape of the same family recorded today.** The others were
a delegation to a tool nobody runs, and a cross-reference that was never
written. This one is a remedy that exists, is documented with its own threat
model, is used ten times -- and stops at a file boundary. None is a missing
check; all three are **knowledge that exists somewhere it cannot be seen from
where it is needed.**

**What was done.** All 46 `vec![0` sites in `linux.rs` were classified, so
this converts the class rather than a sample:

| class | count | verdict |
|---|---|---|
| literal size | 5 | safe |
| named constant | 10 | safe |
| variable, clamped at its `let` | 9 | safe, mostly `.min(4096)` |
| self-test, sized by `PAYLOAD.len()` | 9 | kernel constants, not user input |
| bounded by an `if` earlier in the function | 1 | `get_nodes_uma`, capped at `NODE_BITS_CAP` |
| **user-sized** | **12** | converted |

**Converted is not the same as exploitable, and the entry should not blur
them.** The conversion is right for all 12 -- the helper costs nothing and
removes an abort path from an allocation whose size is not a compile-time
constant. But only some are reachable. `sys_vmsplice` caps `nr_segs` at
UIO_MAXIOV = 1024, so its buffer cannot exceed 16 KB; `sys_setgroups` caps
at NGROUPS_MAX = 65536 and `sys_futex_waitv` at FUTEX_WAITV_MAX = 128.
Those are bounds that genuinely help.

The demonstrably reachable ones are those whose ONLY gate is
`validate_user_write`, which bounds the request to what the caller has
mapped -- and therefore permits ~17 MB, which is all it takes:
`dispatch_memfd_write`, `dispatch_memfd_read`, `pread_file_to_user`,
`pread_memfd_to_user`, `sys_getdents64`, `epoll_wait_core`, and the
`poll`/`select` pair already fixed.

Plus the 7 blocks I had hand-rolled an hour earlier for poll/select --
themselves a reimplementation of this helper -- folded into it. That is 19
helper call sites in a file that had none, from two populations that must
not share a denominator: 12 of the 46 `vec!`, and 7 that were no longer
`vec!` at all. (I wrote a table conflating them first; the applier's own
assertion is what caught it.)

**`epoll_wait_core` is the one worth remembering.** Its caller looks
careful: it rejects `maxevents > EP_MAX_EVENTS` and then validates the
user range. But `EP_MAX_EVENTS` is `i32::MAX / 12`, so `maxevents * 12`
reaches 2.1 GB, and crossing the kernel's 16 MiB allocation ceiling needs
only `maxevents = 1_398_102`. **A bound that exists is not the same as a
bound that helps** -- "is it validated?" was the wrong question and
"what does the bound permit?" was the right one. Any audit that greps
for the presence of a check passes this site.

**What was NOT done, and why it is not laziness.** `read_user_vec` is the
purpose-built replacement for the `validate_user_read` + `vec!` +
`copy_from_user` dance that `linux.rs` performs at least 17 times, and
adopting it would close the class rather than the instances. But it takes a
`max` that is a **hard limit, not a truncation** -- its doc explains that
silently clamping a path turns `/very/long/path/to/a.txt` into a *different,
shorter path that may well exist*, so an over-long argument would operate on
the wrong file instead of being rejected. That makes it right for path-like
reads and wrong for `write()`, which legitimately handles large sizes:
imposing a `max` there would break large writes in the name of fixing a
panic. So each site needs a decision about what its natural bound is, or
whether it has one. **Actionable is not mechanical** -- the same conclusion
the renamed-doc-links reached this morning from the other direction.

**The remaining work, in the order it is worth doing:**

| step | why |
|---|---|
| ~~review the other `vec![0`~~ | done: 46 classified, 12 converted |
| adopt `read_user_vec` at the 10 sites that already have a bound | measured below; the `max` it wants is already in the code |
| the four wrappers still at zero | `with_user_out_buf`, `write_user_items`, `read_user_items` each encode a check `linux.rs` currently repeats by hand |

**The `read_user_vec` opportunity, measured -- and the first measurement
was keyed on the wrong axis.** Its shape is `validate_user_read(p, n)`
followed by `copy_from_user(p, ...)`, and `linux.rs` performs exactly that
**15** times. I first grouped them by whether a bound was already present
and concluded ten could adopt it immediately. Then I opened one:

```rust
const FLOCK_SIZE: usize = 32;
let mut buf = [0u8; FLOCK_SIZE];   // a STACK array
```

`read_user_vec` returns a `Vec`. Adopting it there would add a **heap
allocation for a 32-byte read** -- strictly worse than what is there. The
axis that decides adoption is not bounded-vs-unbounded, it is
heap-vs-stack, and a fixed-size read on the stack has no allocation
hazard to fix.

| group | n | verdict |
|---|---|---|
| stack array, fixed size | 4 | leave alone; adoption would be a regression |
| heap, with a bound already enforced | 6 | adopt -- `.min(4096)` x3, `RING_CAP` x2, `NODE_BITS_CAP` |
| heap, no bound | 3 | needs a decision: `dispatch_memfd_write`, `pwrite_file_from_user`, `pwrite_memfd_from_user` |
| unclear | 2 | `sys_setitimer`, `socket_sendmsg` |

So the immediately-actionable set is 6, not 10, and the three that need a
decision are all `write()`-shaped -- which may legitimately have no
natural bound at all.

**A gate is the wrong instrument here and worth saying so.** A rule of the
form "no `vec!` in syscall code" would fire on the 34 that are fine and get
switched off. What would actually have caught this is the question that found
it: *for each safe wrapper, which callers that should use it do not?* That is
a survey, not a gate, and it wants running when a wrapper is added rather
than on every build.
