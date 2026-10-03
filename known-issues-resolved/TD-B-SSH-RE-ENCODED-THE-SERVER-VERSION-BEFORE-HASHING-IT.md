### TD-B-SSH-RE-ENCODED-THE-SERVER-VERSION-BEFORE-HASHING-IT

**Status: FIXED** (`userspace/ssh/src/main.rs`, `classify_version_line`).

**What it was.** The client read the server's identification line one byte
at a time into a `String` with `line.push(char::from(byte))`. `char::from(u8)`
is the *Latin-1* mapping, so every byte from 0x80 to 0xFF became the code point
of the same number — and came back out of `.as_bytes()` as that code point's
**two-byte** UTF-8 encoding.

That string is `V_S`, the second input to the RFC 4253 §8 exchange hash. The
server hashes the bytes it put on the wire. We would have hashed a re-encoding
of them, one byte longer, so the two exchange hashes differ, so the host key
signature does not verify, so the connection dies — reporting only that the
signature was bad, which is also what a real attack looks like.

**Why it never fired.** §4.2 requires the identification string to be printable
US-ASCII, and every server anyone has ever run obeys that. The bug was reachable
only from a non-conforming server, which is precisely the property that would
have kept it hidden indefinitely — the same shape as the placeholder `V_C` in
sshd (`TD-B-SSHD-SIGNS-AN-EXCHANGE-HASH-OVER-A-CLIENT-VERSION-THE-CLIENT-NEVER-SENT`),
which was likewise invisible to every test on either side.

**How it was found.** Not by a test. By reading both ends' version-line parsing
side by side while extracting `sshwire`, specifically asking whether the two
still agree on the bytes they feed the now-shared hash. The shared crate removes
the *hash* from the list of things that can drift; it says nothing about what
each end passes to it, and this was the first thing checked afterwards for
exactly that reason.

**The fix.** The line is accumulated as `Vec<u8>` and `classify_version_line`
decides what it is:

| Line | Then | Because |
|---|---|---|
| starts `SSH-` | `String::from_utf8`, strictly; error if it fails | it is `V_S`; a line we cannot reproduce byte-for-byte must be refused, never silently altered |
| anything before it | `quoting::escape_unprintable` | a banner is not hashed, and is unauthenticated bytes on their way to a terminal |

Being a free function rather than an inline block in `version_exchange` is what
makes it testable: the loop around it needs a socket, and this crate's tests do
not have one.

**A second, smaller bug fixed with it.** Banner lines were printed raw under
`-v`. A server that has authenticated nothing to anybody could therefore write
arbitrary control sequences to the user's terminal before the handshake even
started. They are now escaped.

**What was still true after this, and no longer is.** This fix established only
that the two ends agree; no test ran one against the other. That was
`TD-B-THE-SSH-WIRE-LAYER-IS-WRITTEN-TWICE-AND-NOTHING-MAKES-THE-TWO-COPIES-AGREE`
item 4, and it is now `userspace/ssh-interop` — the only thing that would have
caught either of these without someone happening to read the right two functions
on the same day.
