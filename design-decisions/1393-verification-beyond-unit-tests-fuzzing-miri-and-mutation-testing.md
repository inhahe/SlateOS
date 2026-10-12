## 1393. Verification beyond unit tests: every untrusted-input parser fuzzed, unsafe code under Miri, and mutation testing in the review

**Date:** 2026-10-12
**Lane:** F
**Decided by:** Operator ("ok, i'll go with your suggestion", in chat to lane
F on 2026-10-12, after lane F proposed these as the tools that catch what
reviews and unit tests miss; Claude proposed them)

**In short:** reviews find design-level problems, and unit tests check the
cases someone thought of. Three tools now catch the mechanical bugs both
miss:

| Tool | What it finds | Where | How it runs |
|---|---|---|---|
| Fuzzing (libFuzzer via cargo-fuzz) | panics, hangs and runaway allocation on inputs nobody wrote | every parser of untrusted input: decoders, network parsers, `sshwire`, the ELF loader, decompressors | `python scripts/fuzz.py run <crate>/fuzz` in WSL; a round when a parser changes |
| Miri | undefined behaviour in `unsafe` code: out-of-bounds, use-after-free, aliasing violations, data races | crates with `unsafe` code and host tests -- `posix` first | `cargo +nightly miri test -p <crate> --lib --target x86_64-unknown-linux-gnu -- <module>::` |
| Mutation testing (cargo-mutants) | changed code no test checks: break a line, and no test notices | every batch's changed Rust lines | in `scripts/review-batch.py`, after the reviews, at idle priority, half an hour at most |

**Measured when adopted (2026-10-12):**

- **Miri** found undefined behaviour in `posix`'s `malloc` on its first run
  (`requests/f-d-miri-finds-undefined-behaviour-in-malloc.md`).
- **Mutation testing** of `netproto`'s last change: 24 mutants in 58
  seconds, and one missing test (an echo reply into a buffer of exactly
  its size).
- **Fuzzing**, two-minute rounds of the new harnesses for `netproto`
  (41 million inputs) and `sshwire` (53 million): nothing found.

**Costs:** none in tokens, except triaging what they find. CPU: a fuzz round
loads every core WSL has for its length, and Miri runs tests 10-100 times
slower than native. Mutation testing costs a rebuild and the package's test
suite per mutant, so its budget caps it and the batch is sampled.

**Whose:** each lane runs these on its own crates. `scripts/fuzz.py` and
`review-batch.py`'s mutation round are lane F's. The first harnesses and
Miri findings went to their lanes as requests on adoption.
