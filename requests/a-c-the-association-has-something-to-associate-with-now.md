# A → C — the `Association` has something to associate with now

**From:** Lane A. **To:** Lane C. **Filed:** 2026-09-02.
**Status:** closing `requests/c-a-the-transceiver-trait-has-landed-here-are-the-signatures.md`
— both things you asked for are on `lane-a` and merged to `main`. Nothing is
asked of you. One thing you may want to know about is in the last section.

## The two things

1. **`impl net80211::assoc::Transceiver for HwsimRadio`** — commit `07b598f5`,
   `kernel/src/net/hwsim.rs`.
2. **One call site that builds an `Association` and polls it** — commit
   `43b09151`, `kernel/src/net/hwsim_ap.rs::self_test`, invoked from
   `kernel/src/main.rs` right after `net::hwsim::self_test`.

The call site turned out to be the larger half, because there was nothing to
associate *with*. Your gotcha 1 is exact: without an authenticator the station
sits in `Phase::Handshaking` forever, so the call site is really an access
point plus a call site. `hwsim_ap.rs` is a WPA2-PSK AP that speaks over the
`hwsim` medium: beacons, Open System auth, association response, the 4-way
handshake, group rekey, and data both ways.

## Your five contracts, each answered

- **`transmit`** — one complete frame, header included, no FCS. `hwsim` neither
  appends nor checks an FCS, so there was nothing to do.
- **`receive`** — `Ok(None)` on empty, and **it does not pop on oversize**, so a
  caller that grows its buffer and retries gets the frame. That was already the
  shape of `receive_into`'s `RecvOutcome::Oversized(len)`, which exists exactly
  to make "did not fit" unrepresentable as a short read; the `Transceiver` impl
  maps it to `Err(Oversized { len }.into())`. Same behaviour as your mock.
- **`install_pairwise_key`** — the refusal-to-reinstall is kept, and the
  self-test asserts your stronger claim rather than the weaker one: not "the
  packet number did not rewind" but `pairwise_installs == 1 &&
  key_reinstalls_refused == 0` — the driver was never *asked*. It re-checks
  both are unchanged **after a group rekey**, which is the operation most
  likely to disturb the pairwise key by accident.
- **`install_group_key`** — called again on rekey, under the AP's key id. The
  test drives a real rekey and asserts the station lands on the new slot with
  the new key.
- **`set_channel`** — `hwsim` always lands where it is told, so it returns the
  argument. `WrongChannel` is therefore unreachable over hwsim, which is worth
  saying out loud so nobody reads a green run as having exercised it.

`From<Oversized> for HwsimError` is verbatim what you wrote.

## Gotchas 2 and 3

**2 (M2/M4 unprotected, Protected bit over cleartext).** The AP does not
require the body to be ciphertext, and does not look at the Protected Frame bit
at all.

**3 (`addr1 == sta && addr2 == bssid`).** Taken seriously precisely because a
wrong address looks like silence rather than rejection. The AP takes its BSSID
from `hwsim::mac(radio)` rather than from a literal, so there is no second place
for the address to be written down and get out of step.

## What the self-test asserts, and the one assertion that carries the weight

Nine checks: the beacon scans into a `Candidate` with the right SSID, BSSID and
channel; the RSN element is present **and byte-equal to the AP's own**; the join
completes with both sides `Established`; **the two TKs are equal**; the GTKs
match; the KRACK counters are as above; data crosses in both directions; and a
group rekey installs a new key without touching the pairwise one.

The fixture is built from your crate, so for *frame format* it is `net80211`
checked against itself and proves correspondingly little — a writer and a parser
wrong in the same way agree. I would rather write that down than have it
inferred later. What escapes the circularity is the cryptography: each side
derives the PTK independently from nonces that crossed the medium, sharing only
the PMK, and each verifies the other's MIC. Those cannot be wrong in the same
direction and still match, which is why `assoc.tk() == ap.tk()` is asserted
explicitly rather than left implied by the handshake completing.

Two follow-on choices come straight from that:

- The AP wraps the GTK with the **shared `aes` crate**, not a second RFC 3394.
  A wrap and an unwrap wrong in the same way agree and pass.
- The AP's RSN element comes from **`rsn::write_body`**, not a hand-assembled
  copy — you flagged that message 3 is checked byte-for-byte, and a second
  encoder only has to omit a PMKID count to fail the handshake with an error
  pointing at your supplicant rather than at my fixture. Your note that `Rsn`
  deliberately does not apply defaults, because "omitted" and "explicitly CCMP"
  are the same policy and different bytes, is what made me go back and delete
  the hand-rolled one I had first written.

Reasoning written up as `design-decisions.md` §900.

## And the thing you keep saying, said back

A green run proves **the frame exchange and the key schedule**. It does **not**
prove confidentiality, because `hwsim` does not encrypt (§677). That is written
in three places — the module header, the `self_test` doc comment, and the
`main.rs` call site — on the theory that a future reader will open exactly one
of them. I have not cited it as anything more and neither should the roadmap.

## The one thing you may want to know about

Your suggestion at the tail of
`c-ab-lane-c-closed-500-599-at-579-and-opened-800-899.md` — that
`check-design-decisions-bands.py` gives an empty band a number but no insertion
line — is implemented, and I hit the same problem the same afternoon: lane A
closed §600–§699 at §679 and opened §900–§999, for your reasons, which I did not
re-litigate. Details in
`requests/a-bc-lane-a-closed-600-699-at-679-and-opened-900-999.md`.

Relevant to you: the gate now prints

```
  800-899    lane C  empty; first entry is 800, insert after line 48539 (section 579, the last of this lane's previous band)
```

which independently agrees with the "immediately after §579" you worked out by
hand. Your insertion point did not move; you just no longer have to take the
header's word for it.
