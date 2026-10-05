## A-KSHELL-3676-FAILING-COMMANDS-REPORTED-SUCCESS — ✅ FIXED 2026-08-24 (lane A)

**In short:** 3,676 built-in shell commands printed an error message and then
told the script that they had succeeded. Nothing looked wrong on screen — the
error was right there in the terminal — but a script could not see it. `rm -rf
"$dir"` after a failed step still ran; `cmd || echo "failed"` never printed
"failed"; `id=$(cgroup create)` set `id` to the empty string and carried on.
Fixed by giving every one of them a failing exit status, in three sweeps, plus
a fourth commit undoing the four places where the sweep went too far.

**Where:** `kernel/src/kshell.rs`. Commits `306eba384`, `ad372afab`,
`2e7008827`, `2ab39d291`.

**The damage is never the wrong value.** It is the control-flow decision
downstream, and every shell construct that branches — `||`, `&&`, `if`,
`set -e` — reads the exit status and nothing else. A shell in which failure
reports 0 makes all of them decide the opposite of what they were written to
decide. That is strictly worse than the command not existing: a missing command
at least fails.

### Three shapes, found by three different rules

| shape | sites | what it looks like |
|---|---|---|
| bail | 766 | diagnostic, then a bare `return;` |
| fall-through | 945 | diagnostic is the last statement of its block; control falls out |
| error arm | 1,965 | `Err(e) => shell_println!("Error: {:?}", e),` |

The error arms are the worst of the three, because the message is *right
there*. A human reading the terminal sees the error and assumes everything
downstream saw it too.

### What made the sweeps safe, and the three times they weren't

A line-level rule — "the text contains Usage:/Invalid/not found" — is wrong,
and it was wrong on real sites. Each guard below exists because it caught one:

1. **Judge the statement *run*, not the line.** Walk up from the last print and
   collect the contiguous run at the same indent (folding rustfmt's split `);`
   tail back into one statement), then judge the run by its **first** message. A
   multi-line usage block leads with `"Usage: …"` and continues with indented
   examples; a listing leads with a banner or a row.
2. **A complaint starts its own message.** If the run's first literal begins
   with whitespace it is an indented row inside a report. That single test is
   what separates `"Group '{}' not found"` from `"  Unknown drops:    {}"` — a
   row of `vlan stats`, which the first draft swept. Marking that command failed
   would have been a *new* bug.
3. **`Usage:`/`Error:` must lead the message** (after at most a `cmdname: `
   prefix), **and every other word must be a whole word.** Two successes were
   swept before these anchors existed:
   - `"Network Usage: {} apps, {} interfaces"` — the *header* of `netusage
     stats`, which then ends on an indented row: textually the exact shape of a
     usage error.
   - `"datausage: self-tests completed (see serial)."` — the command name
     `datausage` merely ends in the letters `usage`.
4. **A `=== … ===` banner means output, not a diagnostic.** Encoded as a rule
   rather than an exception for `cmd_ksyms`, the only site that trips it today.
5. **`Err` arms need no anchoring.** The arm *is* the failure path — stronger
   corroboration than the `return;` the first sweep leaned on. All 197 distinct
   messages were enumerated: every one names a real failure, so the vocabulary
   filter (which exists to spare arms that absorb an error deliberately) had
   nothing to spare.

### The mirror-image bug the sweep introduced: `--help` is not a mistake

`ad372afab`. A granted help request and a rejected argument **print the same
text**, so no textual rule can separate them — only the gate above the block
can. Four sites printed the usage because the user asked for it, and the sweep
marked all four as failures. That is this task's own bug class in reverse: a
command that succeeded reporting failure, breaking `cmd --help >/dev/null ||
echo "not installed"`, a real idiom for probing whether a tool exists. `grep
--help` exits 0; `grep --bogus` exits 2.

Three were a plain deletion (`dedup`, `journal`, `fsck.ext4`). `zip` was not:
its gate is `args.is_empty() || args == "--help" || args == "-h"`, one condition
covering both a granted request and a command with nothing to run, so it had to
be split.

Found by scanning **every** help-gated block in the file for a failure status,
not by re-reading the sweep's diff — so the check outlives the commit and covers
sites no sweep touched. It is worth re-running after any future sweep.

### Coverage

Self-test rungs 18–20 in `kshell::self_test`, one per lesson rather than one per
shape:

| rung | asserts |
|---|---|
| 18 | `zip --help` and `zip` print **byte-identical** output and opposite statuses |
| 19 | `tag zzz` fails (fall-through); `netusage stats` and `datausage test` do **not** (report header, whole-word) |
| 20 | `cgroup delete <bogus>` fails (error arm); **and `echo ok` still reports 0** |

That last control is the one that makes the set meaningful. Without it, a
kshell that set status 1 unconditionally would pass rungs 17–20.

### Reopened 2026-08-24 — the gates leaked, and ~500 more sites were found

**In short:** the three sweeps above were real, but each was guarded by a list
of words the message had to contain, and that list was too short. A second wave
found roughly five hundred more commands with the same bug. The rule that
replaced the word list is that an `Err` arm needs no word list at all.

Commits `89354fcb8`, `c2b986242`, `0b4dd69c9`, `66749a61c`, `aad3bcdaf`.
Self-test rungs 21–24.

**How the leak was found: by accident.** Not by an audit. While reading an
unrelated site, an `Err` arm went past that plainly reported a failure and set
no status. Enumerating the shape turned up **223** more the first sweep had
rejected, every one for a wording near-miss:

| message | why the gate rejected it |
|---|---|
| `Error rebuilding index` | no colon after `Error` |
| `Transaction {} failed` | the list had `failed to`, not `failed` |
| `[oci] Could not read …` | the list had `cannot`, not `could not` |
| `tag add: {:?}` | no listed word at all |
| ~60 × `<subsystem>: test FAILED: {:?}` | a *self-test* announcing a failed test, then reporting success |

Rule 5 above already said `Err` arms need no anchoring. The sweep applied one
anyway, and those 60 self-test arms are what it cost.

#### Four distinctions that decided the ambiguous sites

These are the useful output of the second wave; the status changes are
mechanical once these are settled.

1. **Checker vs dashboard.** A checker runs tests and its verdict *is* the
   answer, so the verdict belongs in the status: `fsck`, `sha256sum -c`,
   `syshealth`, `invariant`, `integrity verify`. A dashboard displays state and
   the display succeeding is the answer: `top`, `vmstat`, and this shell's
   `diag`, which is left exiting 0 on purpose. The distinction is the source's
   own — `cmd_syshealth`'s doc comment reads *"Unlike 'diag' (which reads
   passive counters), this command actively tests kernel subsystem integrity."*
2. **A report row must not decide the command's status; its summary must.**
   `syshealth`'s `[FAIL]` rows, `vlan stats`'s `Unknown drops`, `snmp
   sysinfo`'s `(timeout)` rows, `integrity verify`'s `[MODIFY]` rows. Fix the
   verdict line, not the row. The corollary bit in `zip`, which counts read
   failures and then interpolates `", N errors"` into a *success* line: no
   summary carried the verdict, so there the row had to.
3. **Named lookup vs state query.** Asking for a **named item** that is not
   there is a failed lookup — `printenv FOO`, `git config --get` and `grep` all
   exit non-zero. Asking about **state** and being told the state is a
   successful query — `getfacl` prints the permissions and exits 0 whatever
   they are. This is what separates `"{}: not set"` (swept) from `"No active
   profile."` and `seal check`'s `DENIED` (not swept: the check answered the
   question it was asked, and the answer being "no" is not a failed command).
4. **Did the container start?** For `oci run`, flipping the status when an
   option could not be applied is *actively dangerous*, not merely wrong:
   `oci run … || cleanup` would tear down a container that is up and running.
   Eleven `[oci]` sites are excluded on this rule. Whether `oci run` should
   instead refuse to start is a genuine question about the command's contract
   and is filed in `open-questions.md`.

#### The residue: braced arms, and what a brace means

79 `Err` arms had a `{ … }` body, which no mechanical pass reaches. Reading
them showed a brace means one of two very different things. Usually it means
nothing: `cargo fmt` wrapped one long line and the arm beside it already sets a
status. Sometimes it is deliberate, and then the test is whether the arm
*returns its outcome*: one ending in `false` or `None` hands the decision to a
caller, but one ending in a bare `shell_println!(..)` evaluates to `()` and
defers to nobody. That is **not** the same test as "ends without a semicolon",
which was the first draft's, and the difference was eight sites.

Six such arms end in `continue;` — the per-file loops in tar/cpio/zip/sed. The
status has to be raised *before* control leaves the arm or it is dead code;
rustc caught the naive placement as `unreachable_statement`.

#### Two process lessons, both learned the expensive way

**Key an exclusion list on the message, never on a line number.** A line number
is a fact about a moment; a message is a fact about the site. The first
exclusion list used line numbers, merging `main` shifted every one, and the run
reported *"9 excluded"* while excluding **nothing** and sweeping all four sites
the list existed to protect. Both sweep scripts now key on message text and
**abort if any exclusion matches no site**, so the failure cannot recur
silently.

**The string-literal guard was unsound for two sweeps.** Every rewrite was
checked with "the ordered list of string literals must be byte-identical before
and after", implemented as `re.findall(r'"((?:[^"\\]|\\.)*)"')`. That regex
pairs quotes sequentially from the top of the file, so a single *stray* quote
desynchronises everything below it — and `kshell.rs:826` is

```rust
b'/' | b' ' | b'\t' | b'"' | b'\'' | b':'
```

where `b'"'` contributes one unpaired `"`. Past it the regex stops matching
literals and starts matching the **gaps between** them, and since `[^"\\]`
matches a newline those pseudo-literals swallow whole blocks of code: 4,951 of
its 40,803 matches contained a newline. Found because it aborted a rewrite that
touched no message at all; the dangerous direction is that it can equally
*pass* while a real literal moved. Replaced with a proper Rust lexer (comments,
nested block comments, char literals, byte/raw strings), which finds 40,581
literals and only 23 containing a newline — all genuine `\`-continued strings.
The earlier sweeps are fine on other evidence (brace balance, net `set_exit`
delta, a diff-shape audit, a clean build, a green boot test), but the guard
they leaned on was weaker than believed.

#### Still outstanding

- **~356 statusless *bail* sites** (`survey_bail.py`). Unlike an `Err` arm, a
  bail is **not** by construction a failure: `return;` after a print covers both
  "you invoked me wrong" and "here is your answer, done". The survey found
  `dirname`'s `/` — literally the command's output — beside `zip: no input
  files`. Gating on an argument test (`args.is_empty()`, `parts.len() < 2`)
  narrows it to 29 but does not separate them either, because `alias`,
  `declare`, `trap`, `volume` and `zoom` all use "no arguments" to mean *show
  me the current value*. This set has to be read, not swept.
- **~17 `"Syntax error: …"` sites** in `handle_control_flow`. Verified safe to
  fix: `execute()` returns immediately after `handle_control_flow` returns true
  without touching the status. Use `set_exit(1)`, not bash's 2 — a half-migrated
  POSIX table (2 for syntax but not 127/126/128+N) is worse than a consistent
  one.
- `zip` has three more statusless bails of its own (`need at least an archive
  name and one file`, `'{}' is a directory (use -r)`, `no input files`) — the
  sites that revealed the bail gate had leaked.
- **`cmp` and `diff` never put their verdict in their status** (found
  2026-08-24 while reviewing the bail sweep's chosen sites; `cmd_cmp` at
  kshell.rs 94885, `cmd_diff` at 94951). Both print the answer and exit 0
  whichever answer it is:
  - `cmp` prints `a b differ: byte 7, …` and exits 0. So does `a and b are
    identical`. The idiom `cmp -s a b && echo same` therefore always says
    "same", and `cmp a b || rebuild` never rebuilds.
  - `diff` is the same, and additionally exits 0 after `diff: files too large
    for line diff` — refusing to compare reads as "identical". (That last one
    *is* in the queued bail sweep; the same/differ verdict is not.)

  This is the fourth shape — the verdict-carrying checker — in the two commands
  where the status is not merely *a* channel for the answer but the
  *conventional* one. Unlike `syshealth`, both sides are trivially testable
  from a self-test, so the fix comes with a rung asserting all three of
  identical→0, differ→1, error→1. Follow the tree's flat-`1` convention rather
  than POSIX `cmp`'s 0/1/2: a half-migrated table is worse than a consistent
  one (same reasoning as the syntax-error sites above), so "differ" and "could
  not compare" both report 1. That is a real loss of resolution and it is
  still strictly better than today, where the dangerous direction — "I did not
  look" reading as "they match" — is the one currently taken.

### Known under-sweep, left deliberately

Rule 3 rejects `"base64: decode error: {}"` and `"tee: write error: {:?}"`,
which have two words before the `error:`. Loosening the anchor to admit them
re-admits `"Network Usage:"`. Three unfixed sites beat one invented failure.

**Lesson:** a mechanical rewrite over thousands of sites is only as good as the
predicate, and a predicate that reads one line cannot tell a complaint from a
row of a report. Every guard above came from *sampling the sites the rule chose*
and *enumerating every distinct message it matched* — not from reading the
rule and judging it plausible. Two of the three false positives were single
sites out of thousands, and both would have turned a working command into a
failing one.

#### 2026-08-24, later — the under-sweep is closed, by hand (lane A, `910ef26af`)

All three sites now set a status. The predicate was never loosened, and should
not be: the reasoning above still holds, and `"Network Usage:"` is still a row
of a report. What changed is the recognition that **three is a number you fix by
hand.** A rule that cannot safely admit three sites is a good rule; leaving the
three sites broken because the rule cannot reach them is a separate decision,
and it was the wrong one.

Two had already been picked up by later waves (`base64: decode error:`, and the
piped `tee` in `cmd_tee_input`) — so the count here was stale, and the entry
still read "three unfixed sites" while only one was left. The survivor was
`cmd_tee`, the `tee FILE TEXT` form.

That one is worth recording for its shape rather than its size. `tee` is **one
command with two implementations**, and they disagreed: the piped form set a
status on a failed write, the two-argument form did not. Identical message,
identical failure, opposite status — invisible in the output, visible only in
`&&`. A per-message sweep cannot find that class at all, because nothing about
either message is wrong; the defect is the *disagreement between two call
paths*, which is only apparent when both are read together. Rung 32 therefore
asserts both forms against the same bad path, and both against a good write.

**Worth checking elsewhere:** any command with a piped and a non-piped
implementation (`dispatch_with_input` vs `dispatch`) is a candidate for the same
split-brain. This was found by hand, not by a rule.

### 2026-08-24 — the payoff: giving `syshealth` a status uncovered two real kernel bugs

Fixed in `429a81bc3` and `e5c2d77df`. Recorded here because the *way* they
surfaced is the argument for the whole sweep.

Rung 21 (added with the checker fix in `89354fcb8`) asserts the uncontroversial
half of the rule: a checker that **passes** must still report 0, or a blanket
`set_exit(1)` would "fix" the bug by breaking every caller. It panicked on the
first boot that ran it. `syshealth` was reporting **`ISSUES DETECTED` on a
healthy kernel**, and had been since the lock validator landed — its check 5,
"no lockdep violations", failed on every boot. Nobody had ever seen it, because
the command exited 0 whatever it printed.

Two independent causes, both genuine:

1. **lockdep counted a `try_lock` as a possible deadlock.** A deadlock needs
   both sides stuck; a `try_lock` returns `None` and unwinds, which breaks a
   cycle rather than closing it. `sysctl::get` holds `sysctl-reg` across
   `fs::cache::try_flush_expired`'s `try_lock` of `CACHE`, and that fired on
   every boot. Linux's lockdep skips dependency-add for a trylock for exactly
   this reason. Ours now takes a `lockdep::Acquire::{Blocking, Try}` argument.
   The *outgoing* direction stays — a try-acquired lock is genuinely held, so a
   blocking acquire nested inside it deadlocks normally — and the new rung
   asserts that half too, because over-suppressing would be the silent mistake.
2. **lockdep's own self-test polluted the counter it publishes.** `self_test`
   plants three violations on purpose to prove the detector fires, and they
   landed in the same tally as real ones, so `violation_count()` was
   permanently ≥ 3. Now tallied separately (`SELF_TEST_VIOLATIONS` +
   `IN_SELF_TEST`) rather than reset afterwards — a reset would erase a genuine
   violation landing in the same window, and would leave the self-test's own
   assertions with nothing to check.

See `design-decisions.md` §273 (*lockdep's `try_lock` edge* — not the
unreachable-self-tests §273) for the alternatives weighed on both.

**Two lessons, and the second is the general one.**

*A checker whose verdict is not in its status is a checker nobody reads.* This
is stronger than the claim the sweep started from. The original argument was
that a missing status misleads *downstream control flow* — `||`, `&&`, `set -e`.
Here there was no downstream: a human ran `syshealth`, and the `[FAIL]` row was
right there on screen, and it still went unnoticed for the validator's entire
lifetime. Seven rows of which six say `[PASS]` are read as "fine". The status
is not merely the machine-readable channel; it is the *only* part of the output
that is read every time.

*Assert the passing side of a checker, not just the failing side.* Rung 21
could only exercise `syshealth`'s success path — a corrupt heap cannot be
staged from a healthy kernel — and it looked like the weak half of the test at
the time it was written. It is the half that found both bugs. A test that
pins "healthy means 0" fails loudly the moment anything makes healthy stop
meaning 0, which is precisely the regression a status-carrying checker invites.

### 2026-08-24 — and a third, in the same seven rows: the page-fault check

Fixed in `98a19ac11` and `c8d55ccdd`. With lockdep's check 5 green, `syshealth`
still failed — `[FAIL] Page faults: 5 fatal fault(s)` — so the same rung caught
a third independent bug behind the first two. (This is also the entry that
makes the heading above read "two"; it should read "three".)

Finding it took one boot rather than two only because the rung now dumps a
failed captured command's output to serial (`dump_if_failed`, `75f186b29`). The
first time, a bare `assert_eq!(last_exit(), 0)` panic named neither the failing
row nor the reason, and identifying it cost a whole extra 13-minute boot cycle.

**The counter was wrong.** `handle_page_fault` called `mm::fault::record_fatal()`
on entry to its "the kernel cannot resolve this" path — before the Linux
`SIGSEGV` delivery attempt, before SEH dispatch, before any kill. Two of the
five were faults the *program handled and survived*:

| Fault | What actually happened | Counted as |
|---|---|---|
| task 97, `spawn-test-seh-exit` | SEH handler entered, called `SYS_EXIT` | fatal |
| task 295, glibc ring-3 | real `rt_sigframe` → SIGSEGV handler → `siglongjmp` recovery | fatal |

Both are the *success condition* of a self-test proving fault delivery works.
The counter was recording those successes as crashes. The tally now happens in
`dispatch_or_kill_userspace_raw`, immediately before the kill that diverges —
the last instant at which it can be taken and the first at which the outcome is
known. A new `FAULTS_DELIVERED` counts the other branch, so a handled fault
stays visible instead of merely uncounted (`pgfault` → "Handled by userspace").

**And the question was wrong.** The remaining three were user processes the
kernel correctly killed for faulting with no handler — normal operation, not a
kernel health problem; no other OS calls itself unhealthy because a process
segfaulted. A fatal *kernel* fault would be a real verdict, but that path ends
in `halt_loop()`, so no shell ever runs to report one. Check 7 could therefore
only ever fail for the one reason that did not matter.

It was also a passive counter, which this command's own doc comment excludes:
"Unlike `diag` (which reads passive counters), this command actively tests
kernel subsystem integrity." `diag` already had the row.

Check 7 is now an **active probe**: `mm::fault::probe_demand_page()` registers
an anonymous VMA at a reserved kernel address, writes to it — which must take a
fault — and requires the handler to have mapped a full 16 KiB frame (all four
4 KiB pages, so a resolver that mapped only the first would not pass), then
unmaps and frees it. That matches how checks 1, 2 and 6 exercise the allocators
instead of reading their counters. `mm::fault::self_test` is now a logging
wrapper over the same probe.

**Making the probe repeatable exposed a fourth, latent bug.** The boot-only
original removed its VMA on the failure paths but never freed the frame the
fault had mapped. Invisible while it ran once per boot and only on success;
from `syshealth` a single failure would have leaked a frame, and every later
run would then have failed with `AddressBusy` — a real bug reported as a
different, fictitious one. The probe now releases everything on every path, and
`AddressBusy` is a failure rather than the old silent `SKIP` → `Ok(())`, which
was the sweep's own bug class hiding inside the test for it.

**The lesson this one adds.** The first two bugs were about a verdict nobody
read. This one is about a verdict about the wrong thing — which survives even a
loudly-reported status, because it fails *correctly*, forever, for a reason no
operator can act on. A check that has never once passed is not a strict check;
it is a check whose subject is wrong, and the fix is to change the question,
not the threshold. Ask what the checker could observe that would be *actionable*
— here, "does demand paging still work?" — rather than what it happens to have
a counter for.

### 2026-08-24 — a fourth shape: the assertion whose *input* takes the other branch

Fixed in `326d4e0b9` (the test) and `e9c60b5c4` (a real bug it was hiding).

Rung 23 of `kshell::self_test` asserted that `realpath /zzz_no_such_path`
reports a failure and exits 1. It does neither, and never had. `resolve_inner`
lets the **final** path component be absent — that is the open-with-create path
— so resolution succeeds, `realpath` prints the canonical name, and `dispatch`'s
entry stamp leaves the status at 0.

That leniency is not a quirk to route around: it is exactly GNU `realpath`'s
default mode, where every component except the last must exist (`-e` demands the
last one too, `-m` demands none). **The command was right and the test was
wrong** — it exercised the error arm with an input that takes the success arm.

The rung now points its failing assertion at a path whose *parent* is missing,
which is a genuine `NotFound`, and asserts the succeeding case beside it so the
"missing final component is fine" rule is pinned as deliberate rather than left
looking like the bug it isn't.

**What the wrong test was hiding.** `cmd_realpath` passed the raw operand to
`Vfs::resolve_path` instead of the shell's cwd-aware `resolve_path` helper.
`normalize_path` unconditionally prepends `/`, so a relative argument was
silently reinterpreted as absolute-from-root: `realpath foo` in `/tmp` answered
`/foo` and exited 0. Making a relative path absolute is the command's entire
purpose, so its only interesting input was the one it got wrong — and every
assertion in the rung passed an absolute path, so none of them could have caught
it. A regression that changes directory now sits beside them.

**The lesson.** Bugs one and two were a verdict nobody read; the third was a
verdict about the wrong thing. This one is a verdict about the wrong *input*:
an assertion is only as good as the branch its argument actually reaches, and a
test that has never passed is evidence about the test at least as often as about
the code. Before tightening an assertion that fails, confirm the input reaches
the branch being asserted — here, reading `resolve_inner` cost minutes and would
have saved two 10-minute boot cycles.

**And a note on cost.** Both diagnoses came from reading, not from booting, but
what made the failure *legible* was the `assert_output_starts_with` helper added
in `1123c4637`: it printed `actual output (18 bytes): /zzz_no_such_path` instead
of repeating the test author's sentence. Auditing that commit then showed it had
converted only 11 of 15 sites — the rewriter matched `&out`, and four captures
are bound to other names (`0ddfaa5bc`). A mechanical sweep's own coverage is
worth re-deriving from the source rather than from the count the sweep reported.
