## 576. The supplicant's defence against key reinstallation is a return-type variant, not a flag the caller is trusted to read

**Date:** 2026-09-01

**Lane:** C

**Decided by:** Claude (autonomous)

**In short:** When a WiFi station finishes negotiating with an access point,
it installs the encryption key it just agreed. If it ever installs that same
key a *second* time, the encryption silently becomes breakable — this is a
real published attack, KRACK. The trouble is that the access point routinely
re-sends the last message of the negotiation when it does not hear the reply,
so "install the key" and "you have already installed the key, just answer
again" arrive as *identical frames*. Something has to tell those two cases
apart. The decision is about where that distinction lives: in a boolean the
caller is expected to check, or in the shape of the value the caller gets
back — such that the code will not compile unless the caller has considered
both cases.

**Decision:** `Handshake::on_eapol` returns `Result<Outcome, Error>` where
`Outcome` is a three-variant enum:

```rust
pub enum Outcome {
    Reply { len: usize },           // send this; the handshake continues
    Complete { len: usize },        // send this, and install the keys
    Retransmission { len: usize },  // send this, and do NOTHING else
}
```

Only `Complete` authorises installing a key. A repeated message 3 — the
attack's vehicle — returns `Retransmission`, which carries a reply to send
and no permission to touch the keys.

**Why this is a decision and not an obvious call.** The alternative is
smaller and reads perfectly well:

```rust
pub struct Outcome { pub len: usize, pub install: bool }
```

That is one field instead of three variants, needs no `match`, and is the
shape most C supplicants use. It has a genuine advantage: a caller who wants
"just send the bytes" writes `out[..o.len]` with no ceremony at all, whereas
the enum makes every caller name three cases even when two of them do the
same thing.

The reason it lost is what happens to each shape under the mistake actually
being guarded against. With the boolean, forgetting `if o.install` is a
*missing* line — nothing in the type system, the compiler, or a code review
diff shows an absence. The resulting bug is invisible in testing, because a
supplicant that installs the key twice associates perfectly and passes every
functional test; the only symptom is that the traffic is decryptable by
someone listening. With the enum, the same mistake is a non-exhaustive
`match`, which is a compile error. The class of bug is converted from
"silent, security-critical, undetectable by testing" into "will not build".

That trade — a little ceremony at every call site, in exchange for making
one specific catastrophic error unrepresentable — is worth taking precisely
because the error is undetectable any other way. It would not be worth
taking for a mistake that a test could catch.

**Why the third variant, rather than just two.** `Retransmission` could have
been folded into `Reply`, since both mean "send this and continue". They are
kept apart because they mean different things to a caller that logs or
counts: a burst of `Retransmission`s is the signature of either a lossy link
or an attacker replaying frames, and a supplicant that cannot distinguish
them from normal progress cannot report either. The variant costs nothing —
a caller that does not care writes `Reply | Retransmission` in one arm.

**The escape hatch, and why it is not the primary API.**
`Outcome::installs_keys()` exists and returns the boolean. It is there for a
caller genuinely writing a dispatch table, and it is documented as the less
safe of the two spellings. Making it the *only* spelling would have been the
rejected design; making it available alongside the enum costs nothing,
because a caller who reaches for it has had to type its name and read its
doc comment, which is exactly the moment of attention the boolean-only
design never provides.

**Two smaller calls recorded here rather than separately, both with the same
character — the type refuses to let the caller be careless:**

- **The station's nonce is supplied by the caller, not generated.** `new`
  takes `snonce: [u8; 32]`. `net80211` is `no_std` and has no entropy source,
  and inventing one — a counter, a clock — would produce a supplicant that
  works in every test and derives a predictable key in the field. Requiring
  the caller to pass it makes the dependency on real randomness impossible to
  overlook. The doc comment says why, at the parameter.
- **Both RSN elements are borrowed slices, not parsed structures.**
  `Config` holds `sta_rsn_element: &[u8]` and `ap_rsn_element: &[u8]`.
  Message 3 must be compared against the beacon's element *byte for byte*;
  comparing parsed structures would compare this parser's opinion of two
  elements rather than the elements, and a downgrade hidden in a field the
  parser ignores would compare equal. The borrow also keeps the handshake
  allocation-free, but that is a secondary benefit, not the reason.

**Where it lives:** `net80211/src/supplicant.rs` — `Outcome`, `Handshake`,
`Config`. Tested by `a_retransmitted_message_three_is_answered_but_does_not_reinstall_the_key`,
`a_replayed_group_message_cannot_reinstall_the_group_key` and
`a_downgraded_rsn_element_in_message_three_is_caught`. The rationale is
repeated in the module docs, since that is where a driver author will read
it, and in `requests/c-a-the-wifi-handshake-is-written-and-has-nothing-to-run-on.md`,
since the driver is the code that performs the install.
