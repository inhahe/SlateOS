### TD-OILS-ARGV-PANICS-ON-NON-UTF8. `osh` aborts at startup if any of its own arguments is not UTF-8 — FIXED 2026-07-30

**Where:** `userspace/oils/src/main.rs:210` and `:224` —
`let args: Vec<String> = std::env::args().collect();`

**What:** `std::env::args()` **panics** on an argument that is not valid UTF-8
(the documented behaviour: it is the checked counterpart of `args_os`). So
`osh script.sh "$(printf 'a\xffb')"` does not run the script and mis-bind `$1`
— it kills the shell before it starts, with a Rust panic message rather than a
diagnostic. On SlateOS, where a filename may contain any byte but `/` and NUL,
the trigger is an ordinary `osh -c 'cat "$1"' -- <that filename>`.

This is strictly worse than the `from_utf8_lossy` corruption bugs the
byte-strings work has been removing: those produced a wrong answer, this one
produces no answer at all. It is also the *only* remaining non-UTF-8 input that
aborts rather than degrades — the environment already reads through `vars_os`
(see `interp.rs:3402`), and the value layer is byte-typed as of
TD-OILS-BYTE-STRINGS step 6.

**Fixed 2026-07-30**, ahead of its place in TD-OILS-BYTE-STRINGS step 9 — it is
separable (it depends on neither step 7 nor step 8) and it was the only
remaining input that *aborted* rather than degraded. `os_argv()` reads
`std::env::args_os()` and widens through `bytes::os_to_bytes`, and `run` /
`parse_long_options` take `&[Str]`. The option parsing stays text — an option
is a name, and every spelling in both tables is ASCII — by matching on
`bytes::as_str(arg)`: a word that does not decode matches no option, which ends
the option pass and makes it an operand, exactly what it is. Consequences that
came with it:

* `StartupFiles::rc_file` is `Option<BStr>` and `Shell::set_execution_string`
  takes bytes (both name/carry data, not text).
* `Plan::Script` opens the script through `bytes::bytes_to_path`, so a script
  whose *name* is not text now runs.
* `Plan::Command` (`-c`) still needs `str` — the parser is `str`-typed until
  step 8 — so a `-c` string that is not text is **rejected** with a diagnostic
  rather than approximated: running an *altered* command is the one outcome
  worse than running none.
* Diagnostics that quote an argv word are written by a new byte-level `ediag`
  helper rather than `eprintln!`, so the token printed is the token given.
* `Shell::set_name` (`$0`) takes bytes, but still narrows internally: `$0`
  doubles as the label `error_source` feeds to all 275 `err_prefix` sites. That
  is the one remaining seam here and it closes with TD-OILS-BYTE-STRINGS step
  10; the narrowing is deliberately confined to the single line that writes the
  field.

Regression tests live in `main.rs`'s own `mod tests` (they exercise
`parse_long_options`/`positional_args` with a `\xff` word — before the fix every
one of them would have aborted the process).
