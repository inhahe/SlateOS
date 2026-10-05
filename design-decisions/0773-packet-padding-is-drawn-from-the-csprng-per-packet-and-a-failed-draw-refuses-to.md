## 773. Packet padding is drawn from the CSPRNG per packet, and a failed draw refuses to send

**Date:** 2026-09-05
**Lane:** B
**Decided by:** Claude (autonomous)

**In short:** every SSH packet is topped up with a few filler bytes so its total
length lands on a multiple of the cipher's block size. Ours were all zero. The
standard says they should be random, so they now are — drawn fresh for each
packet. The part that is a judgement call is what to do when the system cannot
produce random bytes at all: we refuse to send the packet, which ends the
connection, rather than quietly going back to zeros.

**Where:** `SshSession::send_packet` in `userspace/ssh/src/main.rs` and
`ConnectionState::send_packet` in `userspace/sshd/src/main.rs`. The padding is a
parameter of `sshwire::PacketCodec::encode`, and `padding_len` says how much is
wanted, precisely so that this decision is made in the two programs rather than
in the crate they share.

### The tradeoff, stated honestly

The uncomfortable part is that **with the cipher we actually run, predictable
padding is harmless.** AES-CTR XORs the plaintext with a keystream that never
repeats (`sshwire::Aes128Ctr`, and see
`TD-B-THE-AES-CTR-COUNTER-IS-REUSED-BY-THE-CLIENT-AND-INVENTED-BY-THE-SERVER`
for the period when it did). Known plaintext therefore reveals keystream at byte
positions that will never come round again. So on today's cipher, refusing to
send is a pure availability loss for no confidentiality gain, and "send zeros
and stay up" is the option that serves the user better *right now*.

Three things outweigh that:

1. **The padding rule outlives the cipher choice.** §6's SHOULD is written for
   the general case, and it is the CBC modes where a known trailing block is
   directly useful to an attacker. If this daemon ever gains a second cipher —
   and a daemon that cannot negotiate one is a daemon nobody can connect to
   from a policy-constrained client — the padding policy would need to be
   revisited in the same breath, by someone who knew to. Making it correct once,
   at the framing layer, is what stops that being a thing to remember.
2. **A system that cannot produce eight random bytes cannot produce a session
   key either.** The same `randrange::fill_secret` supplies the host key seed
   and the Diffie-Hellman exponent, both of which already refuse rather than
   fall back (`generate_dh_private`, both ends). A padding draw failing means
   entropy has gone away *mid-session*, which says nothing good about the keys
   the session is already using. Continuing on the grounds that this particular
   draw was not important is reasoning from the least important consumer.
3. **Fail-open is the shape of the bug we just removed.** The server's MAC check
   read "if the tag is too short, skip verification" — the same instinct,
   applied where it was fatal (see
   `TD-B-THE-SERVERS-PACKET-DECODER-TRUSTED-A-LENGTH-IT-HAD-NOT-CHECKED-AND-A-MAC-IT-HAD-NOT-VERIFIED`).
   A codebase that answers "carry on degraded" in the cheap case teaches itself
   to answer it in the expensive one.

**The alternative not taken:** buffer entropy in a long-lived
`randrange::SystemRandom` held by the session, which amortises the syscall over
256 bytes and would make the failure sticky rather than per-packet. Rejected for
now on two grounds: `SystemRandom` exposes only `u64` draws, so it would need a
byte-slice fill added to `randrange`, which is outside lane B's write scope and
would need a request filed; and `SystemRandom::open()` fails on the host build
by design, so a session that held one could not be constructed in a host test at
all. `fill_secret` per packet keeps the failure at the exact call site that
caused it.

**Cost:** one `getrandom` per outbound packet, for 4–19 bytes. Against the
SHA-256 and AES pass the same packet already takes, this does not register; at
32 KiB packets a 100 MB/s transfer asks for it about 3000 times a second.
OpenSSH avoids even that with a userspace CSPRNG reseeded from the kernel, which
is the optimisation to reach for if this ever shows up in a profile — not a
reason to skip the draw.
