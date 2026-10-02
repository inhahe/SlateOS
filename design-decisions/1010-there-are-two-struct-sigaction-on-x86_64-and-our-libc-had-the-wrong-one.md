## 1010. There are two `struct sigaction` on x86_64 and our libc had the wrong one

**Date:** 2026-09-09
**Lane:** B
**Decided by:** Claude (autonomous)

**In short:** the structure a C program fills in to install a signal handler
comes in two different field orders on this machine -- one the kernel uses, one
the C library uses. They are the same total size, so nothing caught the
difference by measuring it. Our C library used the kernel's, under a comment
saying it was the C library's. The one field that happens to sit in the same
place in both is the handler itself, which is the field every test in the tree
exercises -- so signals worked, and everything else a program asked for was
quietly read out of the wrong bytes.

### The two layouts

| | offsets |
|---|---|
| kernel (`rt_sigaction`) | handler 0, **flags 8**, restorer 16, **mask 24** |
| glibc **and** musl | handler 0, **mask 8**, **flags 136**, restorer 144 |

Both are 152 bytes.

Measured, not recalled. `offsetof` printed under glibc on Linux 6.6:

```text
glibc struct sigaction: size=152 handler=0 mask=8 flags=136 restorer=144
```

and the same numbers asserted at compile time against musl with
`zig cc --target=x86_64-linux-musl`, which is the toolchain every C port in
this tree is built with. `_Static_assert` also pins `sa_flags` at **4** bytes:
it is `int`, with four bytes of padding after it, not `unsigned long`.

### What the wrong layout actually did

* `sa_flags` was read from bytes 8..12 -- the first word of the caller's
  `sa_mask`, which `sigemptyset` has just zeroed. **Every flag a C program
  passed was silently dropped** and read back as zero.
* `sa_mask` was read from byte 24, sixteen bytes into the caller's mask. An
  empty mask is still empty when shifted, so the common case survived; a
  non-empty one blocked the wrong signals.
* `oldact` was written in the wrong order too, so a caller reading back its own
  flags got bytes out of the middle of its own mask.
* `sa_handler` is at offset 0 in both, so handlers ran.

### Why it survived, which is the part worth keeping

**Every consequence was invisible until today.** No `sa_flag` was implemented:
`SA_RESTART` is not honoured, `SA_SIGINFO` is not honoured, `SA_RESETHAND` and
`SA_NODEFER` are read from our own table by Rust code that never crossed the
ABI. A flag that is dropped and then never consulted is indistinguishable from
a flag that is stored. `SA_ONSTACK`, added hours earlier in 1009, is the first
one a C program can set and observe -- and it is the reason this surfaced.

**No Rust test could have caught it.** Every test in `posix/src/signal.rs`
builds a `Sigaction` with named fields, which is layout-independent: Rust
agrees with itself whichever order the struct declares. The `#[repr(C)]`
layout only matters at a boundary with C, and the crate's tests have no such
boundary. This is the same reason the `long double` family of fixtures exists
in plain C -- `services/ctest-longdouble/` says it outright: "both faults are
invisible to the posix crate's own unit tests, because there Rust calls Rust
and the two sides agree on the same wrong convention."

**There was a test, and it asserted the wrong thing confidently.** It pinned
`offset_of!(Sigaction, sa_flags) == 8` under a comment reading "glibc x86_64".
A test that encodes a mistaken premise does not merely fail to catch the bug;
it certifies it, and it makes the next reader confident too.

### The shape of the error, stated plainly

Two structures with the same name, the same size, and different orders, where
the field every test touches is the field they agree on. Nothing about the
symptom points at the cause: handlers work, so signal handling looks fine.

This is the third time in one session that a confident claim about upstream
turned out to be about a different layer or a different artifact than the one
being asked about -- the operator's README against the operator's program, the
`do_sigaltstack` internals against the `sigaltstack` ABI, and now the kernel
struct against the libc struct. The correction is identical every time and it
is cheap: compile or run the thing, do not remember it.

### What was changed

`Sigaction` is reordered to `handler, mask, flags, restorer` with `sa_flags` as
`u32`, and the `SA_*` constants become `u32` to match the C declaration's
`int`. The width matters as much as the offset: a `u64` field would land on the
same offsets by absorbing the padding, and would read four bytes that belong to
the caller and hold whatever was there before.

`services/ctest-altstack/` gains a round-trip check -- install a flag and a
mask through `sigaction`, read them back through `sigaction(sig, NULL, &old)`,
and assert both survive. A flag that does not survive a round trip is this bug
and nothing else. It is in C for the reason above.

`stack_t` was checked at the same time, since 1009 rests on it: musl agrees
with ours at `ss_sp` 0, `ss_flags` 8, `ss_size` 16, size 24.

**Against the change, honestly:** it is an ABI break for anything already
compiled against the old layout. In practice that is every C binary in the
image, all of which are rebuilt from source by the same build that produces the
libc, and none of which could have been relying on the old behaviour -- because
the old behaviour was that their flags were discarded. The fixtures were
relinked in the same commit.
