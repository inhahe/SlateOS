# B → C: the password manager's CSV export must survive any password

**Status:** OPEN
**From:** lane B. **Date:** 2026-09-27.
**Source:** the operator's answer to lane B's B-Q12 (relayed verbatim by lane
F's session on 2026-09-27), which opens with a remark about lane C's C-Q25.

## In short

While answering a question about the shell, the operator added a warning that
belongs to the password manager's plain-text export (C-Q25, option A: a `.csv`
of every password). Passwords can contain anything, including every character
CSV uses to separate or quote fields. The operator asked that the export not
get that wrong. Lane B is passing it on because it was said in lane B's queue
and may not have reached lane C's.

The operator's words:

> This reminds me, the proposed format for exporting passwords from the
> password manager in plaintext was to do it in csv, but passwords can have any
> characters, including any combination of characters used to delimit a string
> or escape a character in csv, so make sure you don't mess that up.

## What "not messing it up" takes, concretely

RFC 4180 is enough, applied without exceptions:

* **Quote every field** that contains a comma, a double quote, a carriage
  return or a line feed -- or simply quote every field, which is never wrong.
* **Double every double quote** inside a quoted field (`a"b` → `"a""b"`).
* **Write fields as bytes, not trimmed or normalised text.** Leading and
  trailing spaces are part of a password; so are a lone `\r`, a tab, a NUL if
  the store allows one (if it does not, say so in the export rather than
  dropping it), and invalid UTF-8 if the store holds bytes.
* **Line endings between records are CRLF** (the RFC), and a CR or LF *inside*
  a quoted field stays exactly as it is.
* **Test by round trip**: export, re-import (or parse with a strict RFC 4180
  reader), and compare byte-for-byte, over passwords built from exactly the
  dangerous characters -- `,`, `"`, `""`, `\r`, `\n`, `\r\n`, a leading `=`
  (see below), spaces at either end, and a field that is only a quote.

One more worth knowing, though the operator did not raise it: a field that
starts with `=`, `+`, `-` or `@` is run as a formula when the CSV is opened in a
spreadsheet ("CSV injection"). For a password export the right answer is
usually *not* to alter the field -- that would change the password -- but to
say in the export warning that the file should not be opened in a spreadsheet.

Nothing here is lane B's to change; it is yours to fold into C-Q25's
implementation however you see fit.
