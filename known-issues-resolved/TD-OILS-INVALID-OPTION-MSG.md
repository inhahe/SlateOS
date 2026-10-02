### TD-OILS-INVALID-OPTION-MSG. `osh`'s unknown-invocation-option error differs from bash (wording + no usage dump, and reports the whole token not the offending letter) — ✅ RESOLVED 2026-07-20

**Where:** `userspace/oils/src/main.rs` — the leading-option `while` loop's
short-cluster arm and the `Some(other)` invocation-mode dispatch arm, plus the
new `eprint_option_usage()` helper.

**What (was):** for an unrecognised leading option, bash prints `bash: -z:
invalid option` followed by a `Usage:` block, then exits 2. osh printed a single
line `osh: unrecognized option '-z'` and exited 2. For a *cluster* containing an
unknown letter (`-xz`), bash processes the valid letters first and then errors on
the specific bad letter (`-z: invalid option`); osh reported the whole token
(`unrecognized option '-xz'`) without applying the leading valid letters.

**Fix (2026-07-20):** the short-option cluster arm now parses getopt-style —
valid letters (`set` options, `-i`, and mode letters `-c`/`-s`) are applied
left-to-right, and the first unrecognised letter aborts with `osh: -<letter>:
invalid option` (the offending *letter*, keeping its `-`/`+` sign) followed by a
concise osh `Usage:` summary, exit 2. The `Some(other)` dispatch arm (unknown
`--long` options) uses the same `<opt>: invalid option` wording + usage. As a
bonus, bare `-` now ends option processing and reads commands from stdin with the
operands as positional parameters (matching bash's legacy spelling), where it
previously errored. Regression coverage: `tests/cli_options.rs`
(`unknown_option_reports_invalid_option_and_exits_2`,
`invalid_letter_in_cluster_reports_the_offending_letter`,
`plus_sign_unknown_option_keeps_its_sign`, `unknown_long_option_reports_invalid_option`,
`bare_dash_reads_commands_from_stdin`).

**Residual:** osh's `Usage:` text is osh-flavored (names `osh`, not the host bash
path) and shorter than bash's full GNU-long-option dump — exact byte-parity of
the usage block is neither possible (different program name) nor desirable.
