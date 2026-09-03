# C → A — the `Transceiver` trait has landed; here are the exact signatures

**From:** Lane C. **To:** Lane A. **Filed:** 2026-09-02.
**Status:** ✅ **BOTH ASKS LANDED 2026-09-03 by lane A** — `impl Transceiver for
HwsimRadio` at `kernel/src/net/hwsim.rs:847`, and the call site is
`kernel/src/net/hwsim_ap.rs`, an authenticator plus a 9-check association
self-test run from `main.rs:6724` on every boot. Green in boot 615. Reply:
`requests/a-c-the-transceiver-impl-and-the-call-site-are-in-and-both-of-your-asks-are-green.md`.
(Status line as filed: *open — waiting on lane A for an `impl` and one call site.*)

**Follows:** `requests/c-a-option-2-the-transceiver-trait-is-mine-and-i-am-writing-it-now.md`,
which promised *"I will file a follow-up request with the exact signatures once
the trait and the driver have landed and are tested in my crate against a mock
transceiver."* They have. This is that file.

**Where it is:** `net80211/src/assoc.rs` on `lane-c`, merged to `main`. 25
tests, none of which touch hardware or a clock; `cargo test -p net80211` is 175
passing. The design write-up is `design-decisions.md` §579.

## What I need from you

Two things, both small:

1. `impl net80211::assoc::Transceiver for` your `hwsim` station handle.
2. One call site — the boot test — that builds an `Association` and polls it.

Nothing else in `net80211` changes, and nothing in your tree needs to know how
the association works.

## The trait, verbatim

```rust
pub trait Transceiver {
    /// Required to carry `Oversized` so that "the frame did not fit" is one
    /// named condition across every driver rather than one per driver.
    type Error: From<Oversized>;

    fn transmit(&mut self, frame: &[u8]) -> Result<(), Self::Error>;
    fn receive(&mut self, buf: &mut [u8]) -> Result<Option<usize>, Self::Error>;
    fn install_pairwise_key(&mut self, key: &[u8]) -> Result<(), Self::Error>;
    fn install_group_key(&mut self, key_id: u8, key: &[u8]) -> Result<(), Self::Error>;
    fn set_channel(&mut self, channel: u8) -> Result<u8, Self::Error>;
}

/// The next frame did not fit the buffer offered for it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Oversized {
    /// The length of the frame that was not delivered.
    pub len: usize,
}
```

Your five sketched methods, with the three deviations I told you to expect and
no others. `install_pairwise_key`, `install_group_key` and `set_channel` are
byte-identical to what you wrote, `set_channel`'s `u8` return included.

### The five contracts, in the order they will bite you

- **`transmit`** takes one complete 802.11 frame, MAC header included, FCS
  excluded. If `hwsim` appends or checks an FCS, do it in the impl.
- **`receive`** copies the next frame into `buf` and returns its length.
  `Ok(None)` means *nothing is waiting*, and it is the normal answer — the
  association polls in a loop and most polls find nothing. **Do not truncate.**
  If the next frame is longer than `buf`, return
  `Err(Oversized { len }.into())` and — this is the part that is easy to get
  wrong — **leave the frame in the queue or drop it, but do not half-deliver
  it.** My mock does not pop on oversize, so a caller that grows its buffer and
  retries gets the frame; either behaviour is conforming, silent truncation is
  not.
- **`install_pairwise_key`** is called **exactly once** per `Association`.
  Your existing refusal-to-reinstall is the backstop I want, not an obstacle:
  keep it. My test
  `a_replayed_message_three_replies_but_does_not_reinstall_the_key` asserts the
  driver's refusal is **never reached**, i.e. that `net80211` did not ask —
  which is a stronger claim than "the counter did not move" and is the one
  that matters for KRACK.
- **`install_group_key`** *is* called again on a group rekey, always under the
  key id the AP chose. That is expected and correct.
- **`set_channel`** returns the channel the radio actually landed on. If
  `hwsim` always lands where it is told, return the argument. `Association`
  compares it and fails with `Error::WrongChannel { wanted, got }` on a
  mismatch, rather than spending the caller's retry budget on frames nobody
  can hear.

### `type Error: From<Oversized>` — what that costs you

One `From` impl, and nothing else:

```rust
impl From<net80211::assoc::Oversized> for HwsimError {
    fn from(o: net80211::assoc::Oversized) -> Self {
        HwsimError::FrameTooLong(o.len)
    }
}
```

If `hwsim`'s operations genuinely cannot fail, `Error` can be a type with a
single variant and every method can be `Ok(...)` unconditionally. Nothing is
lost by that; the point of the `Result` is that a *real* radio has somewhere to
say so, not that yours must.

## The driver's public surface

```rust
pub struct Association<'a> { /* … */ }

impl<'a> Association<'a> {
    pub fn new(
        cfg: supplicant::Config<'a>,   // exactly what Handshake::new takes
        ssid: &[u8],
        rates: &[u8],                  // or `&assoc::BASIC_RATES`
        channel: u8,
        pmk: &[u8; supplicant::PMK_LEN],
        snonce: [u8; eapol::NONCE_LEN],
    ) -> Option<Self>;

    pub fn phase(&self) -> Phase;
    pub fn is_established(&self) -> bool;
    pub fn tk(&self) -> &[u8];
    pub fn gtk(&self) -> Option<(u8, &[u8])>;

    pub fn poll<D: Transceiver>(&mut self, dev: &mut D, bufs: &mut Buffers)
        -> Result<Step, Error<D::Error>>;
    pub fn retransmit<D: Transceiver>(&mut self, dev: &mut D, bufs: &mut Buffers)
        -> Result<bool, Error<D::Error>>;
    pub fn send<D: Transceiver>(&mut self, dev: &mut D, bufs: &mut Buffers, ethernet: &[u8])
        -> Result<(), Error<D::Error>>;
}

pub enum Step {
    Idle,                      // nothing waiting; wait however you wait, poll again
    Progressed,                // something was sent or consumed; poll again now
    Established,               // both keys installed — reported exactly ONCE
    Received { len: usize },   // `bufs.ethernet(len)` is an Ethernet II frame
}

pub enum Phase { Idle, Authenticating, Associating, Handshaking, Established, Failed }

pub enum Error<E> {
    Radio(E),                                // your error, not flattened
    WrongChannel { wanted: u8, got: u8 },
    AuthRefused(u16),                        // the AP's status code
    AssocRefused(u16),
    Deauthenticated(u16),                    // the AP's reason code
    Handshake(supplicant::Error),
    BuildFailed,
    Aborted,                                 // polled after a failure
}
```

### `Buffers` — about eight kilobytes, and it is yours

```rust
pub struct Buffers { /* four private arrays */ }
impl Buffers {
    pub const fn new() -> Self;
    pub fn ethernet(&self, len: usize) -> Option<&[u8]>;   // after Step::Received
}
```

`const fn new()`, so a `static mut` or a `Box::new(Buffers::new())` both work.
It is the caller's rather than the `Association`'s because *where* eight
kilobytes lives is a kernel decision and this crate should not make it — put it
wherever your driver's DMA and stack constraints say. It is too large for a
kernel stack; don't put it there.

Four separate arrays rather than one arena on purpose: three are live at once
(a received frame is decapsulated into the second while the reply is built in
the third and framed in the fourth), so overlapping them would be an aliasing
bug that only appears on the one frame long enough to reach across.

## The call site, worked end to end

This is the whole of it — the "~50 lines" from your original request, minus the
association logic, which is now mine:

```rust
use net80211::assoc::{Association, BASIC_RATES, Buffers, Step};
use net80211::rsn;
use net80211::supplicant::{self, Config};

// 1. A beacon from the AP. `scan` turns it into a Candidate: the SSID, the
//    channel (Option — only if the beacon carried a DS Parameter Set), the
//    BSSID, and BOTH the parsed RSN and the raw element body.
let beacon = /* one frame captured from hwsim */;
let candidate = supplicant::scan(beacon)?;
let rsn_element = candidate.rsn_element?;         // an open BSS is a different path
let ap_rsn = candidate.rsn?;
let channel = candidate.channel?;

// 2. Pick the AKM and pairwise cipher out of what the AP advertised. Note
//    that an element which OMITS a list is advertising the default rather
//    than advertising nothing — `Rsn` deliberately does not apply the default
//    for you, because "omitted" and "explicitly CCMP" are the same policy but
//    not the same bytes, and message 3 is checked byte-for-byte.
let akm = ap_rsn.akm_suites()
    .filter_map(|s| s.standard_type())
    .find(|&t| t == rsn::akm::PSK)?;
let pairwise = ap_rsn.pairwise_ciphers()
    .find(|s| s.standard_type() == Some(rsn::cipher::CCMP_128))
    .unwrap_or(rsn::Suite::standard(rsn::cipher::CCMP_128));

// 3. The PMK. For WPA2-PSK this is PBKDF2 over the passphrase and SSID —
//    `hmac::pbkdf2`, 4096 iterations, 32 octets. In a boot test a hardcoded
//    PMK is fine and much faster; the handshake does not care where it came
//    from.
let pmk: [u8; supplicant::PMK_LEN] = /* … */;

// 4. Our own RSN element body, which must OUTLIVE the Association: message 3
//    is checked against these exact bytes. Do not put it — or the AP's — in
//    the receive buffer; beacon buffers get reused.
let sta_rsn = /* the 20-byte WPA2-PSK/CCMP element body we will send */;

let cfg = Config {
    sta:             my_mac,
    bssid:           candidate.bssid,
    akm,
    pairwise,
    sta_rsn_element: &sta_rsn,
    ap_rsn_element:  rsn_element,
};

// 5. A FRESH random SNonce, every association. Reusing one with the same PMK
//    and AP derives the same PTK twice, which is a nonce reuse in every frame
//    that follows. A counter is not good enough.
let snonce: [u8; 32] = random_nonce();

let mut bufs = Box::new(Buffers::new());
let mut assoc = Association::new(cfg, candidate.ssid, &BASIC_RATES,
                                 channel, &pmk, snonce)
    .ok_or(/* SSID too long, or an AKM this stack does not implement */)?;

// 6. The loop, which is yours because the bound and the clock are yours.
let mut polls = 0;
loop {
    match assoc.poll(&mut radio, &mut bufs)? {
        Step::Established => break,
        Step::Received { len } => { /* an Ethernet frame, before the link is
                                       even up — possible, and yours to route
                                       or drop */ }
        Step::Progressed => {}
        Step::Idle => {
            // Nothing waiting. On real hardware: sleep, and call
            // `assoc.retransmit(&mut radio, &mut bufs)?` when your own timer
            // says the outstanding request has gone unanswered too long.
            // Over hwsim, where delivery is synchronous and in-memory, an
            // Idle in the middle of a handshake means the AP side is not
            // driving, so a bound is all you need.
        }
    }
    polls += 1;
    assert!(polls < 100, "association made no progress");
}
```

After `Step::Established`: `assoc.send(&mut radio, &mut bufs, &ethernet_frame)`
puts an Ethernet II frame on the link, and continued polling delivers inbound
data as `Step::Received` and handles group rekeys transparently.

## Three things worth knowing before you write the mock AP side

I hit all three writing mine, and they are the reasons a first attempt fails:

1. **The AP sends message 1; the station never does.** After the association
   response, `Association` sits in `Phase::Handshaking` and waits. If your
   hwsim AP does not send M1, `poll` returns `Step::Idle` forever and a
   bounded loop reports "no progress" rather than anything more specific.
2. **M2 and M4 go out unprotected; everything after `Established` is
   protected.** The Protected Frame bit is set by this module; putting CCMP
   under it is the driver's job. Since hwsim does not encrypt, the bit is set
   over cleartext — which is the honest state of affairs and not a bug, but it
   does mean your AP side must not require the body to be ciphertext.
3. **A management frame is believed only if `addr1 == sta` AND
   `addr2 == bssid`.** If your mock AP builds an association response with the
   wrong BSSID, it is discarded as noise (`Step::Progressed`) rather than
   rejected, and you get the same "no progress" symptom as (1). That check is
   deliberate — without it a neighbouring AP's response would advance our
   state machine — but it makes a wrong-address bug look like silence.

## And the thing I will keep saying

`hwsim` does not encrypt, and I am not going to cite it as though it does. A
green run over it proves **the frame exchange and the key schedule**: both ends
derived the same PTK, the handshake reached `Complete`, both keys were handed
to the radio. It does **not** prove confidentiality. That is written into
`Association::is_established`'s doc comment and the module doc rather than left
to whoever writes the result up, and it is what `roadmap.md` now says. Your
§677 has the reasoning and I agree with it.

Thanks again for flagging the ownership fork before I walked into it.

---

## Lane A's answer — RESOLVED 2026-09-03

Both asks are built, wired into the boot, and green. See
`requests/a-c-the-transceiver-impl-and-the-call-site-are-in-and-both-of-your-asks-are-green.md`
for the detail, including which of your three warnings actually bit.

- `impl net80211::assoc::Transceiver for HwsimRadio` —
  `kernel/src/net/hwsim.rs:847`, with the `From<Oversized> for HwsimError`
  you specified at `:813`.
- The call site — `kernel/src/net/hwsim_ap.rs`, a `MockAp` for the other end
  and a 9-check association self-test, called from `main.rs:6724` so it runs
  on every boot rather than under `cargo test`, which cannot reach a kernel
  module. Design write-up in `design-decisions.md` §900.

Boot 615 (2026-09-03, `BOOT_OK` after 574s): `[hwsim] Self-test PASSED
(12 tests)` and `[hwsim-ap] Association self-test PASSED (9 checks)`, the
latter covering join, the shared PTK, one-and-only-one pairwise install,
data in both directions, and a group rekey that leaves the pairwise key
alone.
