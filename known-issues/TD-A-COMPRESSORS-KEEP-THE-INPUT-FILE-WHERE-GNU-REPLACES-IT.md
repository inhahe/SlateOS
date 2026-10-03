### TD-A-COMPRESSORS-KEEP-THE-INPUT-FILE-WHERE-GNU-REPLACES-IT (lane A, 2026-08-24)

**In short:** typing `gzip big.log` leaves you with **both** `big.log` and
`big.log.gz`. On Linux you would be left with only `big.log.gz` — GNU `gzip`
deletes the original once it has written the compressed copy, and `gunzip`
likewise deletes the `.gz` once it has unpacked it. So someone compressing a
directory to reclaim disk space reclaims none of it, and a script that compresses
a log and then counts files finds one more than it expected.

**Where.** `kernel/src/kshell.rs`. It applies to the whole family, not one
command: there is no `Vfs::remove` call anywhere in the compressor region, so
`gzip`/`gunzip`, `bzip2`/`bunzip2`, `xz`/`unxz`, `zstd`/`unzstd` and
`lz4`/`unlz4` all write the output and leave the input untouched. The behaviour
is at least *uniform*, which is why this is one entry and not five.

**How it was found.** Self-test rung 30 asserted that a refused `gunzip` had
written no archive, and the assertion failed even though the code under test was
correct — the archive it found was a leftover from the rung's own earlier
compress step, still present because `gzip` had not removed it. Cost one boot
cycle (`aed60824f`). The rung now deletes the file first (`a6155cb82`), which is
also what makes the assertion mean what it says.

**Is it a bug?** Genuinely arguable, which is why it is filed as tech debt rather
than fixed on sight:

* *For matching GNU:* it is what every script and every user expects, and the
  surprise is silent — you only notice when the disk does not empty.
* *For keeping the input:* deleting the user's original file is destructive, and
  this shell has no `--keep`/`-k` flag to opt out of it yet, so adopting GNU's
  behaviour without that flag would make the safe case unreachable. The current
  behaviour is the conservative half of the tradeoff.

**Proper fix.** Add `-k`/`--keep` to the family, then make removal the default so
the commands match GNU, with `-k` preserving today's behaviour. Removal must
happen only after the output write has been confirmed to succeed — otherwise a
write error midway through would delete the input and leave nothing behind,
which converts a harmless failure into data loss. Doing it in the other order is
the whole reason this is worth writing down rather than just doing.

**Severity.** Low and non-destructive as it stands — the divergence costs disk
space and script compatibility, never data. Note that fixing it moves it *toward*
being destructive, so the fix needs more care than the bug does.

---

### 2026-08-24 — the split-brain screen: what the `tee` lead actually found (lane A)

The `tee` entry above closed with a lead: *"any command with a piped and a
non-piped implementation (`dispatch_with_input` vs `dispatch`) is a candidate
for the same split-brain."* This is what came of following it. Two of the
findings are fixed (`0a785652a`); three are recorded below and not fixed.

**How the screen was run, and why its output is not a verdict.** All 19 paired
commands were enumerated from `dispatch_with_input`'s match arms, and a script
counted `set_exit(1)` calls in each `cmd_X` against its `cmd_X_input`. Fourteen
pairs came back with a non-zero count on one side and zero on the other. **Only
two of those fourteen were bugs.** A `cmd_X_input` with no `set_exit` at all is
usually *correct*: the file half's only failures are file-open errors, which
have no piped analogue — the pipe supplied the bytes, so there is nothing left
to fail at. Every one of the fourteen was read by hand against its twin before
any verdict was reached, which is the same discipline the earlier sweeps needed
and the reason this count is described here as a *filter* rather than a result.

**Fixed: `sed` and `awk` printed the input verbatim and exited 0 for a script
they could not run.** See `0a785652a` and self-test rung 33. This is a worse
shape than `tee`'s — `tee` at least printed its error, whereas these produce
plausible output, so `cat config | sed 's/old' > config.new` writes an unedited
copy and reports success.

**Correct, on inspection:** `sort`, `uniq`, `head`, `tail`, `wc`, `nl`, `rev`
and `tac` all delegate to the file form when the argument names a file and have
no failure mode left on the pipe path; `paste`, `tr`, `column` and `grep`
already set a status. `cmd_tr_input` is the model the `sed`/`awk` fix followed.

#### TD-A-FIVE-PIPE-FORMS-SILENTLY-DISCARD-A-FILE-OPERAND (lane A, 2026-08-24) — ✅ **FIXED same day (`bb9787783`, boot-verified `eba6d16ce`)**

> **Fixed as described below, with one deliberate departure and one addition.**
>
> * **`mapfile` delegates rather than rejecting a second word.** The plan below
>   argued for rejection on the grounds that bash's `mapfile` takes no file
>   operand at all. That reasoning does not survive contact with the fact that
>   *this shell's* `cmd_mapfile` does take one — the operand is our own
>   extension, so refusing it in the pipe half would invent a third behaviour
>   rather than remove a disagreement. Consistency with `cmd_mapfile` beats
>   consistency with a bash spelling bash does not have.
> * **`sed`'s share was done as a refactor, not a patch.** Both halves now call
>   one `classify_sed_args`; the duplicated argument loop is gone. The two
>   copies had *already* drifted — that drift is precisely how `-i` came to be
>   parsed in one half and ignored in the other — so removing the duplication is
>   what stops the next flag doing the same thing.
> * Covered by self-test rung 34, which pipes one token while naming a file
>   containing a different one, so the two possible answers are distinguishable.
>   It also asserts that a pipe with *no* file named still reads the pipe, so
>   that "the file wins" cannot have been implemented as "the pipe never wins".
>
> The original entry follows unchanged.

**In short:** `cat a.txt | cut -f1 b.txt` cuts fields out of `a.txt` and ignores
`b.txt` entirely — no error, no warning, exit 0. On Linux the named file wins
and the pipe is left unread. Five commands do this: `cut`, `fold`, `sed`, `awk`
and `mapfile`. Their piped implementations parse the file operand out of the
arguments and then throw it away.

**Where.** `kernel/src/kshell.rs`: `cmd_cut_input` (discards `parse_cut_args`'s
fourth return value), `cmd_fold_input` (same, `parse_fold_args`), `cmd_sed_input`
(never collects `file_args` at all), `cmd_awk_input` (binds `_files` and drops
it), `cmd_mapfile_input` (takes only the first word as the array name).

**Why it is a bug and not a design choice.** The other eleven paired commands in
this shell — `sort`, `uniq`, `head`, `tail`, `wc`, `nl`, `rev`, `tac`, `grep`,
`tee`, `paste` — all handle it, and they handle it two different but deliberate
ways: eight delegate wholesale to the file form, `grep` delegates once it sees a
second positional word, `paste` reads the file as an extra column (which is what
GNU `paste - file` does). So there is an established convention, stated in the
comment above the pipe-input block, and these five are simply outside it.

**The `sed -i` corollary, which is the sharp edge.** `cat f | sed -i 's/a/b/'`
parses `-i` in the file form and *silently ignores* it in the pipe form. The
user asked for an in-place edit of a file and got a filtered pipe instead — the
file is untouched, and exit status is 0. GNU refuses this outright (`sed: no
input files while in-place editing`). This is the one case in the entry that
loses work rather than merely diverging.

**Proper fix.** Delegate, following `grep`'s shape rather than the blanket one:
decide *before* flag parsing whether a file operand is present, and if so hand
the whole argument string to the file form and leave the pipe unread. `grep`'s
own comment explains why the order matters — the two halves do not accept the
same flag set, so rejecting flags first would break a legitimate invocation that
merely has a pipe attached. `mapfile` is the exception: bash's `mapfile` takes
no file operand at all, so the right answer there is to reject a second word
rather than delegate to a file form this shell invented.

**Severity.** Medium. Silent and plausible — the output looks like a result, and
in the `-i` case the user believes a file was edited that was not.

#### TD-A-SED-ACCEPTS-AN-UNTERMINATED-SUBSTITUTION (lane A, 2026-08-24) — ✅ **FIXED same day (`da9e7a0ca`, boot-verified `6e7a6ced7`)**

> **Fixed, and two more of the same shape were found in the same function
> while fixing it.** The entry below asked only for `find_unescaped(...)?`. The
> unterminated `s///` turned out to be one of three ways this parser answered a
> script it could not honour by honouring it approximately:
>
> | Script | Old behaviour | Now |
> |---|---|---|
> | `s/a/b` | ran as though the delimiter were there | `unterminated command`, exit 1 |
> | `s/a/b/i` | flag dropped; a **case-sensitive** substitution reported as success | `unknown option to 's'`, exit 1 |
> | `-e good -e bad` | the bad one dropped by `filter_map`, the good one ran | fails, naming `expression #2` |
>
> The third is the one worth remembering: `parsed.is_empty()` only caught an
> **all**-bad invocation, so a single bad `-e` standing among good ones was
> invisible. All three are *wrong answers reported as success*, which is
> strictly worse than missing ones — nothing downstream can detect them.
>
> **What changed structurally.** `parse_sed_command` returns
> `Result<SedCmd, SedParseError>`; both halves share one `parse_sed_scripts`,
> for the same reason `classify_sed_args` is shared (two copies of this drifted
> once already, and that drift is what let `-i` be parsed by one half and
> ignored by the other). The diagnostic quotes the offending script back
> because `#1` identifies nothing when there is only one expression — the usual
> case — and because the commonest confusion here is *which string got read as
> a script*: `classify_sed_args` will take a bare `something` for one, since it
> begins with `s`.
>
> **The entry's own prediction held.** It said tightening would *simplify*
> rather than complicate, because the flag-suffix slice `&cmd[rep_end + 1..]`
> already assumed a terminator. It did: the `rep_end < bytes.len()` branch is
> gone, and so is the `bytes.len() >= 4` guard, which had been sending `s` and
> `s/` to `unknown command` instead of `unterminated`.
>
> **One test-design consequence.** Quoting the script back means a token shared
> between a script and the data makes "the message mentioned it" and "sed
> emitted the line" the same observation. Rung 33's file content therefore moved
> off `old`, and rung 35's witness `zz_keep` is named in none of its scripts —
> it is asserted *absent* to prove a refused run emits neither an edit nor a
> pass-through.
>
> The original entry follows unchanged.

**In short:** `sed 's/old/new'` — a trailing `/` short — is an error on Linux
(`unterminated 's' command`) and runs happily here, substituting `new` for
`old`. Both halves of the command agree about this, so it is not a split-brain;
it is a lenient parser.

**Where.** `parse_sed_command` in `kernel/src/kshell.rs`: the replacement's end
delimiter is found with `find_unescaped(...).unwrap_or(bytes.len())`, so a
missing one silently means "to the end of the script".

**Why it was not fixed with `0a785652a`.** That commit's whole subject was the
two halves *disagreeing*; this is the two halves agreeing on something GNU
rejects. Fixing it changes the behaviour of a command that currently works, for
every caller, which is a separate change with a separate risk — a script relying
on the leniency would start failing. Rung 33 documents the distinction inline so
the next reader does not mistake one for the other.

**Proper fix.** Require the closing delimiter (`find_unescaped(...)?`), and add
a rung asserting `sed 's/old/new'` fails while `sed 's/old/new/'` succeeds.
Note the flag suffix parsing already assumes a terminator is present when it
slices `&cmd[rep_end + 1..]`, so tightening this simplifies the code rather than
complicating it.

**Severity.** Low. The current reading is the one the user almost certainly
meant; the cost is that a genuinely truncated script runs instead of failing.

#### TD-A-XARGS-REPORTS-THE-LAST-COMMANDS-STATUS-NOT-THE-WORST (lane A, 2026-08-24) — ✅ FIXED (`a3eea79a1`, boot-verified `65b4dda4b`)

**In short:** `printf 'good\nbad\n' | xargs check && deploy` runs `deploy` if
the *last* invocation succeeded, even when an earlier one failed. GNU `xargs`
exits 123 if any invocation exits non-zero, precisely so that a batch failure
cannot hide behind a final success.

**Where.** `cmd_xargs_input` in `kernel/src/kshell.rs` calls `execute(&full_cmd)`
in a loop and never inspects the status, so whatever the final `execute` left in
the exit slot is what the pipeline reports.

**Proper fix.** Track the worst status across the loop and set it once at the
end — the "batch operations must track and report the worst error, not just the
last one" rule from `CLAUDE.md`. Whether to use GNU's 123 or a flat 1 should
follow whatever the `cmp`/`diff` entry above settled for multi-valued statuses,
so the shell speaks one convention rather than two.

**Severity.** Medium in scripts, invisible interactively.
