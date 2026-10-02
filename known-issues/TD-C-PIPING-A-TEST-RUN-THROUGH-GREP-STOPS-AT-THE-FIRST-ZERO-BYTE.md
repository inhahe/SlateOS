## `TD-C-PIPING-A-TEST-RUN-THROUGH-GREP-STOPS-AT-THE-FIRST-ZERO-BYTE` (lane C, 2026-09-17)

**In short:** if you pipe a build or test run through `grep` to pull out the
interesting lines, and any test anywhere in the workspace prints a zero byte,
`grep` decides the stream is binary, prints `Binary file (standard input)
matches`, and **stops filtering**. Everything after that point is discarded --
including the line that says whether the run passed. What is left on screen is
a list of tests that passed, which is what a successful run also looks like.

**How it presented.** A `cargo test --workspace` was run as

    run-timeout.py ... cargo test --workspace | grep -E "test result:|..." | tail -30

and came back with thirty `test result: ok` lines, no failures, and a
task-notification reading *exit code 0*. All three signals were worthless:

| Signal | Why it proved nothing |
|---|---|
| The `ok` lines | They are the lines before the abort, not the whole run. Summing them gave 1,523 tests where the workspace has tens of thousands. |
| No `FAILED` line | `grep` had stopped looking long before the end. |
| Exit code 0 | **A shell pipeline exits with the status of its *last* command.** That was `tail`, which succeeds whatever happens upstream. `cargo`'s own non-zero exit never reached it. |

The `[run-timeout] child exited: PASS/FAIL` line -- the one piece of output
that *is* a verdict -- was itself swallowed by the abort, because it arrives
after the binary byte.

**This is the `grep -c` mistake in a new costume,** and that one is written up
two entries above: a measurement that silently stops short and is read as a
result. The shared shape is a tool reporting "I could not look" in a way that
is indistinguishable, at the shell, from "I looked and it was fine". It is
worth recognising by shape, because the specific spelling changes every time.

**What to do instead.** Send the output to a file and let the runner's own
exit code be the verdict:

    run-timeout.py ... cargo test --workspace > build/ws.log 2>&1; echo "RC=$?"

`RC` is then `cargo`'s real status, because nothing is downstream of it. Read
the log afterwards with **`grep -a`**, which forces text handling and does not
abort. `build/` is gitignored, so the log costs nothing but disk and should be
deleted when the run is done.

**Do not "fix" this by adding `-a` to the pipeline and stopping there.** That
restores the filtering but leaves the exit-code half untouched, and the exit
code is the half that turns a red tree into a green-looking one.
