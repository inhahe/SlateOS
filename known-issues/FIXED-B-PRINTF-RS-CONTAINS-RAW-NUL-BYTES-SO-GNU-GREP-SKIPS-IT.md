## FIXED-B-PRINTF-RS-CONTAINS-RAW-NUL-BYTES-SO-GNU-GREP-SKIPS-IT (lane B, 2026-08-20)

**What.** `posix/src/printf.rs` contains 47 literal `0x00` bytes, inside byte-
string literals in its test module: `fmt_f(b"%.1f\x00", 8.25)` is written with a
real NUL byte rather than the `\0` escape. This is valid Rust and the tests pass
— but GNU `grep` applies a binary-content heuristic, sees the NULs, and reports
`Binary file posix/src/printf.rs matches` instead of the matching lines. With
`-l` it is listed; with a normal content search its lines are invisible.

**Why it is worth an entry.** A search that silently returns nothing is the same
class of false-green this file is full of. An agent grepping `posix/src` for a
symbol defined in `printf.rs` gets no lines back and may reasonably conclude the
symbol is not there.

**Actual exposure is small**, which is why this is low priority and not fixed:
ripgrep — the tool this project's agents are told to use for searching, and
which the Grep tool wraps — handles the file correctly and returns matches
normally. Verified both ways. Only bash-invoked `grep`/`git grep` are affected.

**Proper fix.** Replace the 47 raw NUL bytes with `\0` escapes. Purely
mechanical, no behaviour change (`b"%.1f\0"` and `b"%.1f<NUL>"` are the same
bytes to rustc), and it makes the file plain text again. Not done here only
because it touches 47 test lines in a file already carrying a large diff for
the archive-granularity work, and mixing a mechanical byte-level rewrite into
that diff would make both harder to review.

**Resolved 2026-08-20**, once the archive-granularity diff (§339/§340) was
committed and the deferral reason expired. All 47 bytes replaced with `\0`;
the file grew by exactly 47 bytes and contains no `0x00`, which is the whole
of the change. `grep -c fmt_f posix/src/printf.rs` now answers `74` where it
previously answered `Binary file posix/src/printf.rs matches`.
