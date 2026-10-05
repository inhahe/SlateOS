## §900 — the mock AP is built from the crate it tests, and the assertions that matter are the ones that therefore aren't circular

**Date:** 2026-09-02. **Decided by:** Claude (autonomous). **Lane:** A.

**In short:** Lane C wrote the half of a WiFi join that a laptop performs — find
a network, prove you know the password, start sending data. It had nothing to
join *to*, so it could be unit-tested but never actually run. This adds the
other half: a pretend access point, living inside the kernel, that the real
code can associate with during the boot test. The catch is that the pretend AP
is built out of the same library it is supposed to be testing, so for anything
to do with message *format* it is that library being checked against itself,
which proves little. The decision is to accept that and be explicit about which
handful of checks escape it — the ones involving the encryption keys, which
each side works out on its own and which therefore cannot agree by accident.

**Context.** `net80211::assoc::Association` (lane C's, §579) drives the station
side of an 802.11 join over a `Transceiver`. Lane A supplied the `Transceiver`
impl on `hwsim`, the simulated radio (§677), and lane C's request
(`requests/c-a-the-transceiver-trait-has-landed-here-are-the-signatures.md`)
asked for one more thing: a call site that *builds an `Association` and polls
it*. There is no such call site possible without an authenticator, because the
AP sends EAPOL message 1 — without one the station sits in `Phase::Handshaking`
forever, and "the code ran and did not crash" is all a test could report.

**Decision.** Write `kernel/src/net/hwsim_ap.rs`, a WPA2-PSK access point that
speaks over `hwsim`, and drive a full join from the boot test: beacon, scan,
Open System authentication, association, the 4-way handshake, data in both
directions, and a group rekey. Build it from the shared crates — `net80211` for
frames and the key schedule, `aes` for the RFC 3394 key wrap — and state in the
module header, in the self-test's docs and at the call site both what a green
run proves and what it does not.

### The circularity, and what rescues it

A fixture built from the crate under test cannot independently confirm that
crate's wire format. If `net80211`'s beacon writer and its beacon parser are
wrong in the same way, this test agrees with itself and passes. That is a real
limit and pretending otherwise would be worse than the limit.

What escapes it is the cryptography. Each side derives the PTK independently,
from nonces that crossed the medium, sharing only the PMK; each verifies the
other's MIC over frames the other built. Two implementations cannot be wrong in
the same *direction* and still produce equal keys — a wrong KDF input on one
side yields 32 bytes that simply differ. So the self-test asserts
`assoc.tk() == ap.tk()` explicitly rather than leaving it implied by the
handshake completing, because that single comparison is the one carrying the
weight, and a reader should be able to find it.

The same reasoning fixes two implementation choices that would otherwise look
like gratuitous reuse:

- **The GTK key wrap uses the shared `aes` crate**, not a second RFC 3394
  written for the AP side. A wrap and an unwrap that are wrong in the same way
  agree with each other and pass — which is precisely the failure this fixture
  is supposed to be able to catch, and a second implementation would install it
  deliberately.
- **The RSN element comes from `rsn::write_body`**, not a hand-assembled
  duplicate. The station compares message 3's copy against the beacon's
  byte-for-byte, so one writer must produce both. A duplicate encoder only has
  to disagree in one octet — an omitted PMKID count, an elided capabilities
  field — to fail the handshake with an error pointing at the supplicant rather
  than at the fixture. The test additionally asserts the on-air element equals
  `ap.rsn_element()`, which turns what was a dead-code warning on that accessor
  into a check that the beacon path did not mangle the element in transit.

### KRACK, stated the strong way

The property worth asserting is not "the packet number did not rewind" but
"the driver was never *asked* to reinstall a key". `hwsim` refuses reinstalls
(§677), and that refusal is a backstop; a refusal that is never reached is the
actual assertion. So the test checks `pairwise_installs == 1 &&
key_reinstalls_refused == 0`, and re-checks both are unchanged **across a group
rekey** — the operation most likely to disturb the pairwise key by accident.

### A poll bound, not a clock

`drive()` gives up after 200 polls rather than after an elapsed time. Over
`hwsim` delivery is synchronous and in-memory, so a persistent `Idle` means one
side is not answering, not that it is slow. A bound in polls fails at the same
place every run, rather than at whatever place the machine happened to be slow
that day; a timeout would convert a deterministic bug into a flaky one.

### What a green run does not prove

**Confidentiality.** `hwsim` does not encrypt, deliberately (§677). The frame
exchange and the key schedule are exercised; the cipher is not. Lane C asked
specifically that this not be overclaimed, so it is written in three places —
the module header, the `self_test` doc comment, and the `main.rs` call site —
on the theory that the one a future reader meets is whichever they happen to
open.

**Alternatives considered.** *Hand-write the AP's frames independently*, which
would remove the circularity for format — rejected because it removes it by
adding a second unverified encoder, so a disagreement tells you two things
disagree without telling you which is right, and the likeliest outcome is
debugging the fixture. *Unit-test the AP in isolation* — rejected because the
whole request was for something that runs the station end to end. *Skip the AP
and assert only that `poll` returns `Idle`* — that is what already existed, and
it is what made the handshake untested.

**Where it lives.** `kernel/src/net/hwsim_ap.rs` (the AP and `self_test`),
`kernel/src/net/mod.rs` (module decl), `kernel/src/main.rs` (call site, after
`net::hwsim::self_test`), `kernel/Cargo.toml` (the `aes` dependency and the
comment explaining why it is not a second implementation).

**How to reverse.** The AP is test-only and nothing in the kernel depends on
it: delete the module, its declaration, the call site and the `aes` dependency.
Reversing the *reasoning* — deciding the format circularity is unacceptable —
means an AP built from an independent implementation, which is a much larger
piece of work and should be weighed against simply testing against real
hardware or against `hostapd` under QEMU, either of which removes the
circularity outright rather than trading it for a second unverified encoder.
