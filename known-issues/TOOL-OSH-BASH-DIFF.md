### TOOL-OSH-BASH-DIFF. `scripts/osh-bash-diff.py` — the osh-vs-bash differential harness — 2026-07-27

Every fidelity gap fixed above was found the same way: write a snippet, run it
under real bash, run it under osh, eyeball the difference. That loop was
entirely manual, so it was only ever run *once* per bug and never replayed after
a later change. `scripts/osh-bash-diff.py` institutionalises it: each case in
`userspace/oils/tests/corpus/*.sh` is run by both shells in a fresh temporary
cwd and stdout/stderr/status compared. `# STDIN: <text>` feeds stdin;
`# EXPECT-DIFF: <reason>` waives a known divergence — and the run **fails** if a
waived case starts matching, so a stale waiver cannot hide a fix.

Two traps it already had to defuse, worth knowing before touching it:

* It picks the **newest** `osh` binary by mtime, not the first one that exists.
  The first draft preferred `target/…/release/osh.exe`, which was a week stale,
  and duly reported 14 false failures against week-old behaviour.
* Cases are written with `write_bytes`, not `write_text`: on Windows the latter
  translates LF→CRLF, and the two shells disagree about stray CRs, so every
  multi-line case diverged spuriously.

Reference bash on the dev host is MSYS bash, which itself differs from Linux
bash in a few documented places (C-locale byte-wise string ops, signal
numbering, exit 127 for fatal expansion errors) — see the TD-OILS-* "probe
artifact" entries below. Cases hitting those need an `# EXPECT-DIFF` waiver.
