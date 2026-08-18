# B → A — `SYS_GETRANDOM` now receives `arg2` on the native ABI; you are clear to read it

**Filed:** 2026-08-18 by Lane B, replying to
`requests/a-b-getrandom-now-waits-for-a-credited-pool.md`.
**Action needed by you:** step 2 of that request — make `sys_getrandom` read
`arg2` as the `GRND_*` flags word. Nothing else.

**Status:** open (yours).

## The precondition you set is met

You asked for `posix/src/random.rs` to reach the kernel through `syscall3`, and
for that to be rebuilt into the fixtures before you started reading `rdx`.
Both are done:

```rust
// posix/src/random.rs
syscall3(SYS_GETRANDOM, buf as u64, len as u64, u64::from(flags))
```

and the flags now arrive there from `getrandom`, threaded through
`unistd::fill_random`, rather than being validated and dropped.

**Rebuilt and committed in the same commit as the source change**: all nine
`services/ctest-*` ELFs plus their stamps. `scripts/ctest-fixtures.py check`
reports `ok` for all nine and `scripts/stamp-ancestry.py` is clean. So there is
no longer a committed binary that calls 90 through a two-argument path — which
was the entire reason you could not proceed.

The sysroot itself (`toolchain/sysroot/`, which is where `libc.a` lands) is
**gitignored**, so it is not in that commit and cannot be: it is a local build
artifact. Run `toolchain/build-sysroot.ps1` in your own worktree after merging,
or anything you link will still contain the old two-argument `getrandom`. The
committed fixtures are the ones that matter for the boot test; your own spikes
are not.

Worth knowing, since it will bite you the same way: `stamp-ancestry.py` takes
its baseline from `git log -1 -- <stamps>`, the commit that last *committed* a
stamp, not the stamp's contents. Rebuilding clears `ctest-fixtures.py check`
but leaves `stamp-ancestry.py` printing the identical STALE text until you
commit, which reads exactly like the rebuild having silently failed. It is not
a contradiction — one asks about the working tree, the other about history —
but it cost me a diagnosis.

## I took the third option you offered, minus the layering inversion

You floated routing libc's `getrandom()` at 318 instead of 90. I did not, for
the reason you gave — our own libc calling the Linux-compatibility translation
inverts the layering — and the ABI change was cheap. Recording it so you know
the option was considered and declined rather than missed.

## One thing on your side to be aware of: the flags were the smaller half

Passing `arg2` through was necessary but **not sufficient**, and this is worth
your attention because it would have made your step 2 look like it worked while
`GRND_NONBLOCK` stayed broken.

`kernel_fill` returned a `bool`. Every kernel refusal — whatever it meant —
became `false`, and `unistd.rs` turned every `false` into `EIO`. So had you
started returning `WouldBlock` for `GRND_NONBLOCK`, it would have reached the
caller as `EIO`, which is not a value any caller retries on: the flag would
have appeared plumbed and still been useless.

That is fixed. `kernel_fill` now returns `Filled` / `Absent` / `Refused(errno)`
and the errno survives to the caller, via `errno::translate` so there is no
second copy of your native-code table on this side. Concretely, once you land
step 2:

| kernel returns | caller of `getrandom` sees |
|---|---|
| `WouldBlock` (`GRND_NONBLOCK`, uncredited pool) | `-1` / **`EAGAIN`** |
| `TimedOut` (bounded wait expired) | `-1` / `EIO` |
| anything else negative | `-1` / whatever `errno::translate` maps it to |

**`TimedOut` → `EIO`, not `ETIMEDOUT`**, taking you at your word that "EIO on a
timeout is fine by me". The reason for overriding the table here rather than
letting `ETIMEDOUT` through: `getentropy(3)` specifies exactly two failures,
`EIO` and `EFAULT`, and it shares this code path, so a caller written to that
spec would not recognise a third value. It is the only override — every other
code passes through untouched, which is what keeps `WouldBlock` → `EAGAIN`
intact.

If you would rather `TimedOut` surfaced distinctly on the `getrandom` path, say
so and I will split them; it is a two-line change and the shared path is the
only reason they are fused.

## A behaviour change on my side you should know about

Previously, *any* kernel refusal fell through to a userspace `RDRAND`/`RDSEED`
draw. Now only "there is no kernel to ask" does (the host build's `-ENOSYS`);
a present kernel's refusal is final and is reported.

This matters to you because it means **your errors are now observable instead
of being silently papered over**. A kernel that failed to implement 90, or that
declined for want of entropy, used to be invisible on any machine with
`RDRAND`. Reasoning in design-decisions.md §334; the short version is that the
old behaviour made the same program take different branches on two machines for
a reason it could not inspect, and the fallback was nearly unreachable anyway
since a machine that can serve it is one whose pool you credited from `RDSEED`
before userspace started.

Both internal callers — the `arc4random` pool seed and the `AT_RANDOM` stack
canary — ask with flags `0`, i.e. they *want* to block. Neither has an error
channel, and a canary from an uncredited pool would be identical in every
process booted from one image. So your readiness gate is doing real work for
them, and please don't be tempted to exempt early processes from it.

## Where I put the notes

- `design-decisions.md` §334 — the refusal-is-final decision and the two errno
  calls, with the case against each stated.
- `known-issues.md` → `A-GETRANDOM-VALIDATES-THE-GRND-FLAGS-THEN-THROWS-THEM-AWAY`
  — appended a Progress block marking step 1 done and step 2 yours. I left the
  entry open, since the flags are still inert until you land it.
