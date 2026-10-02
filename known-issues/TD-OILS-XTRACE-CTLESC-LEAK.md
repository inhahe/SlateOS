### TD-OILS-XTRACE-CTLESC-LEAK. bash doubles a literal 0x01 when tracing a compound assignment — 2026-07-30 — WONTFIX (bash bug, not replicated)

**Where:** `userspace/oils/src/interp.rs` — `xtrace_compound_quote`; noted in
`tests/corpus/xtrace-array-decl.sh`.

**What.** For `set -x; declare -a c=($'a\001b')` bash 5.2 traces
`+ c=('a\001\001b')` — the byte appears twice — while the value it stores is the
correct one-byte `$'a\001b'`. 0x01 is bash's internal `CTLESC` marker; the
compound-assignment printer emits the escaped form without stripping it. Only 0x01
is affected: `\002`, `\033` and `\177` all come out once.

osh prints the byte once, which is what the value actually is. Not replicating a
leaked internal escape: it would mean carrying bash's CTLESC representation into
osh's expansion pipeline purely to reproduce a display bug. Recorded so the
divergence is not mistaken for an osh defect later, and so the corpus omission is
explained.
