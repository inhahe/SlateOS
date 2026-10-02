### TD-OILS-PRINTF-ERRORDER. `printf` emits all invalid-number errors before its stdout, not interleaved — RESOLVED 2026-07-27 (was: MINOR 2026-07-19)

**Resolved 2026-07-27.** `printf` already emitted its output and its
conversion errors in stream order (`printf_output_segments` / `PrintfSeg`);
what remained was that a `> file` target was re-opened, and so re-truncated,
on every write, which silently dropped every piece of that stream but the
last. A builtin's redirect is now held open for its whole run
(`BuiltinStdout`, `Shell::write_redirected`), so both `printf '%d' 5 bad 7
2>&1` and the same into a file match bash. Covered by
`tests/corpus/printf-formats.sh` and
`a_builtin_writing_in_pieces_keeps_its_whole_output_in_a_file`.

**Where:** `userspace/oils/src/interp.rs` — `builtin_printf` (~8381) and
`format_printf` (~13806). `format_printf` builds the *entire* output string and
collects every per-argument "invalid number" diagnostic into a `Vec`; the
builtin then emits all errors to stderr and writes all stdout separately.

**Symptom:** under a combined stream (`printf … 2>&1`), the ordering differs
from bash, which interleaves per conversion. Example — `printf "%d\n" 5 bad 7`:

```
bash: 5 / <error> / 0 / 7        (error appears between the 5 and the 0)
osh:  <error> / 5 / 0 / 7        (all errors precede all output)
```

Values and exit status match; only the stdout/stderr interleaving differs, and
only when both streams are merged. (The error line also carries the ERRLINE
`bash: line N:` prefix osh omits — see TD-OILS-ERRLINE — so the lines differ
regardless.)

**Why deferred:** exact parity needs `format_printf` to emit output and errors
*as it processes each conversion* (a callback/event-stream restructure of a
heavily-used, well-tested function) rather than returning a finished string plus
a separate error list. A partial reorder (write stdout, then errors) would move
osh to "output-then-error" but still not match bash's mid-stream interleaving, so
it is not a clean parity win. Cosmetic-only; left as-is.

**Proper fix:** restructure `format_printf` to write each formatted chunk and any
conversion error in stream order (e.g. take `&mut impl Write` for stdout plus an
error sink, or yield an ordered `Vec<PrintfEvent>`), done alongside ERRLINE so the
whole merged stream matches.
