## 865. A parser that knows where the frame ends hands that back; it does not make the caller rebuild it

**Date:** 2026-09-21 &middot; **Decided by:** Claude (autonomous) &middot; **Lane:** C

**In short:** verifying a Wi-Fi handshake message means re-hashing exactly the
octets the sender hashed, and those are "the header plus however many octets
the header says" — not "the whole buffer the driver handed you", which is
longer because network cards pad short packets out to a minimum size. Working
that range out was left to each caller, and both of the two callers that exist
got it wrong at least once. `KeyFrame::parse_frame` now returns that range
alongside the parsed message, so nobody computes it twice.

**What was wrong with the old shape.** `KeyFrame::parse` took a *body* — the
frame with its four-octet header already stripped. A caller therefore had to
hold two slices of the same buffer at once (the body it parsed, and the frame
it hashed), and derive the second from the first by hand:

```rust
let body = eapol::body(frame)?;
let key = eapol::KeyFrame::parse(body, mic_len)?;
let frame_len = eapol::HEADER_LEN.checked_add(body.len())?;
let hashed = frame.get(..frame_len)?;      // the MIC-covered octets
```

Four steps, and in this lane a fourteen-line comment underneath explaining
which slice was which. The kernel lane had the same four steps in its
authenticator. That is one quantity computed independently in two trees, and
the failure mode is silent: getting it wrong yields a MIC mismatch, which is
indistinguishable from a wrong passphrase.

**The decision.** `parse_frame(frame, mic_len) -> Option<ParsedFrame>` where
`ParsedFrame { key, hashed }`, and `hashed` is derived from the same read of
the header that produced the body — in fact the body is now a sub-slice of
`hashed`, so there is no arithmetic left that could make them disagree.

**Struct or tuple.** Lane A proposed the shape and had no preference between
`(KeyFrame, &[u8])` and a named struct. The struct was chosen because a tuple
names the dangerous quantity only where it is destructured, and the place the
mistake gets made is the `verify_mic` call some lines later. `parsed.hashed`
carries the name to that line; `.1` does not. The cost is a second public type
for a two-field pairing, which is real but small.

**Why `parse` was kept rather than replaced.** Its one remaining caller is in
`kernel/`, which this lane must not write. Deleting it here would have made the
next merge to `main` red for all three lanes to save one line of ceremony.
Lane A offered it either way; the version with no red window was taken. The
deletion is the second half of an agreed two-step and is theirs to trigger.

**The Packet Type check, and the limit of it.** `parse_frame` also refuses a
header whose Packet Type is not `KEY`. Lane A suggested this partly to catch a
*body* mistakenly passed to a frame parser, on the grounds that the octet a
body puts where the Packet Type lives is always `0x00`. That is not so, and
this crate's own frames are the counterexample: message 4 and group message 2
carry `KEY_MIC | SECURE`, whose high byte is `0x03` — which *is*
`packet_type::KEY`. The check was still taken, because it rejects a genuine EAPOL-Start or
EAPOL-Logoff outright, which is its own justification. But it closes four of
six shapes, not six, and the other two are still caught only by the length
overrun. The doc comment says which, and a test named for the gap pins it, so
that a future reader tightening this function knows which check is actually
load-bearing.

**The alternative not taken** was a version check to close the rest. Rejected:
a body's first octet is the Key Descriptor Type, `2` for RSN, which is also a
*valid* EAPOL version — so it would not catch an RSN body anyway — and
refusing unknown versions outright trades a real interoperability rule for a
partial confusion check. Only a type distinction between a frame and a body
closes it properly, which `&[u8]` cannot express.

**How the error in the justification was found,** because the method
generalises: the check was deliberately deleted to see which tests noticed.
One of the two tests written for it passed without it — its name said
"refused by the packet_type check" and the length overrun was doing the work.
Explaining why only one test depended on the check required working out what
the octet really holds, which is what produced the counterexample. The test
was renamed to what its assertions can see. This is dd-855's relative for
controls: **a test named after a mechanism should fail when that mechanism is
removed, and the cheapest way to find out is to remove it.**
