## B-THE-LIBC-WAS-NEVER-COMPILED-FOR-THE-TARGET-IT-SHIPS-TO (lane B, 2026-09-13) -- FIXED, gate 40

Eight crates in lane B's trees pin `[build] target = "x86_64-unknown-none"` in
their own `.cargo/config.toml`. That pin is what lets them build for bare
metal, and it is also what puts the configuration that actually ships outside
every command anyone runs.

**Nothing compiled any of them on the push or boot path.**

### The one that matters

`posix` is the libc. Every one of ~280 userspace binaries links
`toolchain/sysroot/lib/libc.a`, which is built from it for bare metal. The push
hook runs `cargo test -p posix` -- on the HOST target. The kernel does not link
posix. So the only thing that ever compiled the shipping configuration was
`toolchain/build-sysroot.ps1`, run by hand.

That is worse than the six `services/*` crates, which nothing built at all,
because posix *looks* covered. It has 20,703 tests and they all pass.

**The demonstration.** One line added to `posix/src/lib.rs`:

    pub fn zzq_std_only() -> u32 { std::process::id() }

| what | result |
|---|---|
| bare metal, the shipping target | error[E0433]: cannot find module or crate std |
| the host test path the hook runs | `test result: ok. 20704 passed; 0 failed` |

A change making the libc uncompilable for the target every binary links, with
twenty thousand green tests behind it.

### The class

This is the third axis from
`B-A-VERDICT-ABOUT-DEAD-CODE-DEPENDS-ON-A-CONFIGURATION-THE-COMMAND-DID-NOT-NAME`,
arriving somewhere new: **target**. The verdict "posix is green" was true of
`x86_64-pc-windows-gnu` and unasked of `x86_64-unknown-none`, and nothing in the
sentence said which.

Worth noting how it was found, because it was not by looking for it. Closing
the untested-crates line left six `services/*` crates on the list; asking *why*
they could not be tested turned up that nothing built them either; and asking
the same question one directory over turned up posix. Each step was cheap and
none of them was the plan.

### Fixed

`scripts/check-pinned-target-build.py`, gate 40. Discovery is by the marker --
a Cargo.toml plus a cargo config with a `[build] target` -- rather than a list,
so a ninth crate is covered the day it appears. 1.9 s warm for all eight. The
self-test asserts `posix` is in the discovered set by name: if it ever silently
leaves, the libc stops being compiled for the target it ships to and nothing
says so.

It compiles; it does not test. That is the floor, and the floor was missing.
