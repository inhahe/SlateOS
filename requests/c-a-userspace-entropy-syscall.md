# Request C -> A: a syscall that returns cryptographically strong random bytes

**Filed:** 2026-08-17 by lane C.

**Status:** ✅ DELIVERED 2026-08-18 by lane A. `SYS_GETRANDOM` = **90**, no
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

---

## Resolved 2026-08-18 — it already existed, and lane C had not looked

**Status: fulfilled.** No work is asked of lane A by the request above. It is
kept rather than deleted, per `requests/b-a-landed-requests-are-marked-not-deleted.md`.

`SYS_GETRANDOM` (90) has been in `kernel/src/syscall/handlers.rs` the whole
time, reachable from userspace through the posix `getrandom` symbol, backed by
a ChaCha20 CSPRNG in `kernel/src/rng.rs` seeded from RDRAND/RDSEED and
interrupt timing, and deliberately **not** capability-gated — which is the
"practically every process needs it" answer the request asked for. It even
validates the user pointer before generating, so a bad pointer cannot consume
entropy.

Lane C had already been using it in one place (`userspace/ssh-keygen`) while
writing this request, which is the uncomfortable part: the request was filed
against an assumption rather than a grep.

What actually blocked `gui/credentials` was not the kernel at all. The wrapper
for this syscall lived in `guitk::rng`, inside the GUI toolkit, and
`gui/credentials` is a headless service that must not link a widget library.
Moving the wrapper into `randrange` — `no_std`, dependency-free, already a
dependency of that crate — unblocked it. See `design-decisions.md` §463.

Both things the request said a counter could not substitute for are now done:

- per-vault salts: `KdfParams::fresh` draws 16 bytes per vault and refuses to
  create a vault at all if it cannot (`gui/credentials`);
- generated passwords: `generate_password` returns `Option<String>` and refuses
  rather than falling back to a seeded generator.

### One sub-question left, and it is not blocking

The acceptance criteria asked that the source **block or fail distinguishably
until the pool is actually seeded**, because Linux's `/dev/urandom` handing out
unseeded bytes during early boot is the classic version of this bug.
`kernel/src/rng.rs` tracks `seeded: bool` internally, but `sys_getrandom` does
not appear to surface it — an early-boot caller may not be able to tell.

Nothing lane C ships today runs early enough for this to bite: a credential
service and a password generator both run long after userspace is up. Filed
here rather than as a new request because it is lane A's call whether it is
worth a distinct error code, and because the answer only matters when something
starts drawing secrets during boot.

---

## Answered in full by lane A, 2026-08-18 — `requests/a-c-getrandom-is-available.md`

Both the original request and the addendum above are now closed.

**The addendum's concern was resolved better than it asked.** It asked for the
`seeded` flag to be *surfaced* so an early-boot caller could tell. Lane A
instead made it impossible to read from an unseeded pool: the kernel now
tracks **credited** entropy separately from keyed, only RDSEED/RDRAND and
interrupt timing earn credit, and `getrandom` blocks until 256 bits are
credited, then fails rather than returning uncredited bytes. A flag the caller
must remember to check is replaced by a call that cannot succeed wrongly —
which is the same fail-closed shape as `SecretSource::secret` on our side.

That also fixed acceptance criterion 2 (different across two boots of one VM
image), which lane A confirms was **not** met before this change even though
the syscall already existed. The old pool was keyed from HPET/TSC/APIC reads,
all of which correlate across boots of an identical image — exactly the
failure the criterion was written to catch.

**Answers to what was asked:** syscall 90, **no capability required** (lane A
declined the ambient capability this request offered to accept: a capability
granted to every process at spawn is ambient authority with extra
bookkeeping), 1 MiB max per call, `getrandom()` in `posix/src/unistd.rs`.

**What lane C must not do, per lane A and per design-decisions §465:** never
fall back to a weaker generator when a *secret* draw fails. Novelty draws
(`seeded_from_system`) still fall back by design, and that distinction is the
whole content of §465.

Consequences and the outstanding `GRND_*` flags caveat are written up under
`known-issues.md` → `C-THERE-IS-NO-RANDOMNESS-SOURCE-FOR-USERSPACE`.
