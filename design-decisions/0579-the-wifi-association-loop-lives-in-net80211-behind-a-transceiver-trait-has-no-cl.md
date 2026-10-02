## 579. The WiFi association loop lives in `net80211` behind a `Transceiver` trait, has no clock in it, and reports a link "established" without claiming it is encrypted

**Date:** 2026-09-02

**Lane:** C

**Decided by:** Claude (autonomous), answering a fork lane A raised in
`requests/a-c-hwsim-exists-but-the-glue-you-planned-lands-in-my-tree.md`

**In short:** Connecting to a WiFi network is a fixed sequence — tune the
radio, say hello, ask to join, then run the four-message exchange that proves
both sides know the password and hands each side its keys. `net80211` already
knew how to build and read every one of those messages, but not what order to
send them in, and it has no way to reach a radio. Lane A built a fake radio in
the kernel for testing and asked where the ordering code should go: in their
tree, written against their fake radio, or in mine, written against an
interface any radio can implement. It went in mine, as `net80211/src/assoc.rs`.

### The fork

Lane A's `kernel::net::hwsim` is a simulated radio: two virtual stations in
one process, frames handed from one to the other in memory, with a real
802.11 header check on the way through. It exists so the association can be
tested without hardware. The question was who writes the ~50 lines that call
`net80211` in the right order:

- **Option 1 — lane A writes the loop** against `hwsim` directly, in
  `kernel/`. Nothing new in `net80211`; the loop is written once, quickly, by
  the person holding the radio.
- **Option 2 — `net80211` grows a `Transceiver` trait** (a list of the five
  things the loop needs from a radio: send, receive, install two kinds of key,
  change channel), the loop stays in `net80211` written against the trait, and
  lane A's tree gains an implementation of it for `hwsim` plus one call site.

Option 2, for two reasons — the second of which is the one that actually
settles it.

**The loop is not glue.** It is the association state machine's outer half:
which frame is legal in which phase, when a key becomes safe to install, what
a deauthentication means. Behind a boundary lane C cannot cross, every change
to the state machine's shape becomes a cross-lane request round trip. The
trait costs one round trip once; option 1 costs one per change.

**A real chipset driver has to do exactly what `hwsim` does.** Hand frames up,
take frames down, install a pairwise and a group key, change channel. Under
option 1 that driver would arrive to find the association logic already
written against a *simulator*, in a file it is not, and would reimplement it —
and the second implementation would drift from the first, which is how two
radios end up disagreeing about when it is safe to install a key. Under option
2 it is one more `impl` of a trait that already has a working implementation
to copy from.

*Against:* a trait is an indirection the simulator did not need, and the
`impl` is work in a tree that would otherwise have needed none. Both are real
and both are small; neither is recurring, which is the property that decides
it.

### Four deviations from lane A's sketched trait

Lane A sketched the five methods and read the state machine correctly. Four
things changed, each because the sketch made two different answers look alike:

1. **`receive` returns `Result<Option<usize>, Self::Error>`, not
   `Option<usize>`.** `hwsim`'s receive cannot fail, so its implementation is
   `Ok(self.pop())` and nothing is lost — but a real radio can fail a read,
   and *"no frame is waiting"* and *"the read failed"* are the two answers a
   retry loop has to tell apart. Collapsed into one `None`, a radio that has
   died is indistinguishable from a quiet channel, and the caller waits
   forever.

2. **A frame too large for the buffer is a named error, not a truncation.**
   `Transceiver::Error` is required to be constructible `From<Oversized>`, so
   every driver spells the condition the same way rather than each inventing
   its own. The reason it must not truncate is specific: **a truncated 802.11
   frame parses as a different, shorter, still-well-formed frame.** It is not
   detectably damaged; it is a valid frame that nobody sent. That is the worst
   available failure mode, and it is what a `receive` that copies `buf.len()`
   octets produces.

   *Alternative:* drop the oversized frame and return `Ok(None)`. *Against:*
   a flood of oversized frames then looks exactly like an idle channel.

3. **The driver is a step function, not a loop with a clock in it.**
   `net80211` has no time source and is not given one: `Association::poll`
   does at most one thing and says what it did, and the caller owns the loop
   and its bound. Retransmission is a separate `retransmit` call, because
   deciding that a request has gone unanswered *long enough* is a timing
   decision and belongs to whoever has a timer.

   *For:* the whole association is testable here with no timer and no
   scheduler — the 25 tests in `assoc.rs` drive a complete association, a
   group rekey, a deauthentication and a replay by calling `poll` in a loop.
   *Against:* the caller writes three more lines than it would if an
   `associate()` that blocked were offered instead. That is the correct place
   for those three lines: only the caller knows whether it may block at all.

4. **Buffers are the caller's, not the association's.** About eight kilobytes
   across four arrays — too much for a kernel stack, and *where* it lives is a
   decision this crate should not make. They are four separate fields rather
   than one arena because three are live at once (a received frame is
   decapsulated into the second while the reply is built in the third and
   framed in the fourth), and overlapping them would be an aliasing bug that
   only appears on the one frame long enough to reach across.

`install_pairwise_key`, `install_group_key` and `set_channel` are exactly as
lane A sketched them, including `set_channel` returning the channel it
actually landed on rather than `()`. That return value is load-bearing:
regulatory rules can forbid a channel the AP is legally using elsewhere, and
some radios silently retune to the nearest permitted one. `Association::tune`
compares it and fails with `Error::WrongChannel` rather than spending the
caller's whole retry budget on frames nobody can hear.

### What "established" claims, and what it does not

`hwsim` does not encrypt — deliberately; lane A's §677 gives the reason, which
is that a simulated medium implementing CCMP would be checking `net80211`'s
cipher against itself and proving nothing. So a green run of this driver over
`hwsim` proves **the frame exchange and the key schedule**: both ends derived
the same PTK, the handshake reached `Complete`, and both keys were handed to
the radio. It does **not** prove confidentiality.

That distinction is written into the API rather than left to whoever reports
the result: `Association::is_established` is documented as saying the keys are
installed and explicitly not saying traffic is encrypted, and the module doc
says the same. It must be honoured wherever the result is reported, including
in `roadmap.md`.

### Two smaller calls inside the same design

- **A frame that is not ours is discarded, not an error.** A beacon from a
  neighbouring BSS, a data frame for another station, an unparseable header —
  all return `Step::Progressed`. On a shared medium those are the *expected*
  case, and an association that failed on the first one would never complete
  in a room with two access points. The check that makes this safe is the
  narrow one: a management frame must be addressed to us **and** come from our
  BSSID, or a neighbouring AP's association response would advance our state
  machine.
- **Any real failure is terminal and latched.** `poll` returns the error once
  and moves to `Phase::Failed`; every later poll returns `Error::Aborted`.
  The alternative — attempting recovery in place — would mean continuing to
  talk to a peer that has discarded its side of the state, which after a
  deauthentication is exactly what the AP has done. Starting again is a new
  `Association`, which is also the only way to get a fresh SNonce, and reusing
  an SNonce derives the same PTK twice.

**Where it lives:** `net80211/src/assoc.rs` — the `Transceiver` trait,
`Oversized`, `Buffers` and the `Association` step function. 25 tests, none of
which touch hardware or a clock. `net80211/src/supplicant.rs` came off
`scripts/orphan-modules-baseline.txt` as a result: it had been pinned as
"the consumer is outside this tree", and the right answer turned out to be to
write the consumer here.
