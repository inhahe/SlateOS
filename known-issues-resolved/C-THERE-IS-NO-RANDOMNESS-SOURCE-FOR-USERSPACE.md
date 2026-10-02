## C-THERE-IS-NO-RANDOMNESS-SOURCE-FOR-USERSPACE (lane C, 2026-08-17) — **FIXED 2026-08-18; and the "where the fix has to come from" paragraph below was wrong when it was written**

**In short:** nothing in lane C can obtain an unpredictable number. The
credential manager's password *generator* — the feature whose entire job is
producing something an attacker cannot guess — runs a 3-line xorshift
generator seeded by a number its caller passes in, in practice a timestamp.
An attacker who knows roughly when a password was generated can enumerate the
possibilities.

**Where:** `gui/credentials/src/main.rs`, `Xorshift64` and
`generate_password(.., seed: u64)`.

`Xorshift64` is a perfectly good *statistical* generator and a useless
*cryptographic* one: its entire future output is determined by 64 bits of
state, and recovering that state from output is trivial. Seeded from a
second-resolution timestamp, the real entropy is closer to 20 bits.

**Where the fix has to come from:** the kernel. A userspace CSPRNG needs a
seed from an entropy pool the kernel maintains (RDRAND/RDSEED, interrupt
timing, etc.), surfaced through a syscall. `kernel/src/crypto.rs` exists
(lane A) but lane C has no interface to it. Filed as
`requests/c-a-userspace-entropy-syscall.md`.

Until then the honest thing is for `generate_password` to *say* it is not
cryptographically strong rather than to look like it is, and for anything that
needs a real nonce to use a persisted counter instead (see the keystream
entry above).

### The syscall already existed. Nobody grepped. (2026-08-18)

The paragraph above — "the fix has to come from the kernel", "lane C has no
interface to it" — was **false on the day it was written**. `SYS_GETRANDOM`
(number 90) has been in `kernel/src/syscall/handlers.rs` all along, is *not*
capability-gated, validates the user pointer before it generates, and is
backed by `kernel/src/rng.rs`: ChaCha20 seeded from RDRAND/RDSEED and
interrupt timing. Exactly the thing the entry says does not exist.

Worse, **lane C was already calling it** from `userspace/ssh-keygen` while
this entry and `requests/c-a-userspace-entropy-syscall.md` were being
written. The request was filed against an assumption instead of a grep, and
then this entry cited the request as evidence for the assumption. Two
documents agreeing with each other is not corroboration when one is the
other's source.

The real blocker was smaller and entirely inside lane C: the `SystemRandom`
wrapper lived in `guitk::rng`, and `gui/credentials` (a headless service) does
not and should not depend on a GUI toolkit. Merging the two RNG crates into
`randrange` — which `gui/credentials` already depended on — removed it. See
`C-THE-SHARED-RNG-CRATE-WAS-ITSELF-DUPLICATED` and design-decisions §463.

**What is actually fixed:**

- `Xorshift64` and `generate_password(.., seed: u64)` are gone.
  `generate_password` draws from the kernel and returns `Option<String>`,
  refusing rather than falling back (`NO_ENTROPY_MESSAGE`). Design-decisions
  §462 covers the fail-closed rule; §464 covers the salt.
- Per-vault salts, above.
- The fail-closed wrapper itself is no longer copied per crate: it is
  `randrange::SecretSource`, which checks the source's health on *both* sides
  of a draw, because `SystemRandom` refills from the kernel mid-draw and a
  password whose tail is zeroes looks fine to the user.

**The lesson, which is the same one as the LCG sweep:** grep before you file.
A request that asks another lane for something that already exists costs that
lane a context-switch and costs this one a day of believing itself blocked.

~~**One genuine gap remains, and it is not a blocker.** `kernel/src/rng.rs`
tracks `seeded: bool` internally, but `sys_getrandom` does not appear to
surface it, so a caller that runs before the pool is seeded cannot tell.~~
Noted at the end of `requests/c-a-userspace-entropy-syscall.md`; it matters
only for very-early-boot callers, which lane C currently has none of.

### Lane A answered, and closed that gap by making the call fallible (2026-08-18)

`requests/a-c-getrandom-is-available.md`. The gap above is resolved in the
strongest available way — **not** by surfacing a "seeded" flag for callers to
check, but by making it impossible to read from an unseeded pool. Both halves
of what lane C asked for now hold:

| | |
|---|---|
| **Syscall** | `SYS_GETRANDOM` = 90, **no capability** — deliberately ungated |
| Unpredictable to another process | ChaCha20, 256-bit key never leaving the kernel |
| Different across two boots of one VM image | **now true; it was not before** |

The second criterion is the one that needed work, and lane C's guess about
*why* was right: the old pool was keyed from HPET reads, TSC jitter and APIC
tick counts, every one of which correlates across two boots of an identical
image. The kernel now separates **credited** entropy from merely *keyed* —
only RDSEED/RDRAND and interrupt-arrival timing earn credit, clock reads earn
none — and `getrandom` blocks until 256 bits are credited.

There is no capability to hold. Lane C had offered to accept an ambient one;
lane A declined on the grounds that a capability every process receives at
spawn is ambient authority with extra bookkeeping, and that withholding
randomness protects nothing — a process denied it does not stop needing
unpredictable bytes, it just makes worse ones itself. Which is precisely what
the sixteen hand-rolled LCGs in this lane were.

**What this changes for lane C code: `getrandom` can now fail.** It waits at
most 15 s, then returns an error surfacing as `EIO`. That is a new failure
mode on a call that previously always succeeded.

It should be unreachable here — credit accrues from the 100 Hz timer, and lane
A measured the pool ready **330 ms after interrupts are enabled**, long before
any GUI process exists. But the handling must still be right, and the two
tiers in design-decisions §465 handle it in opposite directions **on purpose**:

- **Secrets propagate the failure and never fall back.** Lane A asked for this
  explicitly; `SecretSource::secret` already does it. A vault that refuses to
  create a salt is recoverable; one created with a predictable salt is not,
  and nothing later can detect it. Note the new failure is a *refusal*, not
  weak bytes — the kernel never hands over material it cannot vouch for, so
  there is no "check the strength afterwards" case to get wrong.
- **Novelty falls back, and that stays correct.** `seeded_from_system` taking
  its per-crate fallback on `EIO` is intended behaviour, not a hole. A game
  that will not start is worse than a predictable one.

**One caveat that would bite a call site that must not block.** On our native
ABI the `GRND_*` flags are accepted by libc, validated, and then *discarded* —
they never reach the kernel, because `posix/src/random.rs` reaches it through
a two-argument stub. So `GRND_NONBLOCK` does not actually prevent blocking.
Lane B is tracking the ABI fix
(`requests/a-b-getrandom-now-waits-for-a-credited-pool.md`). Lane C has no
such call site — every draw is at app startup or during a deliberate user
action — so this is noted, not escalated. Lane A offered to prioritise the ABI
change if one appears.

**Nothing left to action.** The request asks lane C to drop the xorshift in
`gui/credentials` and close this entry; both happened during the `randrange`
merge, before the answer arrived. Only a historical comment at
`gui/credentials/src/main.rs:43` still names it. C-Q5 ("do we write our own
crypto, or port a vetted implementation") is untouched by this and still
stands — an entropy source is not a crypto library.
