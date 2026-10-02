## TD-A-FORMAT-SIZE-PRINTED-A-TWO-DIGIT-TENTHS — `format_size(2047)` read "1.10 KiB"

**In short:** the human-readable byte formatter used by the disk cleanup
tool computed the digit after the decimal point by dividing, which gives
**10** near the top of every unit. So sizes just under a boundary printed
with two digits after the point — `1.10 KiB` — which also reads as *larger*
than the `1.9 KiB` just below it.

**Lane A. Found and fixed 2026-08-23 (`11825e56d`), while diagnosing the
first boot panic from the newly-wired suites.**

`kernel/src/fs/storageclean.rs::format_size` used
`remainder / (unit / 10)`; `1023 / 100` is `10`. All three unit arms had
it. Now `remainder * 10 / unit`, which is in `0..=9` by construction, in one
shared helper.

**How it surfaced is the interesting part.** The boot panicked on the
suite's own `assert_eq!(format_size(512), "0.5 KiB")` — and that assertion
was simply wrong, since under a KiB the count is exact and belongs in bytes.
Reading the helper to decide which side to believe is what turned up the
real defect behind it. The assertion had never been executed in its life;
this is precisely the payoff argued for in TD-A-FS-SELFTESTS-NEVER-RUN. The
suite now carries the boundary cases (1023, 2047, one below a MiB) that
would have caught it.
