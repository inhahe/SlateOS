## `A-KSHELL-THE-USAGE-GATE-CANNOT-SEE-A-SYNOPSIS-RUSTFMT-PUT-ON-ITS-OWN-LINE` (lane A, 2026-08-25) — **FIXED** 2026-08-25, all 95 defects resolved

**In short:** the gate that checks "if a command prints its usage line because
you typed something wrong, it must also report failure" could not see 192 of
those usage lines at all. Its pattern required the quoted synopsis to sit on
the *same source line* as the `shell_println!(` that prints it — and `cargo
fmt`, not the author, decides that. A long synopsis gets wrapped onto its own
line, at which point the pattern matches neither line. 95 of the 192 it hid
were real defects: the command complained and then reported success, so a
script could not tell a typo from a completed request.

**Third instance of one bug.** This is the same mistake as
`A-KSHELL-THE-USAGE-GATE-WALKS-STRAIGHT-THROUGH-} else {` (braces counted by
the line) and `A-KSHELL-THE-OPTION-GATE-COUNTS-ONE-LINE-AND-RUSTFMT-USES-FOUR`
(a method chain matched by the line), in a third place:

| # | What was counted by the line | What it should have been | Hidden |
|---|---|---|---|
| 1 | `{`/`}` depth, where `} else {` nets to zero | a character-column walk (`walk_block`) | 195 |
| 2 | a regex spanning `.parse().ok().unwrap_or(…)` | a statement (`statements`) | 466 |
| 3 | a regex spanning `shell_println!(` and its first argument | a statement | 192 |

**Why it was found third and not first.** Each of the three is invisible in the
same way: a gate that undercounts prints a *smaller* number, and a smaller
number reads as progress. Nothing failed, nothing warned; the gate said
"clean" and meant "clean, of the subset I can see." What broke the pattern was
an anomaly, not an audit — `pmgr create` visibly lacked a `set_exit(1)` while
the gate was green, and following that one discrepancy back through the regex
found the class.

**The bias is the worst part.** These are not a random 192. The longer a
synopsis, the likelier rustfmt wraps it, and the longest synopses belong to the
commands with the most subcommands — `container` (28 of them), `fw` (20),
`overlay` (12). So the gate's blind spot was concentrated on exactly the
commands where getting a subcommand wrong is easiest.

**The rule, now recorded in both docstrings.** A walk over a *balanced* region
is safe to do by the line. A walk that must see depth go **negative**, or a
regex spanning a construct **rustfmt is free to wrap**, is not — in the second
case the source you are matching against was laid out by a formatter, and its
newlines carry no meaning you may rely on.

**Fix.**

* `scripts/check-usage-status.py` matches `USAGE` against a statement from
  `_rl.statements(...)` rather than a line. Because a statement's text has had
  its newlines collapsed, the match column within it says nothing about any
  line, so a second regex `CALL` locates the macro call on the statement's
  *first* line to give `walk_block` a real starting column. Without that the
  walk starts at column zero and counts the `{` the print is nested inside.
* The 95 defects were repaired in four shapes:
  * **38** catch-all `_ => { help }` arms that fell out of their match with no
    status, now ending in `end_help_arm(cmd, sub)` — which sets 1 only for a
    subcommand that was *not* a request for help, so the one arm can serve both
    callers correctly.
  * **50** guard branches (`if parts.len() < N`, `if let Some(..) = .. else`)
    that printed a synopsis and exited 0, now ending in `set_exit(1)`.
  * **2** arms where the status was set *above* the message rather than below
    it (`cmd_overlay`, `cmd_container_network`) — harmless at run time and
    invisible to a reader, caught only by the forward walk. Reordered.
  * **5** genuinely correct sites added to `ALLOWED` with reasons: two explicit
    `--help` arms (`cmd_dedup`, `cmd_journal`), `cut`'s error formatter whose
    fifteen callers all set a status, and the two catch-alls whose help text
    lives in a nested `#[inline(never)] fn case()` the walk cannot follow
    (`cmd_firewall`, `cmd_container`) — the same exemption `cmd_netsettings`
    and `cmd_tasksched` already carried.
* `kshell::self_test` rung 68 covers one command per repair shape, each
  refusal paired with a control that runs the same command with a usable
  operand, so the rung cannot pass by having broken the command outright.

**Severity.** Moderate. No memory-safety or correctness consequence inside the
kernel; the damage is to scripting. A command that complains and exits 0 tells
`&&` and `set -e` that it worked, so a script built on 95 of these silently
continues past its own errors — and the gate that existed to prevent exactly
that reported clean the whole time.
