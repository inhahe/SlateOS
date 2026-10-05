## B-THE-TWO-MULTIBYTE-COUNTING-CALLS-DOCUMENTED-A-BEHAVIOUR-NEITHER-HAD (lane B, 2026-09-09) — FIXED same day

**In short:** the two functions that convert between ordinary and wide strings
both offer a "just tell me how big the answer would be" mode, used by passing
a null destination. Both documented it. Neither did it: asked to measure, they
answered zero, for every input.

**Where.** `posix/src/wchar.rs`, `mbstowcs` and `wcstombs`.

**What was wrong.** `mbstowcs`'s loop was `while dst_count < n`, and
`wcstombs`'s capacity check `if dst_off + enc_len > n` ran unconditionally.
Both bound the *writing*, which is right, and both also bound the *counting*,
which is not: C says `n` is ignored when `dst` is null. So the idiomatic
measurement — `f(NULL, src, 0)` — exited immediately and returned 0. Passing a
large `n` with a null `dst` worked, which is why the doc comments could look
true to anyone who tried it that way.

**How it was found, which is the part worth keeping.** Not by a test and not
by review: by writing `vswprintf`, whose first act is to measure the format
string. Its symptom was not an error but an **empty output** — a formatted
string with nothing in it and a return of 0, which is a perfectly plausible
answer for a caller to receive. Three of its seven tests failed and the
smallest one, `swprintf(buf, 4, L"abc")`, returned 0 instead of 3, which is
what made it findable at all.

**Both doc comments were already correct.** They said "if `dst` is null,
counts the total bytes needed" and "if `dst` is null, just counts characters"
— written when the functions were, and never true. This is the same shape as
the week's other findings, with one difference worth noting: the usual case is
a comment that *became* stale when the code moved beneath it. This one never
matched at all, and nothing in the tree called these functions the documented
way, so there was nothing to notice it until something did.

**Fixed** by making both checks conditional on `dst` being non-null, with
regression tests covering the counting form, the mirror-image asymmetry
(`mbstowcs` counts characters and so counts *short* on multibyte input;
`wcstombs` counts bytes and counts *long*), and a guard that the bounded
writing form still respects `n`.
