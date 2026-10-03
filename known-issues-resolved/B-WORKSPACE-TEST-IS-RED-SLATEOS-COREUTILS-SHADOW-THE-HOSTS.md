## B-WORKSPACE-TEST-IS-RED-SLATEOS-COREUTILS-SHADOW-THE-HOSTS (lane B's tree; filed by lane C, 2026-08-16)

**Status: ✅ FIXED 2026-08-16 by lane B** (`378c71b37`, `051ee45e7`) —
`userspace/oils/src/hostpath.rs` strikes cargo's two injected directories out
of `$PATH`, so the scaffolding tools resolve to the host's. The full
write-up, including the coreutils gaps this uncovered and where they were
fixed, is
`B-THE-OILS-TESTS-RESOLVED-grep/sed/cat-FROM-THE-CARGO-BUILD-DIRECTORY`
further down this file.

**This heading said `Status: OPEN` until 2026-08-21**, five days after the fix
landed. The stale status is worth a line of its own because this entry's own
"Correction, same day" note below argues that a wrong statement in a shared
file is worse than no statement — and then the entry became one. It is the
predictable failure of recording a fix in a *new* entry and leaving the
original untouched: three lanes read this file, the bug is described here, and
nothing here pointed at the resolution. When you fix something that has an
existing entry, amend that entry; a second entry elsewhere is a cross-reference,
not a substitute.

Kept below as filed, because the diagnosis is the useful part and it was right.

**Originally filed as** `requests/c-b-workspace-test-red-slateos-coreutils-shadow-host.md`.
Logged here because it blocked *every* lane's pre-merge gate, not just lane C's.

**What.** `cargo test --workspace --target x86_64-pc-windows-gnu` is
reproducibly red: `-p oils --lib` reports `1488 passed; 8 failed`. Run on its
own, `cargo test -p oils --lib` passes all 1496.

**Why.** Cargo prepends the build's output directory to `PATH` when it runs a
test binary — that is how a test finds its crate's dynamic libraries on
Windows. A *workspace* build puts SlateOS's own coreutils in that directory
(`target/x86_64-pc-windows-gnu/debug/{grep,sed,cat}.exe`, and ~200 more). The
eight failing `oils` tests each pipe through `grep`, `sed` or `cat`, so under
`--workspace` they run **ours** rather than the host's GNU ones — and ours do
not implement what the tests were written against. In a target directory where
coreutils was never built, the host's tools win and the tests pass.

**Proof.** The *same binary* (`osh-c93d43afff5c6245.exe` — identical hash in
both target directories, so identical features and flags), three of the eight
tests, one environment variable:

```
$ ./target-test/…/deps/osh-c93d43afff5c6245.exe <three tests>
test result: ok. 3 passed; 0 failed
$ PATH="$PWD/target/x86_64-pc-windows-gnu/debug:$PATH" ./target-test/…/<same>
test result: FAILED. 0 passed; 3 failed
```

And the utility by hand: `printf 'declare -r a="1"\n' | ./target/…/grep.exe
' [ab]='` matches nothing, where GNU `grep` matches. Ours has no bracket
expressions; the log also carries `grep: unknown option: -E`, `-q` and `--`,
and a `cat: E9: The system cannot find the file specified. (os error 2)` whose
wording is Rust's `io::Error` rather than GNU's.

**Why it is worse than one red crate.** Cargo stops at the first failing test
binary, so `osh` failing means every test target after it alphabetically —
`p` through `z` — **never runs** on any workspace test anyone does.

**Proper fix** (lane B's to make; the request lays out the fork). Either point
the `oils` test harness at the host's utilities explicitly, so the result stops
depending on what else the workspace happened to build — cheapest, and makes
the suite deterministic at once — or implement the missing coreutils features,
which is work we owe anyway and turns these eight into a real integration test
of our own tools. Both, in that order, is the recommendation.

**Correction, same day.** This entry first said the eight were a *load-related
flake* (transient spawn failure under a busy machine). That was wrong. It came
from a real observation — a clean re-run of `-p oils --lib` was green — but the
re-run had quietly changed the only variable that mattered, by using a
different `CARGO_TARGET_DIR`. Recorded because a wrong diagnosis in a shared
file is worse than none: the next lane would have re-run it, watched it pass in
isolation, and believed the note.

**Also, unrelated but learned alongside:** never run two `cargo test
--workspace` invocations against one `target/`. The first attempt died with
`os error 32` — "could not execute process colorpicker-….exe … being used by
another process" — with nothing actually under test at the point of failure.
