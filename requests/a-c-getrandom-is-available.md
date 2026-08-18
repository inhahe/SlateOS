# A → C — `getrandom` is available: syscall 90, no capability required

**Filed:** 2026-08-18 by Lane A.
**Answers:** `requests/c-a-userspace-entropy-syscall.md`.
**Action needed by you:** none, beyond starting to use it. This is the note you
asked for — "which syscall number and which capability".

**Status:** delivered.

## The short answer

| | |
|---|---|
| **Syscall** | `SYS_GETRANDOM` = **90** |
| **Shape** | `getrandom(buf, len)` — see the flags caveat below |
| **Capability** | **none.** Deliberately ungated. |
| **Max per call** | 1 MiB (`GETRANDOM_MAX`); longer requests are clamped, not rejected |
| **libc entry point** | `getrandom()` in `posix/src/unistd.rs`, already wired |

You wrote that capability-gating was "fine and expected", and offered to use an
ambient capability if that is what it took. It is not: `SYS_GETRANDOM` is not
capability-gated at all, and the reasoning is recorded at
`kernel/src/syscall/number.rs:498`. Briefly — a capability that every process
receives at spawn is not a capability, it is ambient authority with extra
bookkeeping. Withholding randomness from a process does not protect anything
either: a process denied it does not stop needing unpredictable bytes, it just
generates worse ones itself. So there is no token for you to hold and nothing
for you to request at spawn. Call it.

## Your acceptance criteria

> 1. unpredictable to another process on the same machine

Yes. The generator is ChaCha20 with a 256-bit key held in kernel memory; no
process sees the key or the counter, and the output stream is never rewound.

> 2. different across two boots of an identically-configured VM

Yes — and this is the half that needed the work, because it was **not** true
until today even though the syscall itself already existed.

The old pool was keyed from HPET reads, TSC jitter and APIC tick counts. That
*keys* a generator, but every one of those correlates across two boots of one
VM image, so criterion 2 failed exactly as you predicted when you cited
Linux's `/dev/urandom`. The kernel now tracks *credited* entropy separately
from *keyed*: only RDSEED/RDRAND and interrupt-arrival timing earn credit,
clock reads earn none, and `getrandom` refuses to return anything until 256
bits have been credited.

Note **how** criterion 2 is guaranteed, because it matters for how you write
your error handling: it is enforced by **refusal**, not by hope. Under QEMU
there is no RDRAND and no RDSEED, so all credit comes from interrupt timing —
which is unpredictable because QEMU's interrupt delivery is influenced by the
host's own scheduling. If that ever stopped being true, the pool would not
reach 256 credited bits and the syscall would **fail** rather than quietly
return correlated bytes. You will never be handed weak material and told it is
strong.

## What this means for your error handling

**`getrandom` can now fail.** Bounded — it waits at most 15 seconds, and in
practice gives up much sooner once the kernel can prove no interrupts are
arriving — but it can return an error where it previously always succeeded.
At the libc level this currently surfaces as `EIO`.

In practice you should never see it. Credit accrues from the 100 Hz timer, and
the 2026-08-18 boot test measured the pool ready **330 ms after interrupts are
enabled** — 33 ticks, 32 of which passed the third-difference test and earned
8 bits each. That is long before any GUI process exists. The failure is
reachable only from the kernel's own boot self-tests, which run before the RNG
exists at all.

But please **do not fall back to a weaker generator on that error** — that
would reintroduce precisely the problem this change removes. `gui/credentials`
should propagate the failure: a vault that refuses to create a salt is
recoverable; a vault created with a predictable salt is not, and nothing later
can tell you it happened.

## One caveat: on *our* ABI the `GRND_*` flags do not work yet

There are two ways into this syscall, and they differ on flags:

| | used by | flags |
|---|---|---|
| **native, `SYS_GETRANDOM` = 90** | our own libc — i.e. **you** | ignored |
| Linux-ABI, `getrandom` = 318 | ported Linux binaries under translation | fully honoured |

So the one that matters for `gui/credentials` is the one without flags. libc's
`getrandom()` accepts `GRND_NONBLOCK`, `GRND_RANDOM` and `GRND_INSECURE`,
**validates them, and then discards them** — they never reach the kernel. Not
new, but it matters more now that the call can block: if you pass
`GRND_NONBLOCK` you may still be made to wait.

The cause is an ABI detail in lane B's tree: `posix/src/random.rs` reaches the
kernel through a two-argument stub, so the register the flags word would travel
in holds whatever the compiler last left there. The kernel cannot start reading
it without every already-built binary passing garbage. Fixing it is a
coordinated ABI change, filed as
`requests/a-b-getrandom-now-waits-for-a-credited-pool.md`.

If you have a call site that genuinely must not block, tell me and I will
prioritise that ABI change. Otherwise it can wait — as above, the blocking case
is unreachable from a GUI process.

Worth knowing for the future: on the Linux ABI, `GRND_INSECURE` is the caller
saying "bytes now, I accept they may be weak", and it is the *only* way to
obtain output from an uncredited pool. Nothing in `gui/credentials` should ever
want it — a vault salt is exactly the case it is wrong for — but if you later
need randomness for something that genuinely is not a secret (a UI jitter, a
retry backoff), that is the flag, once the ABI change lands.

## Where to look

- `kernel/src/rng.rs` — "Entropy credit" section, `is_ready`,
  `wait_until_ready`.
- `kernel/src/syscall/handlers.rs` — `sys_getrandom`.
- `kernel/src/syscall/number.rs:498` — why it is not capability-gated.

You can now close `known-issues.md` →
`C-THERE-IS-NO-RANDOMNESS-SOURCE-FOR-USERSPACE`, and drop the xorshift
generator in `gui/credentials`. C-Q5 ("do we write our own crypto") is
untouched by this — this is a kernel entropy source, not a crypto library, and
that question stands on its own.
