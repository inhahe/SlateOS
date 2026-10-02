## `apps/email`: every outgoing header was interpolated raw — header injection (FIXED)

**Status: FIXED 2026-08-15** (lane C). The most serious defect the output-escaping
audit has turned up, and the one whose consequence is least visible to the user.

`EmailDraft::build_message` wrote every header value straight into the message:

```rust
msg.push_str(&format!("Subject: {}\r\n", self.subject));
msg.push_str(&format!("To: {}\r\n", self.to.join(", ")));
msg.push_str(&format!("In-Reply-To: <{reply_to}>\r\n"));
msg.push_str(&format!("Content-Type: {}; name=\"{}\"\r\n", att.mime_type, att.filename));
```

RFC 5322 gives a header field no way to contain a line break. The field *ends*
at CRLF; folding — a CRLF followed by whitespace — is a continuation the
serialiser chooses, not something a value can request. So a CR or LF in a value
is not escaped, it **terminates the header**, and the receiving MTA reads what
follows as a header of its own.

A subject of `Lunch?\r\nBcc: mallory@evil.test` therefore adds a recipient. The
reason this is worse than an ordinary injection: **the forged Bcc appears
nowhere the sender can see it** — not in the compose window, which shows the
subject field as typed, and not in the Sent copy, which is rendered from the
same draft object. The mail silently goes somewhere the user cannot discover it
went.

### What was and was not reachable

Worth stating precisely, because the inbound side turned out to be sound and
that is a design worth not regressing.

- **Not reachable: anything parsed off the wire.** `Headers::parse` unfolds
  continuation lines into spaces, so no value read from a received message can
  carry a CR or LF. That closes what would otherwise be the nastiest path:
  `EmailDraft::reply` copies the original's `Message-ID` into `In-Reply-To`, so
  a hostile `Message-ID` would have been injected into the victim's reply with
  no interaction beyond pressing Reply. The unfolding is what prevents it, not
  anything at the serialiser, which is why the serialiser now sanitises anyway.
- **Reachable: everything composed locally** — the subject and recipients the
  user types or pastes, and attachment filenames. The filenames matter more
  here than on other systems: `design.txt` allows every byte except `/` and NUL
  in a path, so **a newline in a filename is legal on SlateOS**. A downloaded
  file can carry one, and attaching it forged headers.

### Also fixed: the boundary was a constant

The multipart delimiter was the literal `----=_Part_Boundary_001`. RFC 2046
requires the boundary to appear nowhere inside an encapsulated part, and a fixed
string cannot promise that. A body containing it — which a user produces just by
quoting a previous multipart mail — ends the part there, and **every attachment
below that point silently disappears from the sent message**. The boundary is
now derived from the body, lengthening on collision; this terminates because a
finite string contains no arbitrarily long substring, and the first candidate is
the old constant, so ordinary mail is byte-identical.

### The shape of the fix

Five helpers, chosen per field by what the grammar can express and by whether
the field is advisory or load-bearing — the reject-or-sanitise rule this audit
keeps arriving at, now on its fifth format:

| Field | Grammar offers | Treatment |
|---|---|---|
| `Subject`, display names | nothing | sanitise: control characters → space |
| `To`/`Cc`/`Bcc` | nothing | **reject and report** — a recipient decides where the mail goes, so a bad one must not be quietly rewritten into a different address |
| `Message-ID`, `Content-ID` | nothing | sanitise: drop controls, `<`, `>`, whitespace |
| attachment `filename` | `\"` and `\` inside a quoted-string | escape quote and backslash; drop controls |
| attachment `Content-Type` | nothing (it is a token) | **fall back** to `application/octet-stream` — a mangled media type is not a media type, so there is nothing to sanitise it *into* |

`build_message` now returns `BuiltMessage { text, rejected_recipients }` rather
than a bare `String`, for the same reason `export_m3u` returns skipped paths: a
dropped recipient is exactly what the sender must be told about, and a function
returning a `String` has nowhere to say it.

### Two lessons from the break-testing, not the fix

Breaking each defence in turn to check the tests notice found that **two of the
new tests could never have failed**, which is worth recording because both
mistakes are easy to repeat:

1. The header-scanning helper stopped at the first blank line — correct for the
   top-level block, but a **MIME part has its own headers after that blank
   line**, so the test for a forged header in an attachment filename was
   inspecting a region the payload never reached.
2. It split only on `\r\n`. Real receivers are lenient and many end a line at a
   bare LF, so a test that only recognises CRLF is *stricter than the attacker*
   and passes on genuinely vulnerable output.

Both fixed by scanning every line terminator across the whole message and
counting lines that *begin* with the header name. Counting line starts rather
than substrings is what keeps it honest in the other direction, and is the same
point the CSV and SQL tests reached: correctly quoted output legitimately
contains the payload text, so `contains` cannot be the assertion.

The display-name test still fails under no single break, because the value is
covered by two independent defences; breaking both together does fail it, which
is how it was confirmed to be defence in depth rather than a vacuous test.
