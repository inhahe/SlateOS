## §292 — `base64 -d` matches GNU's leniency about trailing bits, and its own encoder about line breaks

**Date:** 2026-08-25
**Decided by:** Claude (autonomous)
**Lane:** A

**In short:** When you decode base64, some inputs are technically malformed but
still decodable. We had to pick how fussy to be. The rule chosen: refuse
anything that would make us *guess* what the sender meant, and accept
everything that has exactly one possible meaning — even where the standard says
we may refuse it. Concretely, `Zh==` still decodes to `f`, and a base64 file
with line breaks in it still decodes without extra flags.

**Background.** Base64 encodes three bytes as four characters from a 64-letter
alphabet, padding the last group with `=` when the input did not divide evenly.
Decoding is mechanical, but real inputs arrive with line breaks in them, with
stray characters, and occasionally with bits set that the padding claims are
not there. Every decoder has to say what it does with each.

The rewrite of `cmd_base64` (`TD-A-BASE64-D-DESCRIBES-ITS-OUTPUT-INSTEAD-OF-WRITING-IT`)
forced the question, because the old decoder answered it by skipping all ASCII
whitespace unconditionally and checking almost nothing else.

**The three calls.**

| case | chosen | why |
|---|---|---|
| **Trailing bits** — `Zh==`: `h` carries four bits that one decoded byte cannot hold | **Accept**, discarding them, and decode to `f` | RFC 4648 §3.5 says a decoder *may* reject this; GNU, OpenSSL and Python's `binascii` all accept it. There is exactly one plausible intent, and refusing input that every other base64 accepts is a gratuitous incompatibility in a format whose entire purpose is interoperation. |
| **Line breaks** — `\n` and `\r` inside the data | **Skip**, always, with no flag needed | They are structure, not content: our own encoder inserts a `\n` every 76 columns, and MIME wraps with CRLF. A decoder that cannot read its own encoder's output is not a decoder. GNU's skips `\n` for the same reason; `\r` is added here because CRLF-wrapped base64 is at least as common as LF-wrapped, and no encoder emits a bare `\r` as data. |
| **Anything else outside the alphabet** — a space, a `!`, a byte ≥ 0x80 | **Refuse**, with `base64: invalid input` and exit 1, unless `-i` was given | This is where the old behaviour was actually wrong rather than merely lenient. Skipping a stray byte means decoding a *different message* from the one that arrived and reporting success for it. `-i`/`--ignore-garbage` exists precisely so the leniency can be asked for. |

**Alternatives considered.**

- **Canonical-form checking (reject `Zh==`).** More correct in the narrow
  sense, and it would catch a class of corrupt input. Rejected because the
  input is unambiguous, so refusing it buys no correctness for the *data* —
  only for the encoder that produced it, which is not our business — while
  costing compatibility with every base64 in existence. A user whose file
  decodes everywhere else and fails here would reasonably call that our bug.
- **Skip only `\n`, as GNU does.** Rejected as a gratuitous failure mode: a
  CRLF-wrapped MIME body would need `-i`, and `-i` would then also mask real
  garbage in the same file. Widening to `\r` costs nothing, since no encoder
  emits a lone `\r` as payload.
- **Keep skipping all ASCII whitespace.** This is the old behaviour, and it is
  the one that was actually harmful: it is not leniency about *structure* but
  about *content*, it could not be turned off, and it made `base64 -d`
  succeed on inputs that were not base64 at all.

**What this could cost later.** If a caller ever needs strict canonical
decoding — verifying a signature over the encoded form, say — this decoder will
not provide it. The fix then is a `--strict` flag checking the trailing bits in
`base64_group`, which is three lines and one branch; it is not a design that
would have to be unwound. That is why the lenient default is safe to choose
now.

**Where:** `kernel/src/kshell.rs`, `base64_decode` and `base64_group`. Pinned by
self-test rung 40.
