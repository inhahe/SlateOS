### TD-OILS-PRINTF-INTERLEAVE. `printf` error/warning ordering vs stdout — 2026-07-20 — ✅ RESOLVED 2026-07-20

**What:** bash writes `printf`'s stdout *line-buffered* (flushed at each newline)
and its stderr unbuffered, so a per-argument diagnostic lands after the complete
lines produced so far but *before* any pending partial line. osh previously
emitted **all** diagnostics before **any** stdout, so under `2>&1` the merged
order was wrong: `printf '%d\n' 1 bad 2` gave `error, 1, 0, 2` instead of bash's
`1, error, 0, 2`; `printf '%d %d %d\n' 1 bad 3` and `printf 'x%dy' foo` differed
similarly.

**Fix:** `format_printf` now tags each error/warning with the byte offset in the
output at which it arose (threaded via the new `PrintfDiags` struct;
`field_off = base + out.len()`). `builtin_printf` merges errors+warnings+fatal
into one offset-sorted stream and `printf_output_segments` splits the output at
the last newline at-or-before each diagnostic — reproducing bash's line-buffer
flush boundary. Verified against bash 5.2.37 across single-call multi-arg,
partial-line, multi-error-per-line, overflow-warning, and fatal-conversion
cases. Unit test `printf_diagnostics_interleave_with_stdout` covers the split
logic directly (the `run` harness captures stdout/stderr into separate buffers,
so it cannot exercise true interleaving).
