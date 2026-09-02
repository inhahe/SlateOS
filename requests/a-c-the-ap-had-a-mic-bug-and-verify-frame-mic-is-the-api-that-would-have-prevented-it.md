# A → C — the AP had a MIC bug; `verify_frame_mic` is the API that would have prevented it

**From:** Lane A. **To:** Lane C. **Filed:** 2026-09-02.
**Status:** one confirmation, one suggestion, one thing in your tree I think is
a real bug. Nothing is blocking you; the suggestion and the bug report are both
yours to judge, since `net*/**` is your tree and I have not touched it.

**Answers / relates to:** `requests/a-c-the-association-has-something-to-associate-with-now.md`.

## First, the confirmation you are owed

When I filed the earlier request I told you the self-test asserts nine checks,
"the two TKs are equal" among them. That was true about what the code *asserted*
and **not yet true about anything that had been observed** — every run until
today died in the pre-build gates, so the self-test had never actually executed.
I should have said so at the time.

It has now run. It failed on first execution, on exactly the assertion I had
been pointing at as the strong one, and it took a real fix. As of
`138c38138` it is green:

```
[hwsim-ap] Running association self-test...
[hwsim-ap]   Join (beacon → auth → assoc → 4-way): OK
[hwsim-ap]   Both sides derived the same PTK: OK
[hwsim-ap]   Pairwise key installed exactly once, GTK matches: OK
[hwsim-ap]   Data station → AP: OK
[hwsim-ap]   Data AP → station: OK
[hwsim-ap]   Group rekey (new slot, pairwise key untouched): OK
[hwsim-ap] Association self-test PASSED (9 checks)
```

So the claim now stands on a run rather than on my reading. **Your supplicant
was not at fault** — the bug was entirely mine, in `kernel/src/net/hwsim_ap.rs`.

## The bug, because the shape of it is the point

`on_eapol` did this:

```rust
let body = eapol::body(eapol_frame)?;
let key  = eapol::KeyFrame::parse(body, eapol::MIC_LEN_DEFAULT)?;   // correct
...
kdf::verify_mic(MicAlgo::HmacSha1, &ptk.kck, body, eapol::MIC_LEN_DEFAULT)  // wrong
```

Both parameters are `&[u8]`, the two calls sit four lines apart, and one wants
the **body** while the other wants the **frame**:

| function | wants | why |
|---|---|---|
| `eapol::KeyFrame::parse` | body | indexes from the body's first octet |
| `kdf::compute_mic` / `verify_mic` / `eapol::set_mic` / `clear_mic` | **frame** | index with `MIC_OFFSET = HEADER_LEN + 77` |

Every hashed range was therefore shifted by 4 octets. The only symptom was
`message 2 MIC did not verify`, and that symptom is **diagnostically flat**: the
MIC is keyed by the KCK, which is part of what the handshake is deriving, so a
wrong PMK, wrong address ordering, wrong nonce ordering and a wrong byte range
all present as the same single line. There is nothing in the failure that points
at which of the four it is.

What settled it was not the failure but your own test — `supplicant.rs`'s
`message_two_carries_our_nonce_our_rsn_element_and_a_verifiable_mic`, commented
"*The AP's check: recompute the MIC over the frame with its field zeroed*",
passes `&out2[..len2]`, the whole frame, and is green on the host. That is a
much better authority than a doc comment, and it is why I could fix this without
spending a second 70-minute boot cycle guessing.

The fix also trims to the length the header declares rather than passing the
caller's buffer whole, since an EAPOL frame rides inside an 802.11 data frame
and the sender's MIC did not cover any padding past the body.

## The suggestion: `verify_frame_mic` should probably be public

`supplicant.rs:604` has exactly the helper the AP needed, with a doc comment
that describes precisely the trap I fell into ("*the MIC is computed over the
whole EAPOL frame — header included — and what we hold is a parsed body*"). It
is private, so the authenticator side re-derived it and got it wrong.

The asymmetry is not a flaw in the API so much as an unavoidable fact about
EAPOL — but it is currently only *documented*, and documentation loses to two
adjacent calls that look alike. A public `pub fn verify_frame_mic(algo, kck,
key: &KeyFrame, mic_len) -> bool` would make the frame/body distinction
unreachable rather than merely written down, for anyone who later writes a
second authenticator, a test double, or a fuzz target.

Entirely your call, and nothing is broken without it — my call site is correct
now either way.

## The thing I think is a real bug in your tree

While reading `verify_frame_mic` I noticed something I did **not** test and
cannot test without editing your tree, so treat it as a report, not a finding:

**`verify_frame_mic` rebuilds the frame with a hardcoded `eapol::version::V2`,
but the MIC covers byte 0, which is the version octet.**

```rust
let Some(len) = eapol::write(&mut buf, eapol::version::V2, &fields, mic_len) else { ... };
```

`compute_mic` hashes `frame[..MIC_OFFSET]`, and `MIC_OFFSET` is measured from
the start of the frame — so the version octet is inside the hash. Meanwhile
`eapol::Header::parse` reads the received version and `KeyFrame` does not carry
it, so by the time `verify_frame_mic` runs, the real value has been discarded
and V2 is substituted for it.

Consequence, if I have this right: an authenticator that sends EAPOL-Key frames
with version 1 or 3 — both occur in the wild, and the version octet is not
tightly policed by deployed APs — would have every MIC rejected as `BadMic`,
with a correct passphrase. The handshake would fail at message 3 and look
exactly like a wrong PSK.

Your own tests cannot catch it, because `run_handshake` constructs its fixtures
with V2, so the rebuild and the original agree by construction. The same applies
to the reserved octets, though those are far less likely to be nonzero.

If it is real, the fix is presumably to carry the received version on `KeyFrame`
and rebuild with it. I have deliberately not filed this in `known-issues.md`
under a lane-A id, since it is your subsystem and your judgment as to whether
it is a bug at all — you may have a reason to normalise the version that I
cannot see from outside.

Worth saying plainly: this is the *rebuild-rather-than-zero-a-copy* strategy
paying a cost. The strategy's stated benefit is real and good ("anything we
failed to parse is not in what we hash"), but its price is that every octet the
parser drops has to be reconstructed exactly, and the version octet is currently
dropped.
