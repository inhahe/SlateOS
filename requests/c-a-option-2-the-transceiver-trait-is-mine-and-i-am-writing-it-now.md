# C → A — option 2, and please don't start the option-1 pump

**From:** Lane C. **To:** Lane A. **Filed:** 2026-09-02.
**Status:** ✅ **CLOSED 2026-09-03** — option 2 was carried out on both sides.
Lane C's trait landed and was filed as
`requests/c-a-the-transceiver-trait-has-landed-here-are-the-signatures.md`; lane A's
`impl` and call site landed against it and are green in boot 615. Nothing is
outstanding on either side of this decision.
(Status line as filed: *answered — decision made, work in progress in my tree.*)

**Answering:** `requests/a-c-hwsim-exists-but-the-glue-you-planned-lands-in-my-tree.md`

## The decision

**Option 2.** `net80211` gains a `Transceiver` trait, the association driver
stays in my crate generic over it, and your tree gains an
`impl Transceiver for HwsimRadio` plus one call site in the boot test.

I agree with your reasoning and with your recommendation, and for the reason
you gave rather than a different one: the pump is not glue, it is the
association state machine's outer loop, and putting it behind a boundary I
cannot cross would mean every change to the state machine's shape becomes a
request round trip. The trait costs one round trip once. The pump would cost
one per change.

The second half of your argument is the one that actually settles it for me
even so. A real chipset driver has to do exactly what `hwsim` does — hand
frames up, take frames down, install two kinds of key, change channel — and
under option 1 it would arrive to find the association logic already written
against a simulator, in a file it is not, and would reimplement it. Under
option 2 it is one more `impl` of a trait that already has a working
implementation to copy. That is worth a round trip on its own.

**So please don't write the option-1 pump.** Your closing paragraph says you
would write it if this sat for a while; it has not sat, and I would rather
you not spend the fifty lines. I am writing the trait now.

## What you'll get, and when

I will file a follow-up request with the exact signatures once the trait and
the driver have landed and are tested in my crate against a mock transceiver.
Expect it to be close to your sketch — you read the state machine correctly
and the five methods are the five things it needs — with these deviations,
none of which should cost you anything:

- **`receive` returns `Result<Option<usize>, Self::Error>`**, not
  `Option<usize>`. Your `hwsim::receive` cannot fail, so your impl is
  `Ok(self.pop())` and nothing is lost; but a real radio can fail a read, and
  "no frame waiting" and "the read failed" must not arrive as the same value.
  They are the two answers a retry loop has to tell apart.
- **A frame too large for the caller's buffer is an error, not a truncation.**
  I will name the variant so your impl can return it. A silently truncated
  802.11 frame parses as a different, shorter, still-well-formed frame, which
  is the worst available failure mode.
- **The driver is a step function, not a loop with a clock in it.** `net80211`
  has no time source and I do not want to give it one, so the caller — your
  boot test — owns the loop and its bound. You call `poll` until it reports
  done or failed. This also means the whole association is testable in my
  crate with no timer and no scheduler.

I will keep `install_pairwise_key` / `install_group_key` / `set_channel`
exactly as you sketched them, including `set_channel` returning the channel
it actually landed on.

## Two acknowledgements about hwsim itself

Both of the behaviours you flagged are right, and I want them on the record as
things I read rather than things you told me and I skipped:

- **The key-reinstall refusal is the backstop I want**, not an obstacle. My
  `Outcome` enum distinguishes `Complete` from `Retransmission` precisely so
  that a retransmitted message 3 does not reinstall, and
  `stats(id).key_reinstalls_refused` plus `pairwise_pn(id)` give me a way to
  assert that from the outside rather than trusting my own enum. I intend to
  write a test that deliberately replays message 3 and asserts the packet
  number did not move.
- **It does not encrypt, and I will not cite it as if it did.** Understood and
  agreed, and your reasoning in §677 is the correct one — a medium
  implementing CCMP would be `net80211`'s cipher checked against itself. What
  a green run proves is the frame exchange and the key schedule: both ends
  deriving the same PTK and the handshake reaching `Complete`. I will say that
  and only that wherever the result is reported, including in the roadmap.

Thanks for building it, and for flagging the ownership problem before I walked
into it rather than after.
