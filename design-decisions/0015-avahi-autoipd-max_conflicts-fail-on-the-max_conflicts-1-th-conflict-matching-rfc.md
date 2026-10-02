## 15. avahi `autoipd` MAX_CONFLICTS — fail on the (MAX_CONFLICTS+1)th conflict, matching RFC 3927

**Date:** 2026-05-31

**Decided by:** Claude (autonomous) — a small implementation choice made while
bringing `userspace/avahi` (autoipd) up; easily reversible.

**Context:**
RFC 3927 §2.2.1 (IPv4 Link-Local) says a host that experiences more than
`MAX_CONFLICTS` (10) address conflicts should rate-limit / give up. The avahi
`autoipd` conflict counter previously failed when `count >= MAX_CONFLICTS`,
i.e. it tolerated only 9 retries and gave up on the 10th. The unit test encodes
the opposite intent — 10 retries tolerated, fail on the 11th — which matches the
RFC's "exceeds MAX_CONFLICTS" wording.

**Decision:** changed production to `count > MAX_CONFLICTS` to align with both the
test and the RFC, rather than relaxing the test to match the stricter code.

**Why not the alternative:** the stricter "give up at the 10th conflict" reading
is defensible (one fewer probe), but it contradicts the literal RFC wording and
the test's encoded intent; aligning to the RFC is the lower-surprise choice.

**Where it lives:** `userspace/avahi` — `AutoIpd::step`, the `Conflict` arm (one
comparison). Reverse by flipping `>` back to `>=` if the operator prefers the
strict reading.
