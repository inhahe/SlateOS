## B-A-TEST-HARNESS-THAT-CAPTURES-ONLY-FD-1-HIDES-THE-REASON-FOR-ITS-OWN-FAILURES (lane B, 2026-08-29) — ✅ FIXED 2026-08-29

**In short:** `oils`' test helpers collected what the shell printed, but not
what the shell *complained* about. So when eight tests went red under load, the
harness reported `left: ""` — "the builtin produced nothing" — while the shell
had in fact written `osh: grep: command not found` to a stream nobody was
watching, and exited 127 like it should. The failure named the innocent party,
and cost a triage cycle establishing that `readonly -p` had not broken. The
harness now watches that stream and quotes it under the assertion.

Reported by lane C as
`requests/c-b-oils-tests-cannot-see-a-failed-spawn.md` (2026-08-16, deleted
that day, restored 2026-08-29 by lane A, answered the same day).

**The general shape, which is not about shells.** A test that captures one of
the two output streams and drops the other will, for any failure that the code
*explained on the dropped stream*, report the explanation's **absence** rather
than the explanation. That reads as "the feature stopped working", which is the
worst possible pointer: it aims the reader at the code that is behaving
correctly. The tell is an assertion whose `left` is empty or short and never
wrong — six of the eight here.

**Why the obvious fix was the wrong one.** "Capture fd 2 as well" reads as
"point the shell's fd 2 at a buffer" — for `oils`, a `StderrTarget::Buffer` as
the base of the stderr stack. That is a *semantic* change, not an observational
one: `Buffer` makes `/dev/stderr` report `SpecialSrc::Unavailable`, changes the
sink-identity comparison that decides whether a `2>&1` is a merge, and routes
every external child's fd 2 through a pipe-and-drain rather than a dup of the
real descriptor. The tests would then be exercising a path production never
takes, in the one file whose entire purpose is "what does the real shell do
here". **In a test harness, prefer an observer to a redirect** — tee the bytes
on their way to where they were already going.

**What landed.** `interp.rs` → `mod stderr_tee` (`#[cfg(test)]`): a thread-local
that records the single `else` branch of `Shell::emit_stderr_depth` (the one
place shell diagnostics reach the process's real fd 2), plus a *chained* panic
hook that prints the recording beneath libtest's own assertion block. No call
site changed, so all ~88 external-spawn sites and every other test in the file
are covered at once. Cleared in `new_shell`, so a failure reports its own run's
diagnostics and a `--test-threads=1` run does not attribute one test's stderr
to the next. Rendering is a separate `report()` with its own assertions: a
quiet fd 2 prints nothing (or the block would appear under every unrelated
failure), and a non-text byte in a quoted filename costs that byte rather than
the rest of the line.

`the_harness_sees_what_the_shell_writes_to_the_real_fd_2` asserts the negative
too — a diagnostic that a `2>&1` took into the capture must **not** appear in
the recording, or the panic message would quote back a message the test had
deliberately collected itself.
