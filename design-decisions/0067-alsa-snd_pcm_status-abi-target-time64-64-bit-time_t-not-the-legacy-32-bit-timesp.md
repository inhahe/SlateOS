## 67. ALSA `snd_pcm_status` ABI target — time64 (64-bit `time_t`), not the legacy 32-bit-timespec variant

**Date:** 2026-07-15
**Decided by:** Claude (autonomous). Sub-implementation call under the ALSA
compatibility-shim roadmap item; low-risk and reversible (a translator-layer
struct/ioctl-number choice, no persistent state), so resolved directly rather
than raised to the operator.

**Context.** The ALSA PCM `STATUS`/`STATUS_EXT` ioctls return `struct
snd_pcm_status`, which — unlike `SYNC_PTR`, whose pages sit in 64-byte unions —
embeds bare `struct timespec`s directly. Its `sizeof` therefore depends on the
`time_t` width, and because the ioctl request number is `_IOR/_IOWR('A', nr,
sizeof(struct))`, the *request number itself* differs between the legacy
32-bit-`time_t` layout and the modern 64-bit (`time64`) layout. The upstream
kernel maintains two distinct structs/numbers for exactly this reason (proven
by the mainline `reserved[]` size expression going negative under a 16-byte
timespec — it cannot be a single struct). So implementing STATUS is not "pin
the timespec layout" but "decide which alsa-lib variant our userspace targets."
This ambiguity is why STATUS was deferred (known-issues TD10) while SYNC_PTR
(ABI-independent) shipped.

**Decision.** Target **time64**: 64-bit `time_t`, 16-byte `struct timespec`,
`sizeof(snd_pcm_status) == 152`, giving `STATUS = 0x8098_4120` and `STATUS_EXT
= 0xC098_4124`. Implemented in `kernel/src/audio_alsa.rs` (`SndPcmStatus` +
size/ioctl-encoding self-test) and `kernel/src/syscall/linux.rs`
(`alsa_pcm_ioctl_status`).

**Alternatives considered.**
- *Legacy 32-bit-timespec variant.* Pro: matches ancient 32-bit alsa-lib
  builds. Con: Y2038-unsafe; not what any modern 64-bit distro's alsa-lib is
  compiled against; a dead-end for a brand-new OS.
- *Implement both request numbers.* Pro: maximal compatibility. Con: doubles
  the surface for a convenience overlay; the 32-bit path is pure legacy we have
  no reason to carry. Can be added later if a real 32-bit client ever appears —
  the numbers are distinct, so adding a second arm is non-breaking.

**Reasoning.** SlateOS is a new x86_64-only OS with a 64-bit `time_t`
throughout; every other timespec-bearing syscall/ABI here is already 64-bit.
A modern 64-bit ALSA-lib (what an unmodified Linux audio client links) uses the
time64 layout, so this is the variant that actually makes `snd_pcm_status(3)` /
`snd_pcm_delay(3)` work for the shim's stated goal (unmodified Linux audio
clients). There is no realistic scenario where a 32-bit-`time_t` alsa-lib runs
on this OS. The choice is effectively forced; recorded here only because the
*existence* of the fork was non-obvious and previously blocked the work.

**Tracking.** known-issues TD10 (RESOLVED); roadmap Phase 5 ALSA shim item.
