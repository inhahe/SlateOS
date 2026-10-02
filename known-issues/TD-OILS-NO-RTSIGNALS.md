### TD-OILS-NO-RTSIGNALS. osh's signal table stops at 31 — no realtime signals — OPEN 2026-07-27

**Where:** `userspace/oils/src/interp.rs` — `SIGNALS` and `NSIG`.

**What.** `kill -l` lists 31 signals; `kill -l 34` and `trap … RTMIN` are
refused. Linux bash lists 64, with `SIGRTMIN`..`SIGRTMAX` (32–64) and the
`RTMIN+n`/`RTMAX-n` spellings its `decode_signal` understands specially.

**Proper fix.** Extend `SIGNALS` past 31 and raise `NSIG` to 65 (the
`nsig_is_one_past_the_last_real_signal` test enforces they move together), then
teach `decode_signal` the `RTMIN+n`/`RTMAX-n` arithmetic. Gate the numbering on
the target: the realtime range is platform-defined, and SlateOS has not fixed one
yet — which is the real blocker, not the parsing.

**Impact.** None today: SlateOS has no realtime signals to deliver, so the table
would be listing names that mean nothing. Deliberately kept out of the corpus,
whose signal cases only use the numbers every Unix agrees on.
