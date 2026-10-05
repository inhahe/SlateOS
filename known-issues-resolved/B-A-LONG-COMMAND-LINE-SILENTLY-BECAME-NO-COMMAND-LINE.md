## B-A-LONG-COMMAND-LINE-SILENTLY-BECAME-NO-COMMAND-LINE (lane B, 2026-09-13) — **fixed**

**In short:** a program started with more than 64 KiB of arguments plus
environment got **none of them**. Not a truncated list, not an error — `argc`
was 0 and the program ran as if invoked bare. `cat` over a long file list would
have become `cat` reading from the terminal. The kernel had always offered a way
to fix this and libc had never taken it.

### What it was

`posix/src/crt.rs::retrieve_initial_args` asks the kernel for the packed
argv/envp with `SYS_PROCESS_GET_ARGS`, into a 64 KiB static buffer. If the data
was bigger, it did this:

```rust
// If the kernel returned more than our buffer can hold, the data
// is still in the PCB (not consumed).  We can't use it without a
// larger buffer.  Fall back to no args.
if total > INIT_ARGS_BUF_SIZE || total < header_size {
    return (0, core::ptr::null(), core::ptr::null());
}
```

The comment is accurate about the mechanism and it names the consequence — and
the consequence is a silent total loss. The kernel allows 256 KiB each for argv
and envp, so the reachable gap was four to eight times the buffer.

**The libc contradicted itself, which is what decides who this bug bites.**
`sysconf(_SC_ARG_MAX)` returns `ARG_MAX`, and `posix/src/limits.rs` sets that to
**131,072 — 128 KiB**, with a test asserting the value. So the library told every
program it could pass 128 KiB of arguments, and then threw all of them away
above 64 KiB. A program that consults `sysconf(_SC_ARG_MAX)` and packs up to
the number it is given is doing the careful, correct thing; it is exactly the
program that loses its arguments, at precisely half the advertised limit, with
no error. The careless program that passes a handful of arguments never notices.

(The remaining mismatch is in the safe direction and is left alone: libc
advertises 128 KiB where the kernel accepts 256 KiB each for argv and envp, so
a caller obeying `ARG_MAX` is under the kernel's limit rather than over it.)

What makes it worse than a size limit is that **the kernel's side of the
protocol was already built**. `kernel/src/syscall/handlers.rs`:

> *"If the caller's buffer is too small, put the data back and return the
> required size so they can retry."*

It takes the args out of the PCB, notices the buffer is too small, puts them
back, and returns the size needed. Every failure path there is careful to
restore the data *specifically so a second call works*. libc never made the
second call. One half of a two-step protocol was implemented, tested and
commented, and the other half was a `return`.

### The fix

`retrieve_initial_args` now retries: when the first answer exceeds the static
buffer it `mmap`s exactly the size the kernel asked for and asks again. A raw
`mmap` rather than `malloc` because this runs before `init_environ()` and before
the ELF constructors, and startup should not depend on the allocator that early;
it is also a once-per-process call on a path almost nothing takes, so a bigger
static buffer would have charged every process for it instead.

A second answer that is still "too small", or larger than what was just
allocated, is refused rather than parsed — the second number decides how much
gets read, so trusting one we have already seen change would read past the
allocation.

### Tested on the host, which needed a seam

`SYS_PROCESS_GET_ARGS` is a stub returning `ENOSYS` on the host triple, so with
the syscall inline there is no way to reach the second ask at all. The kernel is
now behind a small `InitArgSource` trait — the same move `tee` needed for
`TeePipes` — and a fake implements the documented behaviour: keep the data,
report the size. Five tests: fits first ask (and is not asked twice), oversized
is retried and every argument survives, a failed allocation yields no args
rather than a bad pointer, a retry answer bigger than the allocation is refused,
and no-args-at-all is not an error.

Mutation-tested: restoring the original `return` fails
`oversized_args_are_retried_and_not_silently_dropped` and nothing else.

**A test-quality bug found by that mutation.** The first mutation run failed
*two* tests, the retry one and an unrelated small-args one. The second was
mutex poisoning: these tests serialise on a `Mutex` because they write
process-wide statics, and a panic while holding it makes every later
`.unwrap()` panic too. One real regression reported as several failures, with
the extra ones pointing at innocent code. The guard now ignores poisoning, and
the same mutation fails exactly one test.

### The same defect at a second limit — also fixed

`MAX_INIT_PTRS` is 512, and the pointer-array loop stopped there. A program
invoked with 600 arguments got `argc == 512`: the wrong answer, delivered with
confidence, with no way for the program to notice. `grep pat *.c` in a large
directory reaches this easily, and 512 is low enough that it is a normal
command rather than an adversarial one.

Fixed the same way, and the test was written first so it could be watched to
fail — it reported `left: 512, right: 600` before the fix. The pointer arrays
now come from the same source as the packed data when they do not fit, falling
back to the static array if that allocation fails, so the old behaviour is the
floor rather than the default. The test checks more than the count: that
`argv[599]` is really `a599` and that `argv[600]` is NULL, because an array
that is the right length but not NULL-terminated is a worse bug than the one
being fixed.

Writing the failing test first also confirmed the poisoning fix above: this
time exactly one test failed, where the earlier mutation had failed two.

### The near-miss, which is the part worth keeping

I nearly filed something much louder and wrong. Reading `_start` in `crt.rs`
shows it calling `__libc_start_main(main, 0, NULL, ...)` — argc and argv
hardcoded to zero — above a comment reading *"the kernel jumps here with no
arguments on the stack (argc/argv not yet supported)"*. The disassembly of a
staged binary matches it instruction for instruction. I had a confirmed
source-plus-binary finding that **no SlateOS program can receive arguments**,
and it was wrong: `__libc_start_main` ignores the argc/argv it was handed and
calls `retrieve_initial_args()`, which fetches them by syscall.

That is the second time in one session that a conclusion survived two forms of
evidence and still needed a mechanism I had not found yet — the first was
grepping for `[rootfs] staged` and missing the fastpy block, which says
`promoted`. The pattern is the same both times: **the evidence agreed with each
other because it was all downstream of the same wrong assumption about where to
look.** The comment in `_start` is genuinely stale and now says so, but the
behaviour it appears to describe has not been true for a long time.
