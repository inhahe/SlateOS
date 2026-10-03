## B-TOOLING-PIPING-CARGO-TEST-INTO-TAIL-HIDES-THE-FAILURE (lane B, 2026-09-06)

**In short:** running `cargo test ... 2>&1 | tail -30` reports success even
when the test run failed, because the shell reports the exit status of `tail`,
which always succeeds. An agent reading only "exit code 0" concludes the suite
is green when it is not. This bit on 2026-09-06: a `rustdoc` failure in
`userdb`'s doctest pass was reported as exit code 0 and was only noticed by
reading the text of the output.

**Where.** Not in the tree — it is a habit in how the test commands are
invoked. It applies equally to `| head`, `| grep` and `| tee`.

**The fix.** Put `set -o pipefail` at the front of any command that pipes a
build or test into a filter, or drop the filter and read the output file. Note
that `scripts/run-timeout.py` is unaffected when it is the *outermost* command
— it exits with the child's own status — but piping *its* output into `tail`
loses that status again just the same.

### It is not only `cargo test`. It hid a REFUSED PUSH, 2026-09-12

`git push origin lane-b 2>&1 | tail -3; echo "PUSH_EXIT=$?"` printed
`PUSH_EXIT=0` for a push the pre-push hook had **refused**. `$?` was `tail`'s.

Worse than the `cargo test` case in two ways. The pipe also **truncated the
diagnostic**: `tail -3` kept `error: failed to push some refs`, so the reason
-- which gate, which file, and the one-call fix it named -- scrolled past
unread and had to be recovered by re-running the push. And the false success
was about *publication*, not about a test: the next step was merging `main` to
that supposedly-pushed commit.

**What caught it was not the exit code.** It was `git merge --ff-only
origin/lane-b` answering **"Already up to date"** immediately after a push that
claimed to have moved the branch. Two statements that cannot both be true. The
instrument that settled it is `git ls-remote`, which cannot answer from a local
cache -- `origin/lane-b` can, and would have agreed with the lie.

**The habit that replaced it**, and it is shorter than the broken one:

```sh
git push origin lane-b > build/push.log 2>&1; echo "EXIT=$?"
```

Redirect rather than pipe. The status is git's, the whole diagnostic is kept,
and the log can be grepped afterwards as many times as needed. Three further
pushes were refused by gates that same afternoon -- for a diagnostic-forgery
hole, a baseline exemption that outlived its site, and a `.unwrap_or_default()`
regression -- and every one was seen immediately because the status was real.

**The general rule, stated so it outlives these examples:** never pipe a
command whose exit status you intend to read. Sharpened by lane A to the form
worth remembering -- *piping is dangerous even when you do not want the status,
because the status is the only thing separating "no matches" from "no input".*

**Adjacent, unexplained.** The `rustdoc` failure that exposed this was itself
strange: ~98 errors of the form "cannot find type `String` in this scope"
across the whole of `userspace/userdb/src/lib.rs`, i.e. the crate compiling
with no `std` prelude, together with an `E0462` (found staticlib where an rlib
was expected). Re-running the identical command immediately afterwards passed
with 47/47 and clean doctests, so it is a stale-artifact race rather than a
source defect — most likely two `cargo` invocations against the same
`target/` directory, which happens here whenever a lint run and a test run
overlap. If it recurs and is *not* transient, the thing to check first is
whether a `.rlib` in `target/x86_64-pc-windows-gnu/debug/deps/` was written
for a different target.
