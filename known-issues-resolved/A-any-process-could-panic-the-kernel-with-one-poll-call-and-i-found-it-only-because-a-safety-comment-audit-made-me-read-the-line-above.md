### [A] Any process could panic the kernel with one `poll` call, and I found it only because a SAFETY-comment audit made me read the line above -- 2026-09-21
**Status:** FIXED in `a234eabc8` (7 sites); needs a boot. The measurement lesson below is the part worth keeping

**In short:** `poll()` and `select()` copy the caller's list of file
descriptors into a kernel buffer whose size the caller chooses. The kernel
allocated that buffer in a way that *crashes the whole machine* if the
allocation fails, instead of returning an error. Any ordinary program could
do it deliberately, with about 17 megabytes of memory and one system call.

**The chain, each link read rather than inferred:**

| step | fact |
|---|---|
| the size | `nfds` is a `u64` straight off the syscall; `len = nfds * 8` |
| the mitigation that exists | both callers require `validate_user_write(ptr, len)` to pass, so the attacker must genuinely have that much mapped |
| the allocation | `vec![0; len]` -- and `vec!` **aborts** on failure |
| what abort means here | `handle_alloc_error`, which in a kernel is a panic. This tree already says so in three places, e.g. `fs/sevenz.rs:214` |
| why 17 MB is enough | `large_alloc` returns null as soon as `frame::alloc_order` refuses, and it refuses any order above `MAX_ORDER = 10`. The kernel side fails at **16 MiB**, not at machine memory |

So `nfds = 2_097_153` gives `len = 16_777_224`, which rounds to order 11 and
is refused with 2.7 GB free. `select` is worse: **six** such allocations,
not the three I first counted -- `rd`/`wr`/`ex` and the three `_out` copies.

**This is the 16 MiB ceiling composing with something else.** On its own the
ceiling blocks large programs from starting, which is an inconvenience filed
as A-Q19. Combined with a bounce buffer sized by the caller it becomes a
one-syscall denial of service, because it lowers the memory an attacker needs
from gigabytes to megabytes. Neither entry could have predicted that alone.

*(2026-09-25: the ceiling is gone (§959). The sites fixed here allocate
fallibly, so none of them can abort; what changes is that a caller-sized
buffer can now reach 1 GiB instead of stopping at 16 MiB. `epoll_wait`, the
one whose size was entirely the caller's choice, now sizes by its interest
set; the rest are tracked in `A-USER-SIZED-KERNEL-BUFFERS-NOW-REACH-VMALLOC`.)*

**How it was found, which is the uncomfortable part.** Not by looking for
security bugs. I was auditing `// SAFETY:` comments, reached
`copy_from_user`, and had to read the line above the `unsafe` block to state
the invariant. That line was the `vec!`. An audit of documentation found a
remote-ish DoS because the documentation rule forces you to read the code
next to the thing you are documenting.

---

#### The measurement lesson: a passing control licenses only the failure it simulates

The audit that led here reported **318 unsafe blocks missing a SAFETY
comment, 14.4%**. The true number is **8, 0.36%**. The sequence was
318 -> 24 -> 21 -> 8, and each cut came from a different flaw in my own
instrument:

| reported | the flaw |
|---|---|
| 318 | the scan only looked at the line *immediately* above. `madt.rs` has `// SAFETY: entry is at least 8 bytes.` governing a group of reads, separated from the first by one **safe** statement |
| 24 | it counted `unsafe {` inside ```` ```ignore ```` examples in `//!` doc comments -- not code |
| 21 | it claimed to search "the enclosing function" but capped at 40 lines. `pt_walk.rs:175` carries a comment covering reads **48 lines** below it |

**I ran a control before publishing 318, and that is what makes this worth
recording.** My scan could not see a SAFETY note written as the first line
*inside* a block, so I checked: it found 4. The number barely moved, and I
concluded the instrument was sound -- I even wrote down "the 318 are
absences, not variations."

The control tested placement **after** the block. Every real failure was
placement **before but separated**. A control only covers the axis it
varies, and a passing one is far more persuasive than no control at all:
**it licenses exactly the failure it simulates, and the confidence it
produces is general.** That asymmetry is the defect, not the regex.

The cheap version of the right control: plant the defect. Had I inserted a
SAFETY comment two lines up with a statement between, the scan would have
reported it missing and I would have known in thirty seconds. Lane C reached
the same place from the other side the same hour -- their survey's zero
rested on one planted defect that varied one axis, and varying a second
(a field held singly vs. in a `Vec`) turned a silent case into a reported
one.

**Every count I produced today shrank when examined**: 47 qualifiable doc
links became 11, 64 demotable became 2, 9 unbounded allocations became 3,
318 missing SAFETY comments became 8. The habit that caught all four was the
same and it is not sophisticated: read the scan's own output before
believing its total.
