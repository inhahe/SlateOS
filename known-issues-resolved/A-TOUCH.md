## `A-TOUCH--D-CANNOT-ACCEPT-THE-FORMAT-ITS-OWN-USAGE-LINE-PRINTS` (lane A, 2026-08-30) — ✅ **FIXED** 2026-08-30

**In short:** `touch -d` documents `YYYY-MM-DD HH:MM:SS` and could not accept
it. The space in the middle split the datetime into two arguments before
`touch` ever saw it, so the time was taken as the *filename*:
`touch -d '2026-08-30 12:30:00' notes.txt` refused `notes.txt` as an extra
operand, and `touch -d '2026-08-30 12:30:00'` on its own created a file
called `12:30:00`.

**Where.** `kernel/src/kshell.rs` — `cmd_touch`, and `command_parses_own_quotes`.

**Why the quoting did not survive.** Every kshell command receives a flat
`&str`, not an argv, so "these two words were one argument" is a fact the
dispatch boundary cannot carry. `dispatch` calls `remove_quotes` on the
argument string — which deletes the quote characters and leaves the space —
and `cmd_touch` then re-split on whitespace. Four words came out of three
arguments. Commands that need the distinction opt onto
`command_parses_own_quotes` and use `split_words`, which respects quotes and
removes them; `trap`, `awk`, `fold`, `base64`, `cut`, `tr`, `sed` and `column`
were already there for the same reason. `touch` is now the ninth.

**What made it invisible for so long.** The failure mode is not an error
message about the date — it is an error message about the *path*, or a
silently-created file with a strange name. Nobody reading
`touch: extra operand ‘notes.txt’` looks at the `-d` operand.

**It was hiding a second defect underneath it.** Because the time half was
unreachable, the three `.unwrap_or(0)` reads of hour/minute/second in
`parse_datetime_to_ns` had no way to fire, and so survived forty-one batches
of the §600 guessed-value burn-down above. They were fixed in batch 41 —
present fields must now parse, absent ones are still zero, and a fourth
colon-separated field is refused rather than dropped. The two fixes are
worth stating as one lesson: **a guessed value in dead code is not harmless,
it is merely dormant**, and the batch that makes the code reachable is the
batch that ships the bug.

**Pinned by self-test rung 107**, which asserts the resulting `modified_ns`
as an exact nanosecond count (`2026-08-30 12:30:00` UTC = 20695 days +
45000 s after the epoch) rather than reading a formatted date back. Every
step the rung covers — the quoting, the time fields, and the civil-date
arithmetic — can be wrong in a way that still prints a plausible date. It
also asserts that a mistyped minute is refused *as a date*, which is what
proves the time fields are now reached at all, and that nothing is created
under the name `12:3o:00`.

**And there was a third defect under the second one, found by that rung on
its first real boot.** With the quoting fixed and the time fields parsing,
`touch -d '2026-08-30 12:30:00' /tmp/new.txt` still stamped the file with
*the current time*. `cmd_touch` has two arms — update an existing file, or
create a missing one — and only the update arm ever applied the timestamp.
The create arm computed the instant, called `Vfs::write_file` (which stamps
now), printed `created` and exited **0**. The value was worked out correctly
and then dropped on the floor.

That is the worst shape this family takes. A guessed value is wrong; a
*discarded* value is wrong **and reports success**, so there is no diagnostic
to notice and the only way to discover that `-d` did nothing is to `stat` the
file afterwards. And it bit precisely the case `-d` exists for: a file you are
back-dating is usually one you are also creating, so the broken arm was the
common one and the working arm was the rare one. `requested` is now an
`Option<u64>` — `None` meaning "no `-d`, no `-r`" — and the create arm applies
it as an explicit second step, reporting a failure to set it rather than
printing `created`.

**Three defects in one command, each hidden by the one above it**, is the
lesson worth keeping from this entry: the quoting bug made the time fields
unreachable, which kept the guessed-value bug dormant, which meant nothing
ever exercised the create-with-`-d` path far enough to notice the timestamp
was being thrown away. Fixing the outermost one is what made the next one
observable, twice in a row. **A rung that asserts an exact value is what
converts "the fix compiles" into "the fix works"** — this one was written
against the create path, failed on its first boot with `left:
1788119411893528376` (a wall clock) against `right: 1788093000000000000`,
and named the bug outright. Rung 107 now asserts both arms, so a future
change that moves the defect from one to the other fails.

**Not a regression.** All three were true since `cmd_touch` was written.
