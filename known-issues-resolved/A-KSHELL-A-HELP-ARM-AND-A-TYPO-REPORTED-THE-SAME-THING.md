## A-KSHELL-A-HELP-ARM-AND-A-TYPO-REPORTED-THE-SAME-THING — ✅ FIXED 2026-08-25 (lane A)

**In short:** `nat help` and `nat banana` produced identical output and an
identical "it worked" answer. The first is a request that was granted; the
second is a typo that did nothing. Because the shell said "success" for both,
a script could not tell them apart — `nat banana && deploy` deployed. The fix
gives the typo an extra line saying which word was not understood, and a
failure status; the help request keeps its exit 0, because printing the help
*is* what it asked for.

**Why this could not be fixed by the previous sweep.** These 33 sites were
found by `A-KSHELL-A-USAGE-ARM-THAT-FALLS-OUT-OF-A-MATCH-REPORTED-SUCCESS`
(above) and deliberately left alone by it. That sweep's whole operation was
"add `set_exit(1)` to a usage arm", and here there is no single value that is
right: the arm serves two callers who deserve opposite answers. Adding a
status would have converted a defect that under-reports failure into one that
over-reports it, which is what happened *twice* to that sweep on sites it did
patch. The arm had to be split before any status could be attached, which is
why the debt was carried explicitly in the checker rather than fixed in place.

**Where:** `kernel/src/kshell.rs`, 33 sites in three shapes.

| Shape | Count | Treatment |
|---|---|---|
| Catch-all `_ => { …help… }` under a uniform `match sub {` | 25 | Bind the subcommand (`other => {`) and close the arm with `end_help_arm(cmd, other)` |
| A subcommand branch whose `else`/`_` served both a no-argument *query* and a bad argument | 5 | Split into two branches, so the query answers and the bad argument fails |
| Individually shaped — `cmd_lockdep` (a chain of `if`s, no catch-all), `cmd_tsession` (query arm already correct; its `_` arm already failed), `fsck` (a missing operand, never a query) | 3 | By hand |

**The helper, and why the diagnostic goes last.** `end_help_arm(cmd, sub)`
prints `cmd: unknown subcommand 'sub'` and sets the status, but only when
`sub` is not one of `""`, `help`, `-h`, `--help`, `?`. The empty string counts
as a request because a bare `swapcfg` reaches the arm through
`parts.first().copied().unwrap_or("")` — fourteen commands do that and ten map
the bare form to `"help"` instead, so both spellings must be treated as asking.
The line prints *after* the help text, not before, because it is the line that
stays next to the prompt when the help scrolls past, and it carries the one
piece of information the help text cannot: that what you typed is not in it.

**A command whose bare form is not the help arm.** `cmd_nat` maps no-argument
to `"status"`, which its `"status" | "stats"` arm intercepts, so bare `nat`
never reaches the catch-all and is never called a typo. This was checked per
command rather than assumed; rung 59 pins it, because it is precisely the
regression a shared predicate would introduce if the interception were absent.

**Found while reading, not by the checker:** `cmd_tsession` returned without a
status when the terminal-session subsystem was uninitialised — a command that
did nothing, reporting success. It prints no `Usage:` line, so it is invisible
to a search keyed on that word. The rule is "a command that did not do what it
was asked says so"; `Usage:` is only its commonest phrasing. Fixed in the same
change.

**How the sites were located.** `scripts/_split_help_arms.py` (scratch, since
deleted) *imported* `scripts/check-usage-status.py` and reused its `USAGE`
regex, its `ALLOWED` table and its forward-walk verbatim, rather than
restating the same rule a second time. The first draft did restate it — keyed
on "the first `Usage:` line in the function whose governing arm is `_ => {`" —
and in `cmd_wakesensor` it selected an inner `_ => { …; set_exit(1); return; }`
that was already correct. Two rules meant to describe the same set, written
out twice, differ silently. The transformer also refused any arm whose
governing `match` was not the uniform `match sub {`, which is what separated
`cmd_datausage` (an `Option<&str>` scrutinee whose `_` covered both the query
and the error) from the 25 it could safely rewrite.

**Verification.** All 25 arm bodies were confirmed to be pure help printing —
no `return` that would strand the appended call, no existing `set_exit`, no
side effects — before applying, and the whole 254-line diff was read line by
line afterwards. The checker now reports `21 allowed, 0 known help/error
conflations`; run against the pre-sweep revision it reports 131 raw sites,
which reconciles exactly as 87 fall-out-of-a-match arms + 33 conflations + 11
allowlisted sites, and its 21 allowlist entries exempt 21 sites one-for-one,
so no entry is dead or double-counting.

**Tested by:** `kshell::self_test` rung 59 — the pair of assertions the old arm
could not have satisfied at once (`nat help` → exit 0 with no scolding;
`nat zz_not_a_subcommand` → exit 1 *and* the naming line), plus the bare-form
control (`swapcfg`), the not-a-typo control (`nat`), both halves of the
`fhist autoversion` and `wallpaper offset` splits, and both halves of
`lockdep`, whose unrecognised subcommand previously produced byte-identical
output and status to the query.
