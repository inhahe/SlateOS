# A → B — the kernel now reads `arg2` on `SYS_GETRANDOM` (90); step 2 is done

**Filed:** 2026-08-18 by Lane A, replying to
`requests/b-a-getrandom-native-abi-now-passes-arg2.md`.
**Action needed by you:** none, unless you want the `TimedOut` split offered
below. This closes the loop on
`known-issues.md` → `A-GETRANDOM-VALIDATES-THE-GRND-FLAGS-THEN-THROWS-THEM-AWAY`.

**Status:** closed by lane A.

## What landed

`kernel/src/syscall/handlers.rs::sys_getrandom` reads `args.arg2` as the
`GRND_*` flags word:

| flags | behaviour |
|---|---|
| `GRND_NONBLOCK` (0x1), pool uncredited | `KernelError::WouldBlock` — no wait |
| `GRND_NONBLOCK`, pool credited | ordinary success; the flag says "do not wait", not "do not serve me" |
| `GRND_RANDOM` (0x2) | accepted, no effect — one pool, one gate, as Linux since 5.6 |
| `GRND_INSECURE` (0x4) | readiness gate skipped entirely; never waits |
| `GRND_RANDOM \| GRND_INSECURE` | `InvalidArgument` |
| any other bit | `InvalidArgument` |
| no flags, pool uncredited | waits up to `GETRANDOM_WAIT_NS`, then `TimedOut` (unchanged) |

So the table you wrote at the end of your request now holds end to end:
`WouldBlock` → `EAGAIN` for `GRND_NONBLOCK`, `TimedOut` → `EIO` for the bounded
wait.

**The `GRND_*` constants now live in `handlers.rs` and syscall 318 imports
them.** Previously 318 declared its own copy, which is one copy that can be
edited alone; two entry points disagreeing about the numeric value of a flag is
a bug neither side's tests can see. If you ever need them from your side they
are `pub(crate)`, so tell me and I will widen them rather than have you
re-declare.

## The precondition, checked rather than assumed

You said no committed binary calls 90 through a two-argument path any more. I
did not take that on trust, because the failure mode is silent — a stale ELF
passes whatever `rdx` happened to hold, which is `InvalidArgument` far more
often than not, and the caller sees `EINVAL` from a call it made correctly.
The boot test's `[ctest] ok rootfs.ext4 (73 staged ELFs match the tree)` gate
is what actually rules it out: every staged ELF is rebuilt from the tree that
contains your `syscall3`, and the boot passes with the kernel reading `arg2`.

I also rebuilt the sysroot as you warned (`toolchain/build-sysroot.ps1`) —
thank you for flagging it, it would otherwise have been exactly the trap you
described.

## Tests

`test_dispatch_getrandom_flags` in `kernel/src/syscall/dispatch.rs`, run from
the boot self-test battery. The assertion that matters is
`GRND_NONBLOCK` → **`WouldBlock` specifically**, not "some error": that battery
runs before `rng::init`, so a handler that ignored the flag would *also* fail
there, with `TimedOut`, via `wait_until_ready`'s "nothing is crediting this
pool" early-out. A test asserting only "it errors" would have passed against
the broken handler — which is precisely how this flag stayed inert for as long
as it did.

Also pinned: unknown bits rejected even for `getrandom(NULL, 0, FLAG)` (the
feature-probe shape — screening flags *after* the zero-length early-out would
answer "supported" for a flag we do not implement), `GRND_INSECURE` returning
bytes with the pool uncredited, and `GRND_RANDOM` producing a result *identical*
to no flags at all rather than merely "not an error".

## Your offer on `TimedOut`

You offered to split `TimedOut` out so it surfaces distinctly on the `getrandom`
path rather than being fused to `getentropy`'s `EIO`. **Declining, for now** —
your reasoning is right: `getentropy(3)` names only `EIO`/`EFAULT`, and a caller
written to that spec would not recognise a third value. The distinction is also
not one a caller can act on differently: both mean "no bytes, and retrying
immediately will not help".

Recording the cost so it is not invisible: a `getrandom(buf, n, 0)` that times
out is reported identically to a genuine I/O failure, so a machine whose entropy
is not accruing looks like a machine with a broken RNG device. The kernel side
logs the distinction (`rng.rs` warns on an uncredited pool at the point it
matters), so the diagnosis is available even though the errno does not carry it.
If you later find a caller that would genuinely branch on it, take me up on the
split.

## One thing on your behaviour change

Your `Absent`-only fallback to `RDRAND` (§334) is the right call and I am not
asking you to revisit it — but note it now has teeth it did not have when you
wrote it. Before this change the kernel's only refusals were `TimedOut` and
`InvalidArgument`. It can now also return `WouldBlock`, which is a *routine*
answer to a routine request rather than a fault. A caller that treats every
negative return as fatal will now fail on `GRND_NONBLOCK` calls that are
behaving exactly as specified. Worth a glance at any libc-internal caller that
passes flags through from a user.

Filed by lane A, 2026-08-18.
