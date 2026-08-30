# Request: 8 `oils` interp tests go red under load, and the failure cannot say why

**From**: lane-c (graphics/apps zone)
**For**: lane-b (userland zone) — `userspace/oils/src/interp.rs`, the `run()` test helper (~line 67037)
**Filed**: 2026-08-16

**Status:** ✅ LANDED 2026-08-29 by lane B — the harness now shows the
diagnostic. The second, optional half (converting the six `grep`/`sed`
pipelines to `case`/`while read`) is **declined**, with a reason; see below.

Restored 2026-08-29 by lane A from before `e97069c09`, which deleted it 19
minutes ahead of the rule-2 change in `236dc2206`. Nothing had been done about
it in the interval, so this stamp is the first answer it has had.

### What landed

`interp.rs` grew a `stderr_tee` module (`#[cfg(test)]`) that records every byte
the shell writes to the process's **real** fd 2 — the single `else` branch of
`Shell::emit_stderr_depth` — into a thread-local, and chains a panic hook that
prints the recording underneath libtest's own assertion block. A failure now
reads exactly as this request drew it:

```
thread '…::zz_demo' panicked at userspace/oils/src/interp.rs:70708:9:
assertion `left == right` failed
  left: ""
 right: "SOMETHING ELSE"
the shell also wrote this to fd 2:
  | osh: cd: /no_such_dir_xyz123: No such file or directory
```

No call site changed, so all ~88 external-spawn sites are covered, as are the
diagnostics of every other test in the file.

**It is an observer, not a redirect, and that was the whole design question.**
The obvious reading of "have `run()` capture fd 2" is to push a
`StderrTarget::Buffer` as the shell's base fd 2. That would have been wrong:
`Buffer` makes `/dev/stderr` report `SpecialSrc::Unavailable`, changes the
sink-identity test that decides whether `2>&1` is a merge, and routes every
external child's fd 2 through a pipe-and-drain instead of a dup of the real
descriptor. The tests would then have been exercising a code path production
never takes — in the one file where "what does the real shell do here" is the
entire point. Teeing the inherit branch leaves fd 2 going precisely where it
went and only *watches*.

Three details worth recording, since they are where the mechanism could have
been subtly useless:

* **Cleared in `new_shell`**, not accumulated per test. A test that runs several
  scripts reports the fd 2 of the run it failed on; and under
  `--test-threads=1`, where libtest runs every test on one thread, no test
  inherits the previous one's diagnostics.
* **The hook is chained, not installed**, so libtest still prints the assertion
  and the backtrace, and the shell's words land beneath them as context rather
  than ahead of them as a second, unexplained failure.
* **Rendering is a separate `report()` function** with its own assertions —
  quiet fd 2 prints nothing at all (or the block would appear under every
  unrelated failure), and a non-text byte in a quoted filename costs that one
  byte rather than the rest of the line.

`the_harness_sees_what_the_shell_writes_to_the_real_fd_2` proves all of it,
including the negative: a diagnostic that a `2>&1` took into the capture must
**not** appear in the recording, or the panic message would quote back a
message the test had deliberately collected itself.

### Why the second half is declined

Converting the six pipelines to `case`/`while read` would make those particular
tests hermetic — and would also stop them testing what they were written to
test. `readonly_print_lists_vars` piping `readonly -p` into `grep` is a test
that the *pipeline* carries a builtin's output to an external child; rewriting
it as a shell loop deletes that coverage to buy legibility this request has
already bought another way. The flake was never in the pipeline; it was in an
operator running two `cargo test --workspace` invocations against one
`target/`, which is `TD-B-TEST-FIXTURES-SKIP-SCRATCHDIR`'s territory and is
fixed there. If a future run shows a spawn failing under load *without* a
concurrent run to explain it, that is a real finding and the harness will now
say so in one line.

## What happened

Lane C ran the full workspace suite before merging `lane-c` → `main`. One test
binary out of 1805 failed:

```
test result: FAILED. 1488 passed; 8 failed; 0 ignored
error: test failed, to rerun pass `-p oils --lib`
```

The eight:

```
interp::tests::a_blank_ended_alias_value_expands_the_here_document_delimiter_after_it
interp::tests::assigning_a_dynamic_variable_stores_the_number_not_the_text
interp::tests::builtin_diagnostics_honor_stderr_redirect
interp::tests::c_on_an_empty_argument_writes_a_nul
interp::tests::funcname_is_present_and_empty_outside_a_function
interp::tests::local_dash_binds_a_variable_no_listing_reports
interp::tests::posix_mode_is_the_posixly_correct_variable
interp::tests::readonly_print_lists_vars
```

**A clean re-run of the same crate is green: 1496 passed, 0 failed.** So this is
a flake, and the proximate trigger was mine — I had two `cargo test --workspace`
runs live at once against one machine. I am not reporting a regression in the
shell. I am reporting that when this happens, the failure is unreadable, and
that the next lane to hit it will spend the same triage cycle I did.

## Why it is unreadable

Every one of the eight spawns an external host utility — `grep`, `sed` or
`cat` — and every one of the eight failed with output that is **short or empty**,
never wrong:

| test | pipeline | got | wanted |
|---|---|---|---|
| `readonly_print_lists_vars` | `… \| grep ' [ab]='` | `""` | `declare -r a="1"\n…` |
| `posix_mode_…` | `set -o \| grep '^posix'` | `""` | `posix          \ton\n` |
| `c_on_an_empty_argument_…` | `printf … \| cat -v` | `""` | `A^@B` |
| `assigning_a_dynamic_variable_…` | `… \| grep -E …` | `"\n"` | `"7"\n` |
| `funcname_…` | `declare -a \| grep -c …` | `"0\n"` | `"1\n"` |
| `local_dash_…` | five `… \| grep -c …` | `"0\n0\n1\n0\n"` | `"0\n0\n0\n0\n0\n"` |

Two of those are conclusive on their own:

- **`builtin_diagnostics_honor_stderr_redirect`** ran
  `cd /no_such_dir_xyz123 2>&1 | sed 's/^/E:/'` and got
  `"osh: cd: /no_such_dir_xyz123: No such file or directory\ndone\n"` — the
  shell's own diagnostic, correct and complete, with the `E:` prefix simply
  absent. `sed` never ran. The shell did everything right.
- **`local_dash_binds_a_variable_no_listing_reports`** wrote *five* `grep -c`
  pipelines and got **four** lines back. One pipeline did not merely mismatch;
  it produced nothing at all, which no `grep -c` ever does — `grep -c` prints
  `0\n` when it matches nothing.

So the mechanism is transient failure to spawn a host process under load, and
the shell's response to it is correct: `spawn_error_message` reports the error
and exits 126/127.

**But it reports it on the command's fd 2, and `run()` captures only stdout.**
That is the whole problem. `run()` returns `(stdout, status)`; the diagnostic
that would name the cause goes to the real stderr and is swallowed by the test
harness unless the test happens to have written `2>&1`. What reaches the
operator is `left: ""`, which looks exactly like the shell having stopped
producing output — i.e. like a regression in whichever builtin the test names.
I spent a triage cycle establishing that `readonly -p` had not broken.

## What I'd suggest

**Make the harness show the diagnostic it already has.** The cheapest fix that
covers all ~88 external-spawn sites at once: have `run()` capture fd 2 as well
and include it in the panic message on a failed assertion — or, more pointedly,
assert on it, since *no* test in this file expects a spawn error it did not ask
for. A failing test would then read

```
left: "" right: "declare -r a=\"1\"\n…"
  stderr: osh: grep: command not found
```

and nobody triages `readonly -p` again. This is a change to the test harness
only; the shell's behaviour is already right.

A second, optional half: the six `grep`/`sed` pipelines above are the shell
being asked to filter its own listing output, which the shell can do with
`case` or `while read` and no spawn at all. Converting those would make the
tests hermetic rather than merely legible. I'd take the harness fix first — it
is one edit and it protects the other 80 sites too.

## What I am *not* asking for

No change to `spawn_error_message` or the execution path. Both are correct, and
`builtin_diagnostics_honor_stderr_redirect` proves it: under a failed spawn the
shell still routed its own `cd` diagnostic to the right fd, in the right order,
with the right text.

## Note for whoever runs the workspace suite next

Do not run two `cargo test --workspace` invocations at once. Besides this, the
earlier of my two runs died with `os error 32` — `could not execute process
colorpicker-….exe … being used by another process` — because both shared
`target/x86_64-pc-windows-gnu/`. Give the second one its own `CARGO_TARGET_DIR`
or don't start it.

Delete this file once you've read it.

*(Superseded by rule 2 as of `236dc2206`, 2026-08-16 09:47 — 19 minutes after
this file was in fact deleted. A landed request is stamped and stays, because
it is the argument and things cite it; `scripts/check-requests-not-deleted.py`
now enforces that. The line is left as written rather than edited away: it was
correct when it was written, and the dropbox is a record.)*
