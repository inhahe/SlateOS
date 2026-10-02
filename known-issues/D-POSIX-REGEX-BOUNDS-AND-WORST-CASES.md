## D-POSIX-REGEX-BOUNDS-AND-WORST-CASES — the rewritten `regcomp`/`regexec` bounds two things glibc does not, and has inputs that cost it quadratic time (lane D, 2026-09-30) — **Status: OPEN (limits, by design; the costs measured)**

**In short:** the new regular-expression engine (`posix/src/regex.rs`,
design-decisions §1160) answers every case of its oracle as the standard
does. It is not unbounded, though, and some patterns cost it more than they
should. None of this is a wrong answer on an input that fits; each is where
it stops, or slows.

- **A program past two million instructions is refused** (REG_ESPACE from
  `regcomp`). Bounded repetitions are written out, as glibc writes them
  out: `(a{1000}){1000}` is a million copies. glibc stops only when
  `malloc` does.
- **A back-referencing match past four million table entries answers
  REG_NOMATCH**, as glibc answers a `regexec` whose memory ran out. Typical
  patterns are nowhere near it -- `(.*)\1` over 4,000 bytes, `(a|b)*\1`
  over 8,000 (5.8 s in glibc, measured) are near-linear -- but a pattern
  built so that every position leaves a different set of group spans can
  reach it.
- **Quadratic in the span, not linear:** taking apart a repetition whose
  body has variable width runs the body forwards once per iteration, and a
  body whose threads live long (`(a|a*b)*` over a long run of `a`) makes
  each run long; a bounded repetition holding a group keeps a table of
  span x count bits (capped at `min + span` counts); and a back-reference
  pattern whose relaxed form matches at every start but whose exact form
  does not retries each start.
- **No lazy DFA.** The search is a Thompson simulation, the program's size
  a byte; glibc caches DFA states, and is faster on long subjects for big
  patterns without submatches.

**The fixes, if these ever bite:** a lazy DFA for the search and for
`dissect.rs`'s forward runs (states cached per byte, as glibc and RE2 do);
the iterations of a variable-width repetition found from one backward and
one forward pass rather than one pass each; the back-reference engine's
states keyed on only the spans a later back-reference can still reach.

**Where:** `posix/src/regex/prog.rs` (`MAX_INSTS`), `posix/src/regex/backref.rs`
(`MAX_ENTRIES`), `posix/src/regex/dissect.rs`.
