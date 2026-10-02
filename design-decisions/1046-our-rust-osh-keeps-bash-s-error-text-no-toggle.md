## 1046. Our Rust osh keeps bash's error text; no toggle

**Date:** 2026-09-27
**Lane:** B
**Decided by:** Operator (answering B-Q12; Claude recommended D. The operator
leaned toward C, then judged it overkill once genuine Oils is the default
(§1043), so osh is left as it is). Relayed verbatim through lane F's session.
*Claude's reading of an answer that weighs C and then sets it aside; if C was
meant, say so and it is a small change.*

**In short:** our Rust shell prints error messages exactly as bash does,
including names a user typed, unquoted -- so a name holding a newline can
make one error look like two. The rest of the tree quotes such names. The
operator decided the Rust shell need not grow a safer mode: genuine Oils
becomes the default (§1043), and a user who switched to ours and hit this
can switch back.

**The operator's answer, verbatim:**

> This reminds me, the proposed format for exporting passwords from the
> password manager in plaintext was to do it in csv, but passwords can have
> any characters, including any combination of characters used to delimit a
> string or escape a character in csv, so make sure you don't mess that up.
> Anyway, to answer the question, maybe C, but it seems like it may be
> overkill since we're making the real Oils the default, and if the user has
> switched theirs to our Rust Oils and runs into a problem, they can simply
> temporarily use the real Oils instead.

**What follows:** the 16 exemptions in `scripts/quote-names.py`'s IGNORE
table now point here instead of at an open question. The password-manager
remark is for the lane that owns the exporter, and has been passed to it:
a CSV writer must quote every field that holds a comma, a quote, a CR or an
LF, double every quote inside a quoted field, and be tested with passwords
built from exactly those characters.
