### TD-OILS-PRINTF-TREATED-EVERY-NEGATIVE-TIME-AS-NOW. `%(fmt)T` took any negative argument for a sentinel, never truncated the rendered time to a precision, and had no `%U`/`%W` — 2026-08-04 — ✅ FIXED 2026-08-04

**Where:** `userspace/oils/src/interp.rs` — the `%(…)T` arm of the printf
formatter and `format_strftime`.

**What:** three divergences in what happens *after* strftime has produced its
bytes.

* Only `-1` (now) and `-2` (shell start) are sentinels. Everything else
  negative is a real pre-epoch time: `-3` is 1969-12-31 23:59:57, `-86400` is
  1969-12-31 00:00:00, `-2208988800` is 1900-01-01. osh answered "now" to all
  of them.
* bash renders the time and then lays the result out *as a string*, so a
  precision truncates it by bytes the way it truncates `%s`: `%.2(%Y-%m-%d)T`
  is `19`, `%.0(%Y)T` is empty, and `%012.6(%Y-%m-%d)T` is `      1970-0`.
* `%U` and `%W` were simply absent. They are the plain week counts —
  `(yday0 + 7 − wday) / 7` and `(yday0 + 7 − ((wday+6) mod 7)) / 7` — which
  share neither the ISO week `%V` nor its year `%G`. 2006-01-01 is a Sunday, so
  it is `%U` 01 but `%W` 00.

**Fixed** in `e225565f6`, pinned by `only_minus_one_and_minus_two_are_time_sentinels`,
`the_plain_week_counts_are_neither_iso_nor_each_other`, the `%(…)T` assertions
in `a_precision_truncates_the_rendered_b_and_q`, and the corpus case
`printf-lays-a-time-out-as-a-string.sh`.

**Standing lesson:** when a conversion produces text, ask which of the format's
flags apply to the *value* and which apply to the *text it became*. `%T`'s
precision is the second kind, and so is `%s`'s — which is why the fix was two
lines once the question was put that way.
