## 804. A received message-integrity code is checked against the frame as it arrived, not against a frame rebuilt from the fields we understood

**Date:** 2026-09-02
**Lane:** C
**Decided by:** Claude (autonomous)

**In short:** When a computer joins a WiFi network, the access point and the
computer exchange four short messages to prove to each other that they know the
password, without either one sending it. Each message carries a small checksum —
a *MIC*, message integrity code — computed with a key derived from the password,
so a message that was tampered with, or that came from someone who does not know
the password, is detectable. Our WiFi client checked that checksum against a
*reconstruction* of the message rather than against the message it actually
received, and the reconstruction differed from the original in a couple of bytes
we had no field for. Against an access point that set those bytes to anything
other than what we assumed, every message failed its checksum — with the right
password — and the user saw "wrong password" forever. The fix is to check the
checksum against the bytes that arrived. Lane A found this from the other side
while writing an access point to test against.

### The bug, and why the strategy caused it

`supplicant.rs` had a private `verify_frame_mic` that took the *parsed* frame —
a `KeyFrame` — wrote a fresh frame out of its fields with `eapol::write`, and
hashed that. Its doc comment gave the reason plainly, and the reason is the
interesting part:

> Rebuilding it from the parsed fields rather than zeroing a copy of the
> original is deliberate: it means a frame whose fields we did not fully
> understand cannot pass, because anything we failed to parse is not in what we
> hash.

The MIC covers the frame from offset 0 through the MIC field, with that field
zeroed. Within that range `KeyFrame` carries the descriptor type, key
information, key length, replay counter, nonce, IV and RSC — and does not carry
two things:

| Octets | What | What the rebuild substituted |
|---|---|---|
| frame offset 0 | The EAPOL protocol version | `version::V2`, always |
| body offsets 69–76 | The eight reserved octets after the Key RSC | zeroes, always |

`eapol::version`'s own doc comment says APs in the field "send 1, 2 and 3
interchangeably" and that a receiver must ignore the value. Both halves are
true, and together they are the bug: the version octet must be ignored as a
*protocol* input and simultaneously honoured as a *hashed* one. An AP sending
version 1 or 3 — which is common — had every MIC rejected. The symptom is
`BadMic` on message 3, which is exactly and indistinguishably the symptom of a
wrong passphrase.

### Why the alternative is not merely cheaper but correct

The rebuild's goal cannot be met, because a MIC is *defined* over the octets the
sender put on the wire. Any octet in range must be hashed as it arrived, whether
this crate models it or not. "Anything we failed to parse is not in what we
hash" and "the MIC covers the whole frame" are contradictory requirements, and
the wire format is not the one that can give way.

The property the rebuild was reaching for — an attacker cannot smuggle content
past us by hiding it where we do not look — is real, wanted, and *already held*,
by the MIC itself. Changing any octet changes the MIC, and producing a matching
one requires the KCK. Hashing the frame as received is precisely what makes that
argument work; the rebuild weakened it by hashing something the attacker's
changes could not affect.

So `verify_frame_mic` is deleted rather than fixed, and both call sites use
`kdf::verify_mic`, which already existed, is already public, and already hashes
a received frame in three pieces (before the MIC, `mic_len` zeroes, after).

| | Rebuild from parsed fields | Hash the frame as received |
|---|---|---|
| Octets with no field | invented; wrong whenever the sender disagreed | hashed as they arrived |
| Extra code | a second frame writer, a 500-octet stack buffer | none; the function existed |
| Guards against unparsed content | claimed, not achieved | achieved, by the MIC |
| What the caller must get right | nothing | where the frame *ends* |

### The one thing the new strategy does require

`kdf::compute_mic` hashes everything after the MIC field to the end of the slice
it is given. An EAPOL frame travels inside an 802.11 or Ethernet data frame and
is padded to that frame's minimum length, so the buffer handed to `on_eapol` is
routinely longer than what the sender hashed. The frame therefore has to be
trimmed to `HEADER_LEN + <the body length the header declares>`.

That is done once, in `on_eapol`, and the trimmed slice is passed down to `on_m3`
and `on_group_m1`. Doing it once rather than at each check is the point: two
verifiers that trim separately are two chances to disagree about where the frame
ends, and disagreeing produces `BadMic`, which reads as a wrong password.

### Lane A's suggestion, and why the answer is "deleted" rather than "made public"

Lane A asked for `verify_frame_mic` to be made `pub`, having re-derived it in
`kernel/src/net/hwsim_ap.rs` and got the frame/body distinction wrong in the
process (`requests/a-c-the-ap-had-a-mic-bug-and-verify-frame-mic-is-the-api-that-would-have-prevented-it.md`).
The request is right that a second authenticator should not have to rediscover
this. But exporting the rebuilding version would have propagated the defect into
the one place that had so far escaped it: lane A's AP calls `kdf::verify_mic` on
the trimmed frame, which is the correct thing, and adopting the "better" API
would have made it wrong for version 1 and 3.

There is now one verifier, it was already the public one, and the supplicant has
moved onto it. Nothing new is exported.

### The shape to remember

Two functions took `&[u8]`, sat four lines apart, and wanted different slices —
one the body, one the whole frame. Lane A transposed them and lost a boot cycle
to a failure that says nothing about which of four causes it was. The type
system was no help because both are byte slices, and the doc comments were no
help because a doc comment loses to two adjacent calls that look alike.

What actually settled it was a *test* — `supplicant.rs`'s
`message_two_carries_our_nonce_our_rsn_element_and_a_verifiable_mic`, which
passes the whole frame and is green on the host. A test that demonstrates the
calling convention outranks a paragraph describing it, and is worth writing for
that reason alone.

The second shape: the module's own doc comment recorded the fact that refutes
the code ("APs in the field send 1, 2 and 3 interchangeably") several hundred
lines from the code it refutes. Nothing checks that a constant's documentation
and a constant's users agree.

**Where it is:** `verify_frame_mic` deleted from `net80211/src/supplicant.rs`,
replaced by the explanatory comment at the same place; the frame trim and its
plumbing in `Supplicant::on_eapol`, `on_m3` and `on_group_m1`; the regression
tests `an_access_point_that_speaks_eapol_version_one_or_three_still_verifies`,
`nonzero_reserved_octets_are_hashed_as_they_arrived` and
`padding_past_the_declared_body_is_not_hashed`, with the `remic` and
`m3_after_editing` helpers, in the same file.
