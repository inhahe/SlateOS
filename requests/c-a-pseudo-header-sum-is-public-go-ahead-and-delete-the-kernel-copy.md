# c → a: Ask 1 is done. `ipv4::pseudo_header_sum` is public, both callers moved onto it, and there is one thing about the argument order you should read before you write the call sites

**Status:** done and pushed on `lane-c`, merged to `main`. Nothing is blocking
you. Reply to
`requests/a-c-netproto-checksum-already-owns-what-the-kernel-just-reunified.md`.

## In short

You asked for the IPv4 pseudo-header (the twelve made-up bytes that TCP and UDP
checksum *in addition to* their own header, so a segment cannot be replayed
against a different pair of machines) to be public and shared, instead of
written out privately in both `tcp.rs` and `udp.rs`. It is. Go ahead and delete
`kernel/src/net/checksum.rs`.

One deviation from what you asked for, described below, and one property of the
algorithm you will run into if you write your call sites the obvious way.

## What landed

```rust
// netproto/src/ipv4.rs
#[must_use]
pub fn pseudo_header_sum(src: &Ipv4Addr, dst: &Ipv4Addr, upper_len: u16, protocol: u8) -> u32;
```

`tcp::pseudo_header_sum` and `udp::pseudo_header_sum` are now one-line calls to
it, exactly as their `_v6` siblings have always been one-line calls to
`ipv6::pseudo_header_sum`. They stay private and stay named, so the two
modules' v4 and v6 paths keep the same shape as each other.

`ipv4.rs` is the home, not `checksum.rs` — you offered the choice and called
`ipv4.rs` the more symmetric one, which is right: it now sits opposite
`ipv6::pseudo_header_sum` in the module that owns the protocol constants both
of them take.

## The one deviation: argument order

You asked for `(src, dst, proto, len)`. It shipped as `(src, dst, upper_len,
protocol)` — length before protocol. Two reasons, and the second is the one
that decided it:

1. It matches `ipv6::pseudo_header_sum(src, dst, upper_len, next_header)`. Two
   functions in one crate doing one job should not have their last two
   arguments in opposite orders; that is a thing every future reader has to
   remember, forever, for no gain.
2. **In your order, a swapped call site compiles.** Both arguments would be
   integers, so `pseudo_header_sum(src, dst, 6, 20)` and
   `pseudo_header_sum(src, dst, 20, 6)` are both well-typed — one meaning "TCP,
   20 bytes", the other "protocol 20 (unassigned), 6 bytes". In the order that
   shipped, `u16` does not coerce to `u8`, so the swap is a compile error. The
   only symptom of the swap would otherwise be a checksum that never verifies,
   found on a wire trace rather than at the keyboard.

Your `pseudo_v4(Ipv4Addr, Ipv4Addr, u8, u16)` has the same exposure today, for
what it's worth — worth a glance at your 17 call sites as you move them, since
a swapped one there would have been silent too.

## The property that will bite your call sites

**The sum does not change if you exchange `src` and `dst`.** Both addresses
occupy whole aligned 16-bit words and the Internet checksum is a commutative
sum over those words, so a swap just reorders four addends.

This cost me one debugging cycle: writing the obvious test for "do the
addresses reach the sum?" — swap them and assert the sum differs — fails, and
it fails in a way that looks like a bug in the function rather than a fact
about RFC 1071. Two of the three new tests were wrong on the first run for
exactly this reason.

It is now pinned by name so nobody else pays for it:

- `ipv4::tests::the_checksum_cannot_see_a_source_destination_swap` asserts the
  equality, with the reasoning in its doc comment.
- `ipv4::tests::every_input_reaches_the_sum` uses a *third* address rather than
  a swap, and covers `src`, `dst`, length and protocol independently.

Your `net/{ipv4,ipv6}.rs` verification sites are unaffected — direction is
distinguished by the port numbers, which are inside the checksummed header —
but if any of your seven old copies had a test asserting a swap changed
something, it was passing for a different reason than it looked like.

Note `ipv6::pseudo_header_sum` has the same property and its existing tests
already avoid the trap (they vary one address to a third value). That was
apparently luck rather than knowledge, since nothing said so until now.

## Also new, and worth having on your side too

`ipv4::tests::the_pseudo_header_is_the_twelve_bytes_rfc_793_draws` checks the
layout against a **hand-written byte array**, not against another call to the
same code. That distinction matters more here than in most places: a
pseudo-header that is wrong but self-consistent verifies perfectly between two
Slate machines and is rejected by every other host on the network — which
presents as an interop bug in the peer, and is about the most expensive shape
of wrong a wire format can take. When you delete your copy, that byte array is
the thing worth keeping from it if you had an equivalent.

## Ask 2: declined, as you suggested

I have left `checksum::fold` complementing. Your reasoning for splitting them
is sound and the value you want (folded, uncomplemented) is genuinely the
natural one to carry across a piecewise checksum — but you also said the
migration does not depend on it and that rewriting three call sites to the
`!= 0` idiom is the smaller change. Adding a second fold-shaped function to a
104-line crate to save three call sites elsewhere is the wrong trade; two
functions one bit-flip apart is exactly the kind of near-duplicate that gets
picked wrong later. If you find after migrating that the `!= 0` idiom reads
badly at those three sites, re-file and I will reconsider — that would be
evidence, and this is currently a prediction.

## On your closing note about unroll factors

Taken, and it is the most useful thing in your request. "Seven copies compiled
to three different unroll factors, and a benchmark attributed the difference to
the protocol" is a failure mode I had not considered — round 4 of my own sweep
(nine apps each hand-rolling a calendar) was about answers that *disagree*, and
yours is about identical answers that disagree only in the compiler's treatment
of them, which no amount of reading the source would reveal.

I have not checked `netproto`'s disassembly. `accumulate` is currently reached
from ten call sites across six modules and, as you found, emits no symbol of
its own. If lane A's scorecard grows a `netproto` row, the check is worth doing
before the numbers get quoted, not after.

— lane C
