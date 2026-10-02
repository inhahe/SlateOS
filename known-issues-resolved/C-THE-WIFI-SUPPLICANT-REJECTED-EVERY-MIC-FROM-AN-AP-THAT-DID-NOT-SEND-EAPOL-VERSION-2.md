## C-THE-WIFI-SUPPLICANT-REJECTED-EVERY-MIC-FROM-AN-AP-THAT-DID-NOT-SEND-EAPOL-VERSION-2 — FIXED 2026-09-02 (lane C)

**Status:** FIXED 2026-09-02 — awaiting a boot test on `main` before archiving.

**Reported by lane A**, from the authenticator side, in
`requests/a-c-the-ap-had-a-mic-bug-and-verify-frame-mic-is-the-api-that-would-have-prevented-it.md`.

### What was wrong

`net80211::supplicant`'s private `verify_frame_mic` verified a received
EAPOL-Key frame's message integrity code by *rebuilding* the frame from the
parsed `KeyFrame` and hashing the reconstruction, rather than hashing the frame
that arrived. The MIC covers the frame from offset 0, and `KeyFrame` carries no
field for two runs of octets inside that range:

- the **EAPOL version octet** at frame offset 0, and
- the **eight reserved octets** at body offsets 69–76 (frame 73–80).

The rebuild substituted `eapol::version::V2` and eight zeroes for them
unconditionally. Against any access point that sent EAPOL version 1 or 3 —
which `eapol::version`'s own doc comment describes as commonplace, and which is
what makes this a field bug rather than a theoretical one — every MIC failed.

### Why it was invisible

Three things hid it at once:

1. **The symptom is the wrong one.** The handshake fails at message 3 with
   `Error::BadMic`, which is character-for-character what a wrong passphrase
   produces. A user would have retyped their password.
2. **The tests could not catch it.** `run_handshake` builds its fixtures with
   `eapol::write(..., version::V2, ...)` and all-zero reserved octets, so the
   rebuild and the original agreed by construction. All 175 tests passed before
   and after the fix; three new ones had to be written to see it.
3. **The refutation was already in the file, several hundred lines away.**
   `eapol::version`'s doc comment states that APs send 1, 2 and 3
   interchangeably. Nothing checks that a constant's documentation and its
   users agree.

### The fix

`verify_frame_mic` is deleted, not repaired. The rebuild strategy cannot be
made correct — a MIC is defined over the octets the sender transmitted, so
every octet in range must be hashed as received whether this crate models it or
not. Both call sites (`on_m3`, `on_group_m1`) now use `kdf::verify_mic`, which
already existed, was already public, and hashes the received frame in three
pieces. `on_eapol` trims the incoming buffer once to
`HEADER_LEN + <declared body length>`, because a frame padded to its carrier's
minimum length would otherwise have the padding hashed.

Reasoning in full, including why lane A's "make it public" suggestion was
answered with a deletion instead, in `design-decisions.md` §804.

### Regression tests

In `net80211/src/supplicant.rs`:

- `an_access_point_that_speaks_eapol_version_one_or_three_still_verifies`
- `nonzero_reserved_octets_are_hashed_as_they_arrived`
- `padding_past_the_declared_body_is_not_hashed`

The first two were run against the pre-fix code and observed to fail there.
They were spliced into the `HEAD` copy of `supplicant.rs`, so the old
rebuilding verifier was the only thing that differed, and both failed with

    version 1 should verify, got Err(BadMic)
    nonzero reserved octets should verify, got Err(BadMic)

— which is the bug reproducing exactly as reported, and is worth reading twice:
`BadMic` is also what a wrong passphrase produces, so in the field this defect
had no symptom of its own to distinguish it.

The third passes against the old code, and that is not a weakness in it. The
rebuild excluded trailing padding by construction — a frame reassembled from
parsed fields has no room for octets nobody parsed — so the padding hazard is
one this fix *introduced* by hashing the buffer as received, and the test
guards the trimming that answers it.

Each works by taking a canonical message 3, editing the octet under test, and
re-MICing the result — so what is asserted is the property that matters (the
*sender* hashed this octet, therefore we must too) rather than the one that is
easy to assert.

### Still worth knowing

`eapol::KeyFrame::parse` takes a **body** while everything MIC-related indexes
from the start of the **frame**. That asymmetry is what lane A transposed, and
it survives this fix: both are `&[u8]`, so the compiler cannot tell them apart.
Changing `parse` to take the frame would remove the trap at the type level, but
it is a breaking change to an API `kernel/src/net/hwsim_ap.rs` (lane A's tree)
calls, so it needs a coordinated change rather than a unilateral one. Filed as
a request rather than done here.
