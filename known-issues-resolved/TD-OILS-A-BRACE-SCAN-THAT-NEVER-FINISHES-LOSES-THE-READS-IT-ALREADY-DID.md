### TD-OILS-A-BRACE-SCAN-THAT-NEVER-FINISHES-LOSES-THE-READS-IT-ALREADY-DID. A word whose `${` or `$(` is never closed reports nothing from the extent scan — 2026-08-10 — ✅ FIXED 2026-08-14 (`userspace/oils/src/lexer.rs`, `userspace/oils/src/interp.rs`)

**Where:** `userspace/oils/src/interp.rs` — `Shell::brace_extent_scan` and the
`extent_read_of*` family it drives, and the parser that has to produce a `Word`
before any of them run.

**What is wrong.** bash's `extract_dollar_brace_string` (subst.c:1874-1960) is a
**single forward pass that reports as it goes**. Each `$( … )` it meets is handed
to `extract_command_subst` *at the point the scan reaches it*, so a body that
does not parse has already been reported by the time the scan discovers, later,
that the word it is scanning has no closing `}` — or that a subsequent `$(` runs
off the end of the string. The failures of the earlier reads are not retracted.

osh inverts the order: `brace_extent_scan` is reached only from an expansion of
an already-parsed `Word`, so a word that never parses runs no reads at all and
every report the scan would have made is lost. Two shapes show it.

**(a) A later `$(` that is never closed.** The first read fails and is reported
by bash; osh drops it, and then diverges again on what the unterminated second
one leaves behind:

```sh
z=ZZ; v='A${z:-p$(fi
q)r$(for
s}B'; printf '[%s]\n' "${v@P}"
```

| | bash 5.2.37 | osh |
|---|---|---|
| reports | `` near unexpected token `fi' `` **then** `` near `for' `` | `` near `for' `` only |
| then | — | `fo: command not found` (the unterminated body is *run*) |
| value | `[AZZB]` | `[A⏎s}B]` |

Two separate faults here: the lost `fi` report, and — because osh's parser
recovers differently from a `$(` with no `)` — an unterminated body that gets
executed and a brace that never yields its value. bash's scan runs off the end
of the string looking for the `}` and still hands back the operand's value.

**(b) An unterminated `${` in the remainder.** The read that already failed is
reported by bash before the word is condemned; osh emits only the condemnation:

```sh
z=ZZ; v='A${z:-p$(fi
q)r${w'; printf '[%s]\n' "${v@P}"
```

| | bash 5.2.37 | osh |
|---|---|---|
| reports | `` near unexpected token `fi' `` **then** `A${z:-p$(fi⏎q)r${w: bad substitution` | the `bad substitution` only |
| value | `[A${z:-p$(fi⏎q)r${w]` | same |

Only the diagnostics differ in (b); the value already agrees.

**Both are pre-existing** — verified by `git stash` against the read-1
continuation fix of 2026-08-10, which neither caused nor touched them. They are
a *different* rule from that fix: the continuation is about a read that failed
inside a word that is otherwise well-formed, whereas this is about a scan that
never reaches its own end.

**What the proper fix looks like.** The reports have to be emitted by the
**scan**, not by an expansion downstream of a successful parse. That means the
extent scan must run over the word's *source text* at the point the lexer meets
the `${`, in the same forward pass that is looking for the `}` — which is what
bash does and is a genuine restructuring, not a patch: today `brace_extent_scan`
takes a `&WordPart` and presupposes a parse. Doing it properly means the brace
lexer growing the scan, reporting each `$( … )` as it steps over it, and only
afterwards deciding whether the word is well-formed. That would fix (a)'s and
(b)'s lost reports together, and is also the natural place to make (a)'s
unterminated body stop being executed.

**Not urgent:** both shapes need `@P` to be visible at all (any other context
takes the `longjmp` on the first failure), and both are malformed input.

**Re-measured 2026-08-14, and the framing above is wrong in a way that makes
the fix smaller — read this before the text above.** "A word that never parses
runs no reads at all" is not what happens, and the two shapes are not two
faults. Taken apart one read at a time (`build/pgV.sh`), osh is **exact** on
everything except one shape:

| word (inside `v='…'`, shown via `"${v@P}"`) | bash | osh |
|---|---|---|
| `A$(for⏎sB` — no brace at all | reports, runs `fo`, `[Apr⏎sB]` | **same** |
| `A${z:-pr$(for⏎s)q}B` — brace, `$(` closes | reports, `[AZZB]` | **same** |
| `A${z:-pr⏎sB` — unclosed brace, no `$(` | `bad substitution`, raw text | **same** |
| `A${z:-P1$(for⏎S1}B` | reports, `[AZZB]` | reports, **runs `fo`**, `[A⏎S1}B]` |
| `A${z:-P1$(echo hi⏎S1}B` | reports EOF, `bad substitution`, raw text | reports EOF, **runs `echo hi` *and* `S1}`**, `[Ahi]` |
| `A${z:-P1$(for⏎S1B` | reports, `bad substitution`, raw text | reports, **runs `fo`**, `[A⏎S1B]` |

So the fault is exactly one thing: **a `$( … )` inside a `${ … }` that never
closes**. There osh applies the *string-level* rule — `failed_extent_split` /
`run_abandoned_extent`, "run what was read less a byte and splice the rest" —
where the brace-level rule applies. The lost `fi` report of shape (a) is a
*consequence* of that, not a separate lost-report bug: each read reports
correctly on its own, and the first one's report is lost only because the
second one drags the word onto the string-level path.

**This is worse than "a diagnostic is missing".** Row 5 runs `echo hi` — a real
side effect — and then `S1}` as a command, where bash runs nothing at all and
answers `bad substitution`. Malformed input in a *prompt* is attacker-adjacent
(`PS1`, `${x@P}`), so treat this as the reason to fix it rather than as a
formatting nicety.

**Both halves of the correct behaviour are already written; neither is
reached.** bash's `extract_dollar_brace_string` hands a nested `$(` to
`extract_command_subst`, i.e. a *real parse*, and the two rows above are its
two outcomes — which osh already distinguishes, in
`Shell::brace_extent_scan`'s `ExtentRead::Abandoned` arms
(`userspace/oils/src/interp.rs`):

- the parse **errors part way** (`for⏎`), so `si` stops at that line, the brace
  scan resumes after it and finds the `}` — `Abandoned { rest, .. } if
  !rest.is_empty() => false`, the brace closes and expands, `[AZZB]`;
- the parse **runs the string out** (`echo hi⏎…`), so `si` is past the end and
  the brace has no `}` left — `Abandoned { .. }` with an empty rest, which sets
  `extent_consumed` and emits `bad substitution`.

`wordscan::edbs` independently models the *second* outcome on pure source text
(its `$(` arm at `wordscan.rs:458` paren-counts and `break`s to `Err` on
overrun, which `begin_word` turns into the diagnostic). It gets row 5 right for
the wrong reason and would get row 4 wrong, because a paren count is not the
parse: `$(for⏎S1}B` has no `)` at all, so the count overruns where bash's parse
stops early and lets the brace close.

**✅ FIXED 2026-08-14, in two halves.** Routing alone could not work, and that
is the one thing the paragraph this replaces had wrong: *where the scan
resumes is only knowable from a real parse*, so the lexer has to ask for one.

**Half one — the lexer stops swallowing the string** (`lexer.rs`). A new
`Lexer::unread_comsub_stop` runs `crate::parser::comsub_unclosed_error` — the
pure model of `xparse_dolparen` — over the rest of the text **for its stopping
point only**, keeps the consumed text as raw bytes, and leaves the enclosing
scan looking for its own delimiter. The cut it makes in char space is
`Shell::failed_extent_split`'s in byte space: past the stop-th newline, then
back over the whole trailing run of newlines. Three call sites converted from
`?` to a match on `Lexer::unread_comsub`: `read_dollar_brace_body`'s `$(` arm
and its `<(`/`>(` arm.

`read_opaque_span`'s arithmetic `$(` arm was converted too and then **reverted**
before landing. `build/pgX.sh` had suggested the `$((` spelling was affected
identically, but the corpus case
`an-unterminated-construct-in-text-no-parser-read-is-a-runtime-failure`
disagrees: an arithmetic span in unread text is reached only where the first
failure takes the jump — a here-document body sets no
`no_longjmp_on_fatal_error` — so the read's diagnostic is the whole report and
there is no second act for a resumption to reach. Condemning the `$((` instead
regressed that case. What `$((` needs is half *two*, not half one; see
`TD-OILS-AN-ARITHMETIC-SCAN-REPORTS-NONE-OF-THE-READS-IT-MAKES` below.

**Half two — the reads are emitted before the condemnation** (`interp.rs`). A
scan that ran the text out reported everything it read *on the way*; only then
is there a brace with nothing to close on. `Shell::unclosed_brace_reads` runs
`extent_read_of_rest` over the undecoded body from the top of
`expand_unclosed`'s `Unclosed::BadSubst` arm, gated on `close == '}'` because
`$[`'s `extract_arithmetic_subst` passes flags `0` (subst.c:1299) and makes no
reads at all. An `ExtentRead::Aborted` returns early — the jump stands and the
brace's own ending is never reached.

**Measured after: `build/pgW.sh`'s seven rows are byte-identical to bash
5.2.37**, stderr included — including shape (b) above, whose lost `fi` report
half two restores. Every spurious command execution is gone: rows that used to
run `echo hi`, `S1}` and `fo` now run nothing, as bash does.

**Still open, and logged separately below:** the `$((` spelling reports none of
its reads (`TD-OILS-AN-ARITHMETIC-SCAN-REPORTS-NONE-OF-THE-READS-IT-MAKES`); a
`<(` in an undecoded brace body is not read
(`TD-OILS-AN-UNDECODED-BRACE-BODY-IS-RE-LEXED-AS-A-DOUBLE-QUOTED-RUN`); and
`$[ … ]` bounds do not perform their `$( … )`
(`TD-OILS-A-DOLLAR-BRACKET-BOUND-DOES-NOT-PERFORM-ITS-COMMAND-SUBSTITUTION`).
