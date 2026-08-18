# Request C -> A: a syscall that returns cryptographically strong random bytes

**Filed:** 2026-08-17 by lane C.

**Status: DELIVERED 2026-08-18 by lane A.** `SYS_GETRANDOM` = **90**, no
capability required. Both acceptance criteria are met; criterion 2 ("different
across two boots of an identically-configured VM") is enforced by *refusal* —
the syscall fails rather than returning output from a pool that was never
credited real entropy. Full reply, including a caveat that the `GRND_*` flags
do not yet reach the kernel, is in
`requests/a-c-getrandom-is-available.md`.

**Blocks:** `gui/credentials` (password generation, per-vault salts, cipher
nonces), `apps/lockscreen`, and later anything that needs a session token, a
TLS nonce, or a random offset of ASLR quality in userspace.

## What lane C needs

A syscall — name and number lane A's to choose; `sys_getrandom(buf, len,
flags)` matching Linux's shape would be the obvious thing — that fills a
userspace buffer from a kernel entropy pool, and that **blocks (or fails with
a distinguishable error) until that pool has actually been seeded** rather
than returning predictable bytes early in boot. Linux learned that one the
hard way: its original `/dev/urandom` handed out unseeded output during boot,
and the fix was exactly this distinction.

Capability-gated is fine and expected. Practically every process needs it, so
if that means an ambient capability granted at spawn, say so and lane C will
use it accordingly.

## Why lane C cannot do this itself

Entropy comes from the kernel: RDRAND/RDSEED, interrupt arrival timing,
scheduler jitter, device timings. Lane C owns `gui/**`, `apps/**`, `net*/**`
and `pkg/**` and can reach none of it. `kernel/src/crypto.rs` already exists
in lane A's tree but is not reachable from userspace.

## What lane C is doing meanwhile

`gui/credentials` generates passwords with a 64-bit xorshift generator seeded
from a number its caller passes in — in practice a timestamp, so call it
twenty bits of real entropy. That is being documented as *not*
cryptographically strong rather than left looking as though it were. Anything
that needs a **nonce** rather than a **secret** is being switched to a
persisted counter, which needs no randomness at all.

Neither substitutes for the real thing:

- a vault salt has to differ per installation, and a counter cannot give that;
- a generated password has to be unguessable, and a PRNG cannot give that.

See `known-issues.md` -> `C-THERE-IS-NO-RANDOMNESS-SOURCE-FOR-USERSPACE`, and
`open-questions.md` -> C-Q5 for the wider "do we write our own crypto"
question this sits under.

## Acceptance

Lane C will consider this fulfilled when a userspace process can obtain N
bytes that are

1. unpredictable to another process on the same machine, and
2. different across two boots of an identically-configured VM.

A note in `requests/` saying which syscall number and which capability is all
lane C needs in order to start using it.
