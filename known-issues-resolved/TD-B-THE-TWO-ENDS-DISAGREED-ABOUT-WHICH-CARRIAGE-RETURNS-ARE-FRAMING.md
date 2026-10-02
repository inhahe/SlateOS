### TD-B-THE-TWO-ENDS-DISAGREED-ABOUT-WHICH-CARRIAGE-RETURNS-ARE-FRAMING

**Status: FIXED** (`userspace/sshwire/src/lib.rs`; both ends now call it).

**In short:** the SSH client and the SSH server each decided for themselves where
the peer's greeting line ended, and they decided differently. A greeting with a
carriage return in the middle of it would have been read as two different pieces
of text by the two programs — and that text is one of the eight things both ends
must hash identically for the connection to work at all. The connection would
have failed reporting a bad host-key signature, i.e. reporting an attack.

**What it was.** A line ends `\r\n`; the `\r` is framing and comes off. Both ends
knew that and implemented it separately:

| | sshd, `read_version_line` | ssh, `version_exchange` |
|---|---|---|
| CR handling | dropped **every** `\r` in the line (`if b != b'\r' { line.push(b) }`) | dropped only the trailing one |
| length limit | 255, counting neither CR nor LF | 1024 |
| decode | `String::from_utf8`, error on failure | `String::from_utf8`, error on failure |

So for a peer sending `SSH-2.0-a\rb\r\n`, sshd would hash `SSH-2.0-ab` and ssh
would hash `SSH-2.0-a\rb`. Different `V_C`/`V_S` ⇒ different exchange hash ⇒ the
host key signature does not verify ⇒ the session dies, saying only that the
signature was bad. The length limits disagreeing is a second, milder split: a
253-to-1022-byte identification line is legal to one end and fatal to the other.

**Why it never fired.** Same shape as the two before it. RFC 4253 §4.2 requires
the identification string to be printable US-ASCII, so no conforming peer sends
an interior CR, and no real peer sends a 300-byte greeting. Every path that
distinguishes the two implementations is a path only a non-conforming or hostile
peer takes — which is exactly why it survived: it is unreachable from any test
either suite would think to write, and reachable from the wire.

**How it was found.** By reading `read_version_line` and `classify_version_line`
next to each other, immediately after fixing
`TD-B-SSH-RE-ENCODED-THE-SERVER-VERSION-BEFORE-HASHING-IT` in the second of
those. The first fix had touched only the client, which left the obvious
question — *and does the server do it the same way?* — whose answer was no.

**The fix — and why it is not a patch to sshd's loop.** Making sshd strip one CR
would have made the two agree today and left them free to disagree tomorrow,
which is the whole failure mode this stack keeps reproducing. The derivation
moved into `sshwire` instead, next to the hash that consumes it:

- `strip_line_terminator` — removes the CR of a CRLF, that one and no other.
- `is_identification_line` — the line is the one beginning `SSH-`, terminator
  stripped first so `SSH-…` cannot be missed for having a CR.
- `decode_identification` — applies §4.2's 255-byte limit *including the CRLF*
  (the RFC counts them; the old sshd limit did not) and decodes strictly.
- `MAX_IDENTIFICATION_LINE` — that limit, named once.
- `IdentificationError` with a `Display` that spells the reason out, because it
  reaches a user as the reason a connection was refused.

Nine tests in `sshwire` pin the behaviour, including `only_the_last_cr_comes_off`
which is the divergence itself, and `the_length_limit_counts_the_crlf_the_rfc_counts`.

Both binaries now call these. What is left in each is only what is genuinely that
end's own: the client also accepts banner lines ahead of the identification line,
escapes them with `quoting::escape_unprintable` and bounds them at
`MAX_GREETING_LINE` (a memory bound over text the RFC does not bound at all, not
a protocol limit); the server accepts no banner at all, because §4.2 permits them
only from a server and reading one would let an unauthenticated peer write to
this server's log before identifying itself.

**What is still true after this.** As with the other two: the ends agree because
someone read them both, not because anything checks. See item 4 of
`TD-B-THE-SSH-WIRE-LAYER-IS-WRITTEN-TWICE-AND-NOTHING-MAKES-THE-TWO-COPIES-AGREE`.
