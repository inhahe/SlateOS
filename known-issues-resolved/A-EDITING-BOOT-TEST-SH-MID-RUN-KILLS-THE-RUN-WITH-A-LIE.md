## A-EDITING-BOOT-TEST-SH-MID-RUN-KILLS-THE-RUN-WITH-A-LIE — FIXED 2026-09-08

**Lane:** A. **Severity:** costs a 20–45 minute run, and — the part that
matters — blames a line that is innocent, so the first response to it is to
go debug working code.

**In short.** `scripts/boot-test.sh` is a long-running shell script. If you
edit it while a run is in flight, that *running* run dies partway through with
a syntax error pointing at a line nobody touched. The fix in the script is
one line of re-exec; until then the rule is "don't edit `scripts/` while a
boot test is running", which is a rule you have to remember, which is why
this is an entry and not just a habit.

**Why it happens.** Bash does not read a script into memory. It reads it
incrementally, remembering a **byte offset** into the open file, and reads
more when it needs the next command. An edit that changes the file's length
above the current offset shifts every later byte; bash resumes reading at the
old numeric offset, which now lands in the *middle of a different token*, and
reports a syntax error at whatever it happened to land in. The error is
therefore about the file's new contents at a stale position — it has nothing
to do with the code named in the message, and nothing to do with what the run
was doing.

**Observed.** Run `bhjj3hopy`, 2026-09-02. Clippy had already passed (821 s,
0 errors) and all 26 tooling suites had passed. Then:

```
./scripts/boot-test.sh: line 4636: syntax error near unexpected token `&&'
```

Line 4630–4640 is clippy-crash-detection code, was not edited, and is valid:
`bash -n scripts/boot-test.sh` reported "syntax OK" on the very file the error
came from. The edit that caused it was appending `prune_build_cache_if_low`
near line 1570 — thousands of lines *earlier* than the reported failure, which
is exactly the signature: the reported line is downstream of the offset shift,
not at it.

**How to recognise it in one read.** All three together:

1. a `syntax error near unexpected token` from `boot-test.sh` itself, on a run
   that had already got past several long phases;
2. `bash -n scripts/boot-test.sh` says the file is fine;
3. `git status` / your own recollection shows `scripts/boot-test.sh` was
   written to while the run was live.

If (2) fails, it is a real syntax error and this entry does not apply.

**The proper fix** (not yet done — the tree had a run in flight when this was
written, which is the joke). Have `boot-test.sh` copy itself to a temp file
and re-exec from the copy, so the file bash is reading is one no editor will
touch:

```bash
# Near the top, before any long-running work.
if [ -z "${BOOT_TEST_REEXECED:-}" ]; then
    _self="$(mktemp)" || _self=""
    if [ -n "$_self" ] && cp "$0" "$_self"; then
        BOOT_TEST_REEXECED=1 export BOOT_TEST_REEXECED
        # The copy is deleted by the trap the re-execed instance installs.
        exec bash "$_self" "$@"
    fi
fi
```

Two details the implementation has to get right, both of which are why this is
written down rather than left as "obvious":

- **`$0` and `SCRIPT_DIR` must keep pointing at the real checkout**, not at
  `%TEMP%`. `SCRIPT_DIR` is used to find `run-timeout.py`,
  `prune-build-cache.py`, and the Python suites; if it resolves to the temp
  copy's directory the run silently stops testing the tooling. Pass the
  original directory through an environment variable set before the `exec`.
- **The copy must be removed on every exit path**, including the timeout kill
  from `run-timeout.py`. `run-timeout.py` tears down the whole process tree,
  so a `trap … EXIT` in the re-execed instance is not guaranteed to run;
  putting the copy under `build/` with a run-scoped name, and letting
  `reclaim-space.py` step 1 age it out, is more reliable than trusting the
  trap alone.

**Where it is:** `scripts/boot-test.sh` (the whole file is the subject; the
re-exec belongs immediately after the `set -u`/`SCRIPT_DIR` preamble).

**Fixed 2026-09-08.** The re-exec is in place at lines 302–325 of
`boot-test.sh`. It copies itself to a temp file via `mktemp`, sets
`BOOT_TEST_REEXEC=1` and `BOOT_TEST_ORIG_DIR` (so `SCRIPT_DIR` still points
at the checkout, not the temp directory), and execs the snapshot. The parent
shell installs a `trap … EXIT INT TERM` that removes the snapshot on every
exit path. Both details the entry called out — preserving `SCRIPT_DIR` and
cleaning up the copy — are handled.
