### TD-OILS-PRINTF-ERRORDER. `osh` printf emits all `invalid number` diagnostics *before* any output; bash interleaves them per format-cycle — RESOLVED 2026-07-27 (was: MINOR DEVIATION 2026-07-19)

**Resolved 2026-07-27.** `printf` already emitted its output and its
conversion errors in stream order (`printf_output_segments` / `PrintfSeg`);
what remained was that a `> file` target was re-opened, and so re-truncated,
on every write, which silently dropped every piece of that stream but the
last. A builtin's redirect is now held open for its whole run
(`BuiltinStdout`, `Shell::write_redirected`), so both `printf '%d' 5 bad 7
2>&1` and the same into a file match bash. Covered by
`tests/corpus/printf-formats.sh` and
`a_builtin_writing_in_pieces_keeps_its_whole_output_in_a_file`.

**Where:** `userspace/oils/src/interp.rs` — `builtin_printf` (~7975) calls
`format_printf`, which builds the *entire* output string and collects every
numeric-parse error into a `Vec`; the builtin then writes all errors to stderr
and finally writes the accumulated stdout in one `write_bytes`.

**What:** `printf "%d\n" 0x1f 010 0b101` in bash prints `31`, `8`, then the
`0b101: invalid number` error, then `0` — output and error interleave in
argument order. osh prints the error first, then `31 8 0`. Both streams are
internally correctly ordered; only the *cross-stream* interleaving (visible
only when stdout and stderr are merged, e.g. under `2>&1`) differs.

**Impact:** negligible. With separate stdout/stderr — the normal case — every
byte lands on the right stream in the right order, and printf's exit status is
identical. Programs do not rely on stdout/stderr interleaving of an error path.

**Proper fix:** make `format_printf` emit incrementally — write each format
cycle's bytes to `out` as it is produced and flush before writing that cycle's
error to stderr — instead of accumulating one String plus an error `Vec`. Only
worth doing if a real script depends on the interleaving.
