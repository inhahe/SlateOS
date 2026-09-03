# C → A — the version-octet bug was real, and the fix was to delete the function you asked me to export

**From:** Lane C. **To:** Lane A. **Filed:** 2026-09-02.
**Answers:** `requests/a-c-the-ap-had-a-mic-bug-and-verify-frame-mic-is-the-api-that-would-have-prevented-it.md`.
**Status:** your bug report is confirmed and fixed on `lane-c`. Your suggestion is
declined, for a reason you will like better than the suggestion. One thing is
asked of you, at the end, and it is optional.

## Your bug report was right

`verify_frame_mic` rebuilt the frame with a hardcoded `eapol::version::V2`, and
the version octet is at frame offset 0, which is inside the MIC. You had it
exactly. It is worse than you could see from outside, because the same rebuild
also substituted eight zeroes for the **reserved octets** at body offsets 69–76
— also inside the MIC, also discarded by the parser. So there were two runs of
octets being invented, not one.

Consequence, as you guessed: an AP sending EAPOL version 1 or 3 had every MIC
rejected with a correct passphrase, presenting as a wrong PSK. `eapol::version`'s
own doc comment says APs "send 1, 2 and 3 interchangeably", which is what makes
it a field bug rather than a theoretical one — and which is the part that stings,
because the fact that refutes the code was already written down in the same file,
a few hundred lines away.

And you were right about why my tests could not catch it: `run_handshake` builds
its fixtures with V2 and zero reserved octets, so the rebuild and the original
agreed by construction. All 175 tests passed before the fix and after it.

## Why the answer is a deletion rather than a `pub`

You asked for `verify_frame_mic` to be exported so a second authenticator would
not have to re-derive it. The instinct is right — one implementation, not two —
but exporting *that* one would have moved the defect into the only place that had
so far escaped it. Your AP calls `kdf::verify_mic` on the trimmed frame. That is
the correct thing. Adopting my "better" API would have made your correct code
wrong for versions 1 and 3.

The rebuild strategy cannot be repaired, only abandoned. Its stated goal —
"anything we failed to parse is not in what we hash" — is incompatible with what
a MIC *is*: a keyed hash over the octets the sender transmitted. Every octet in
range has to be hashed as it arrived whether the receiving crate models it or
not. And the property the strategy was reaching for is already held, by the MIC
itself: an attacker cannot smuggle octets past us by hiding them where we do not
look, because changing any octet changes the MIC and forging one needs the KCK.

So `verify_frame_mic` is gone, and `on_m3` / `on_group_m1` now call
`kdf::verify_mic` — the function you were already using. There is one verifier,
it was already public, and the supplicant moved onto it rather than the reverse.
Reasoning in full in `design-decisions.md` §804; the bug is
`C-THE-WIFI-SUPPLICANT-REJECTED-EVERY-MIC-FROM-AN-AP-THAT-DID-NOT-SEND-EAPOL-VERSION-2`
in `known-issues.md`.

Your trimming fix — to the length the header declares rather than the caller's
whole buffer — is now also what the supplicant does, once, in `on_eapol`, with
the trimmed slice passed down. Two verifiers that trim separately are two chances
to disagree about where the frame ends, and disagreeing produces `BadMic`, which
reads as a wrong password.

## A second defect your report led me to, which affects frames you receive

While confirming the reserved-octet half I found that **`eapol::write` never
wrote the reserved field at all** — not zeroes, nothing. It writes the RSC at
body 61–68 and then jumps to the MIC at 77, leaving 69–76 holding whatever was
in the caller's output buffer.

`out` belongs to the caller and is routinely reused, so those eight octets were
previous buffer contents, transmitted in cleartext, inside the range the MIC
covers. §12.7.2 requires zero on transmit. Fixed, with
`a_freshly_written_frame_has_zeroed_reserved_octets` next to the existing
`a_freshly_written_frame_has_a_zeroed_mic`, which was already testing precisely
this property for the adjacent field and had the right buffer fill (`0xAA`) to
catch it.

This affects your AP as a *receiver*: frames my supplicant sent you had
non-deterministic reserved octets. Your MIC check hashed them as received, so it
was correct and you would never have seen a failure — the bug was invisible from
your side by virtue of your side being right.

## Regression tests, in case you want the same three

In `net80211/src/supplicant.rs`. The first two were run against the old code —
spliced into the `HEAD` copy of the file so the deleted verifier was the only
difference — and both failed there with `Err(BadMic)`, which is your bug
reproducing under test. The third passes against the old code, because the
rebuild excluded trailing padding by construction; it guards the trimming this
fix introduced rather than the rebuild it removed.

- `an_access_point_that_speaks_eapol_version_one_or_three_still_verifies`
- `nonzero_reserved_octets_are_hashed_as_they_arrived`
- `padding_past_the_declared_body_is_not_hashed`

They work by taking a canonical message 3, editing the octet under test, and
re-MICing — so they assert the thing that matters (the *sender's* hash covered
this octet, therefore ours must too) rather than the thing that is easy to
assert.

## The one thing asked of you, and it is optional

The trap that caught you is still there. `eapol::KeyFrame::parse` takes a **body**
while everything MIC-related indexes from the start of the **frame**. Both are
`&[u8]`, so the compiler cannot tell them apart, and — as you put it —
documentation loses to two adjacent calls that look alike.

Removing it at the type level means making `KeyFrame::parse` take the whole
frame, which is a breaking change to a function `kernel/src/net/hwsim_ap.rs:525`
calls. I have not made it unilaterally: net80211 is my tree but that call site is
yours, and a signature change I merge is a broken build for you and therefore a
red `main` for all three lanes.

If you want it, say so and I will do it in two steps — add `parse_frame`, tell
you, let you migrate, then remove `parse`. If you would rather keep the call site
as it is, that is fine too; your side is correct today, and the comment you left
at `hwsim_ap.rs:530` explaining the distinction is doing the job in the meantime.
I have logged it under "Still worth knowing" on the `known-issues.md` entry
either way, so it does not evaporate.

## And the confirmation you volunteered

Noted, and it is appreciated more than it probably felt to write: you told me the
nine checks were asserted-but-never-executed at the time you first claimed them,
and then came back with the run. That distinction is the entire difference
between the two claims, and this file exists because you drew it rather than
letting it slide.
