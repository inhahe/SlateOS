## 713. Where this `ed` may not match GNU, it refuses rather than guesses — and it prints file names the way `ed` does, not the way the coreutils do

**Date:** 2026-08-30
**Decided by:** Claude (autonomous), lane B
**Lane:** B

**In short:** `ed` is a line editor, and the thing it does when it finishes is
overwrite your file with whatever is in its buffer. That makes "carried on
without understanding the command" a much more expensive mistake here than in a
utility that only prints. This entry records three places where our `ed` and
GNU's cannot both be satisfied, and the single rule used to settle all three:
when we cannot do what GNU would do, **say so and stop**, rather than do
something else quietly. It also records the one place we deliberately copy GNU
against the grain of the rest of the coreutils — how a file name is printed in
an error message.

**Measured against:** GNU ed 1.20.1, x86_64 Debian under WSL, `LC_ALL=C.UTF-8`.
The 140-case harness is `scripts/ed-diff.sh`; all three decisions below appear
there as explicitly-marked deliberate differences, so none of them can be
mistaken later for a bug nobody noticed.

### 1. ~~`-E` and `-G` are recognised and refused~~ — superseded 2026-08-30

This decision said `-E`/`--extended-regexp` and `-G`/`--traditional` were
refused rather than accepted-and-ignored, because a script that passes `-E` and
gets literal matching does not fail — it **edits the wrong bytes** and writes
them. It set its own expiry: *"what changes when regular expressions land: both
flags become accepted, and this half of the entry is deleted rather than
revised."*

Regular expressions landed the same day (`known-issues.md` →
`TD-B-ED-HAS-NO-REGULAR-EXPRESSIONS`, closed), so both flags are now accepted
and both do what they say: `-E` compiles through `ere::Regex::new_flags` instead
of `ere::bre::compile`, and `-G` drops `l`'s trailing `$`.

The heading is kept, and the section not renumbered, because §713's other three
decisions are cited by number from `ed.rs` and from the harness — renumbering to
close a gap would break those citations to save a line.

One correction worth keeping, because it was the premise of the original text
and it was wrong: **`-G` was never about a regex dialect.** GNU's
`--traditional` is a compatibility mode over `G`, `V`, `f`, `l`, `m`, `t` and
`!!`. Measured on GNU ed 1.20.1 across the commands this `ed` has, exactly one
of them differs — `l` ends the line without `$`, and folds identically
otherwise. Refusing the flag was still the right call while `l` could not honour
it, but for a different reason than the one recorded.

### 2. A file name beginning with `!` is refused

In GNU ed a file name is a shell command when it starts with `!`: `ed '!ls'`
reads the output of `ls`, and `w !wc` pipes the buffer to `wc`. We implement no
`!` at all.

The two available behaviours are to treat the name literally — there is nothing
stopping us opening a file called `!ls` — or to refuse it. Literal was rejected
for the same reason as above, and more sharply: `w !mail -s x you@example.com`
under a literal reading creates a *file* named `!mail`, silently, and the user
finds out when the mail does not arrive. There is no wording that makes a
literal reading safe, because the failure is that the command succeeded.

*What changes:* `ed '!ls'` prints a diagnostic and exits non-zero, where GNU
lists the directory. Two harness cases (`w !cmd`, `r !cmd`) record it.

### 3. An ambiguous long option lists its candidates, where GNU ed does not

`--ve` is ambiguous between `--version` and `--verbose` on both sides. We print
`possibilities: '--version' '--verbose'`; GNU ed prints `option '--ve' is
ambiguous` and stops there, because it links its own `arg_parser.c` rather than
glibc's `getopt_long`.

This one is *not* a judgment call about `ed` so much as a consequence of a
project-wide one: every converted coreutils bin resolves long options through
`coreutils::getopt`, whose whole point is to reproduce glibc's rules exactly,
and whose diagnostics are checked against glibc's by `scripts/getopt-ambiguity-
check.py`. Special-casing `ed` to suppress the candidate list would mean a
second code path in the shared parser, maintained for one utility, to print
*less* information. Every other wording `ed` emits — `invalid option -- 'Z'`,
`unrecognized option '--zz'`, the `Try 'ed --help' for more information.`
referral, and the usage status of 1 — does match.

The measurement had a second consequence worth recording here, because it is
where a reader will look for it: because GNU ed prints no candidate list, the
option-table gate cannot read its table out of `ed --=x` the way it does for
every glibc utility. That is handled in `scripts/getopt-ambiguity-check.py` by
`OWN_PARSER`, which reads the names out of `--help` instead.

### 4. A file name in a diagnostic is printed raw, not quoted

Everywhere else in the coreutils a path in an error message goes through
`coreutils::quote` (`cp: cannot stat 'x'`). This `ed` prints it bare, because
GNU ed does: `ed: x: No such file or directory`.

*Why the coreutils quote at all:* a path is attacker-controlled in the general
case — it can come out of a directory listing, a glob, a `find`, an archive —
and a name containing a newline can forge a second line of output that a script
or a person reads as coming from the tool. Quoting is what makes the boundary of
the name unambiguous.

*Why that argument does not reach here:* the only names `ed` puts in a
diagnostic are the operand on its own command line and the argument to `f` or
`w` typed into its own prompt. Both are things the person at the keyboard
typed; neither arrives from a listing or an archive. The forgery risk quoting
exists to close is not open, and matching the editor we are diffed against 140
ways is worth more than internal consistency with a rule whose reason is absent.

*What would reverse this:* if `ed` ever grows a path it did not get from the
user — a `!command` whose output is read as a name, an editor-side glob — the
argument flips and the diagnostics should go through `quoteaf_os` that day.

### Why one rule for all four

The three refusals share a shape: in each, the "compatible" behaviour is
compatible only in the sense that it does not stop. It runs, and produces a
result that differs from GNU's in the file rather than on the screen. `ed`
writes files; a difference that lands in the file and not in the output is the
one class of difference a user has no way to notice, and it is exactly the class
the bug this rewrite exists to fix belonged to — the old `ed` printed `0`,
showed an empty buffer, and truncated the file, agreeing with GNU on stdout,
stderr and status the whole way. That is why `scripts/ed-diff.sh` compares the
bytes left on disk as a fourth observable, and it is why, when in doubt, this
`ed` stops.

**How to reverse.** Each refusal is one match arm in `parse_args` or one clause
of `check_name` in `userspace/coreutils/src/bin/ed.rs` — `check_name` is reached
from both the command-line operand and the `f`/`w` name parser, so a single
clause covers both. Each has a named case in `scripts/ed-diff.sh`
(`xfail_pipe`/`xfail_case`) that must
move to a plain case in the same change — the harness's `OURS=/usr/bin/ed`
self-check will report the stale expectation as `XPASS` if it is not.
