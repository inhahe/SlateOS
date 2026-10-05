### TD-OILS-PRINTF-ERRORDER. `printf` batches all "invalid number" diagnostics *before* its stdout, where bash interleaves each at the point of the bad conversion — RESOLVED 2026-07-27 (was: MINOR ORDERING DEVIATION 2026-07-19)

**Resolved 2026-07-27.** `printf` already emitted its output and its
conversion errors in stream order (`printf_output_segments` / `PrintfSeg`);
what remained was that a `> file` target was re-opened, and so re-truncated,
on every write, which silently dropped every piece of that stream but the
last. A builtin's redirect is now held open for its whole run
(`BuiltinStdout`, `Shell::write_redirected`), so both `printf '%d' 5 bad 7
2>&1` and the same into a file match bash. Covered by
`tests/corpus/printf-formats.sh` and
`a_builtin_writing_in_pieces_keeps_its_whole_output_in_a_file`.

**Where:** `userspace/oils/src/interp.rs` — `builtin_printf` / `format_printf`.

**What:** `format_printf` builds the *entire* output string while collecting all
per-argument numeric-parse errors into a `Vec`, and `builtin_printf` then writes
every error to stderr *first*, followed by the whole stdout string. bash instead
processes the format left-to-right, emitting each conversion's output and any
error as it reaches it. The difference is only observable when stdout and stderr
are merged (`2>&1`):

```
printf "%d\n" 0x1F 010 0b101 2>&1
# osh:  <error for 0b101>, 31, 8, 0     (error batched to the front)
# bash: 31, 8, <error for 0b101>, 0     (error at the 3rd conversion)
```

The numeric *values* are identical (31, 8, 0 — `0b101` is invalid in printf `%d`,
falls back to 0); only the diagnostic ordering differs.

**Proper fix:** stream printf output incrementally to the target fd, emitting each
argument's diagnostic to stderr at the moment its conversion is processed rather
than collecting them up front — i.e. thread the write/emit through
`format_conversion` instead of returning a fully-built `String` + `errors` Vec.
Deferred because it only surfaces under `2>&1` stream-merging and needs a
restructure of printf's single-write-at-the-end buffering model (which also
serves `-v` assignment, where there is no stdout interleaving at all).
