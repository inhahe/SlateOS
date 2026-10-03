## 715. `ere` gained a real `REG_NOTBOL` rather than `ed` faking one, and a decode-once searcher rather than a second empty-match policy baked into an iterator

**Date:** 2026-08-30 · **Decided by:** Claude (autonomous)
**Lane:** B

**In short:** `ed`'s search-and-replace command, `s`, can be told to replace
*every* occurrence on a line (`s/old/new/g`). Some patterns match the empty
string — `a*` matches "zero or more a's", which is satisfied by nothing at all
— and a program that replaces "nothing" and then looks again at the same spot
would run for ever. `sed` and `ed` solve that differently, and ours had `sed`'s
answer. Fixing it needed a switch in the regular-expression library that we did
not have: a way to say "`^` (start of line) has already been passed; it must
not match again". These two decisions are about building that switch properly
instead of simulating it inside `ed`.

The bug: GNU `ed` answers `,s/a*/X/g` on `alpha` with `?` and *Infinite
substitution loop*; ours answered `XlXpXhX`. Measured against GNU ed 1.20.1.
The rule GNU actually follows is: substitute, advance past what was consumed,
search the rest with `REG_NOTBOL` set — and if *that* search matches empty at
the very position it started from, give up on the command. See
`known-issues.md` → `TD-B-ED-HAS-NO-REGULAR-EXPRESSIONS`, point 4.

### Decision 1 — `ere::StartOfLine::No` is a real engine flag, not an `ed`-side trick

`REG_NOTBOL` is the POSIX flag for "the first character of this subject is not
the beginning of a line", which is exactly what a resumed `s///g` search needs:
without it, `s/^x*/X/g` on `alpha` finds a *second* empty match at offset 0 and
is reported as a loop, where GNU prints `Xalpha`. `ere` had no such flag.

**The alternative considered and rejected** was an `ed`-local emulation:
search a copy of the line with one sentinel newline byte in front of it and
shift every span back by one. That is provably equivalent for this engine —
`$` is unaffected, and a word boundary before the first character is decided the
same way by "no character" and by "a newline", both being non-word — and it
needed no library change at all. It was rejected because it is the shape the
project's rules name outright: a band-aid that copies the line on every
substitution, encodes a proof about the engine's internals in a caller that
cannot see them, and would silently rot the day `ere` gained a multi-line mode
in which a newline *is* a line start. The cost of doing it properly was one
`Copy` enum threaded through five private functions and one `if` at each of the
two places `Inst::AssertStart` is decided.

**Why an enum rather than a `bool`:** `capture_spans_from(line, at, false)`
does not say which way round the flag runs, and this is a flag whose two
readings differ by a data-corrupting bug rather than by a formatting nicety.
`StartOfLine::No` reads at the call site.

**Cost accepted:** the flag exists on exactly one public entry point, and every
other one passes `StartOfLine::Yes`. That is deliberate — `grep`, `sed`, `awk`
and `expr` have no use for it, and an API where every search takes a flag would
make four callers pay attention to a distinction only one of them has.

### Decision 2 — `Regex::search` returns a reusable searcher; the walk stays the caller's

`ere` already had two ways to step through a subject, and neither fits:
`capture_spans_at(text, from)` re-decodes the whole subject on every call, so a
global substitution on a long line is quadratic in its length; and
`capture_spans_iter(text)` decodes once but **hard-codes one empty-match
policy** — advance one character — which is precisely `sed`'s rule and
precisely what `ed` must not do. Using the iterator is what produced the bug:
it is not that `ed` used it carelessly, it is that the iterator's contract is
somebody else's answer to the question `ed` was asking.

So the third option: `Regex::search(text)` hands back a `Search` that owns the
decoded subject and answers `capture_spans_from(at, bol)` any number of times.
Decoding happens once; the walk — including what to do about an empty match —
belongs to the caller, which is the only party that knows.

**The alternative considered and rejected** was a second iterator, e.g.
`capture_spans_iter_ed`, or an iterator parameterised by an empty-match policy
enum. Rejected because the policy is not a closed set: `ed` does not merely
"stop" on an empty match, it stops *only on a pass after the first* and reports
a specific error, which is not a policy an iterator can express without also
owning the error type. A policy enum would have grown a variant per caller.

**Cost accepted:** one more public type in `ere`, and `capture_spans_at` is now
a one-line delegation to it. The alternative reading — that `Search` duplicates
`CaptureMatches` — is answered by what they do *not* share: `CaptureMatches`
decides where the next search starts, `Search` is told.
