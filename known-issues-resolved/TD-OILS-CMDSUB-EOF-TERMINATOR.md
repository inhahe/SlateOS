### TD-OILS-CMDSUB-EOF-TERMINATOR. A command substitution body is parsed as if its `)` were a line end — 2026-07-28 — ✅ RESOLVED same day

**Where:** `userspace/oils/src/lexer.rs` (`Lexer::paren_body`,
`tokenize_paren_body`, the tail of `Lexer::run_into`, `Seg::ProcSub`) and
`userspace/oils/src/parser.rs` (`parse_cmdsub_body`, `parse_procsub_body`,
`parse_tokens_ending`).

**What (was):** a `$( … )` body was re-lexed and parsed as a standalone program,
so the lexer's implicit trailing newline (see TD-OILS-CASE-PATTERN-EOF) landed
where the `)` was. bash parses the body with `)` as the following token, and `)`
is not a `list_terminator` — so a construct that may legally end at a line end
may *not* end at the `)`. osh accepted it, or named the wrong token:

```sh
echo $( ! )      # bash: syntax error near unexpected token `)'   osh: printed nothing
echo $(for)      # bash: … near `)'                osh: … near `newline'
```

Note `( ! )` — a real subshell, parsed inline — already reported bash's error;
only the substitution form diverged. Found while giving `!` and `time` their
`<prefix> list_terminator` productions.

**Fix:** the body is lexed by `tokenize_paren_body`, which closes the stream
with a `Tok::Op(Op::RParen)` instead of the implicit newline every other input
ends with, and `parse_tokens_ending` treats that final `)` as the end of the
program rather than as a leftover token. That is the enclosing-stream model in
effect: the `)` ends the body's list — `$(echo x)` and `$(echo x; )` are both
fine, needing no terminator of their own — while remaining a real token that no
production accepts as a `list_terminator`, so the `BANG list_terminator` /
`timespec list_terminator` productions have nothing to match. Both halves fall
out of one change: `$( ! )`, `$(!)`, `$( ! ! )`, `$(time)`, `$( time -p )` and
`$( echo x; ! )` are now the syntax errors bash reports, and `$(for)`/`$(case)`
name the `)` rather than `newline`. The backtick spelling keeps the newline —
bash lexes that body on its own and accepts `` `!` ``.

The earlier write-up's second option ("suppress the implicit newline … only
repairs the wording") was wrong on both counts: suppressing it is not enough
(the newline is *real* in `$( !\n)`), and closing with the `)` changes which
programs are accepted, not just how the errors read.

**Process substitution had the identical divergence** (`cat <( ! )`,
`cat <(for)`, `cat <( time )`) and is fixed by the same lexing — hence the
neutral `paren_body` name. Its body is parsed by the new `parse_procsub_body`.
Two adjacent bugs surfaced while wiring it up, both pre-existing:

- The body's error line was the body's own (`line 1`), because `Seg::ProcSub`
  recorded no source position at all. It now carries the 1-based line the
  `<(`/`>(` opens on — shifted by `shift_segs` and mapped by
  `CmdSubLineMap::rebase_segs` like `Seg::CmdSub`'s close line — and
  `parse_procsub_body` shifts the body's numbering back into the enclosing
  source's. bash blames the line the body *stopped* on (`cat <(\nfor\n)` on
  lines 3–5 → line 4), which this now reproduces.
- With the line wrong, the offending source line was not echoed at all under
  `eval`. It is now.

**Coverage:** `tests/corpus/cmdsub-paren-terminator.sh` (109 matched, 0 failed)
and `parser::tests::cmdsub_body_is_terminated_by_its_closing_paren`. One
residual divergence, which is a bash bug — see TD-OILS-CMDSUB-BODY-RESERIALISED
below.
