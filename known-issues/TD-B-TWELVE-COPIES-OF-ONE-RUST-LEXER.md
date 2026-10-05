## TD-B-TWELVE-COPIES-OF-ONE-RUST-LEXER (lane B, 2026-09-10) — three consolidated, one deliberately not

**In short:** twelve scripts under `scripts/` define a Rust comment/string
masker. Only one handled raw strings, so the bug fixed there — `r"a\"` ends at
that quote, because a backslash is not an escape inside a raw string — was live
in three others. `scripts/rustlex.py` is now the one implementation, with the
fixtures beside it. Three checkers converted; **one was reverted, and that is
the useful part.**

**Why one implementation.** The function has been wrong three separate times
across its copies: a char literal holding a quote (`rest.find('"')`, in thirty-odd
crates) opening a string that ran to the next quote anywhere later; a raw string
ending in a backslash swallowing its terminator; and lane A's `mask_noncode`
blanking string *bodies* when the pattern it measures **is** a string literal,
taking a real count of 37 to 0 with the gate green. Every one failed toward
silence.

**The one that could not be converted, and why it matters more than the three
that could.** `check-one-libc-per-process` reported a NEW error on
`posix/src/crypt.rs:470` — a line that is plainly inside
`pub extern "C" fn crypt`. The two lexers differ by exactly one observable:

    source   pub extern "C" fn crypt() { ... }
    local    pub extern " " fn crypt() { ... }     <- keeps the quotes
    shared   pub extern     fn crypt() { ... }     <- blanks them too

The checker finds its subject by matching `extern "`. Blanking the delimiters
destroys the marker it needs, so every `extern "C"` function became "not an
extern C function" and its contents became findings.

**Two functions with the same name, the same docstring and the same stated
job can differ in a detail one caller depends on, and nothing shows it until
you swap them.** That is an argument for consolidating carefully rather than
against consolidating — but it is why each conversion was verified by *diffing
the checker's output before and after* rather than by running it and seeing it
pass. Three produced byte-identical output. The fourth did not, and was
reverted rather than argued with.

**CORRECTION, next tick: "twelve copies of one lexer" overstated it.** Read
individually, the remaining eight are **three different jobs** whose names do
not distinguish them:

| script | job |
|---|---|
| `rustscan` | comments *and* literals — the same job. **Converted.** |
| `host-errmsg` | comments only, literals **kept on purpose** — it searches for message text *inside* strings |
| `check-variant-lists`, `count_centrings`, `getopt-ambiguity-check`, `rustemit` | `//` comments only |
| `check-absent-operand-default` (`mask_noncode`) | lane A's |
| `check-shell-callables` (`mask`) | shell, not Rust |

Converting `host-errmsg` would blank the literals it exists to read and take
its count to zero with its gate green — lane A's `mask_noncode` bug exactly, in
the opposite direction. So the real duplication was **two** implementations of
one job, not twelve, and both are now `rustlex`.

`rustscan` is a library four checkers import — three of them lane C's gates —
and its copy did not know raw strings, so that bug was live in all four. After
delegating, all four produce **byte-identical output**, so nothing lane C sees
changes today and the bug is closed for tomorrow. `rustlex` grew
`keep_literals` to take rustscan's signature, which is the parameter it should
have had: a masker that cannot be asked to spare literals invites each caller
to write its own.
