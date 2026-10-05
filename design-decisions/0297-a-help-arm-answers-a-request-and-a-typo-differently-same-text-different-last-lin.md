## §297 — A help arm answers a request and a typo differently: same text, different last line, different status

**Date:** 2026-08-25
**Decided by:** Claude (autonomous)
**Lane:** A

**In short:** dozens of kernel-shell commands end with a catch-all branch that
prints the command's help. Two quite different things landed there: `nat help`,
where printing the help is exactly what was asked for, and `nat banana`, a
mistake. Both got the same output and the same "it worked" answer, so a script
could not tell a deliberate request from a typo. The decision is that both keep
getting the help text — it is the useful reply either way — but the typo also
gets a line naming the word that was not understood, and a failure status,
while the help request keeps exit 0.

### The question this had to answer

Every other option collapses one of the two callers into the other:

| Option | What it costs |
|---|---|
| Exit 1 for the whole arm | `nat help` reports failure. A user asking for help is not making an error, and `nat help && …` would stop working. This is the mistake the previous sweep made twice on query branches, so it is not hypothetical. |
| Exit 0 for the whole arm | The status quo — the typo is silent. |
| Print help for `help`, print *only* an error for a typo | Withholds the one thing that would help someone who has just mistyped: the list of things they could have typed. |
| Print help for `help`, print the error *before* the help for a typo | Correct, but the naming line scrolls off the top with the help beneath it. |

**Chosen: identical help text for both, and for the typo an extra line
afterwards plus exit 1.** The extra line goes last because that is where it
survives — the help text is long, and the line that stays next to the prompt is
the last one printed. It is also the only piece of information the help cannot
supply on its own: that what you typed is not in it.

### What counts as "asking"

`end_help_arm` treats `""`, `help`, `-h`, `--help` and `?` as requests. The
empty string has to be in that set because a bare `swapcfg` arrives at the arm
as `""` — fourteen of these commands map no-argument that way, ten map it to
`"help"` instead, and both spellings mean the same thing to the user.

The risk in a shared predicate is a command whose bare form means something
*other* than help. `cmd_nat` maps no-argument to `"status"`, so bare `nat`
would be named as an unknown subcommand if its `"status" | "stats"` arm did not
intercept it first. It does, and that was verified per command rather than
assumed — a pinned test exists for it precisely because a future command
without such an arm would inherit the bug silently.

### Why a helper rather than 25 open-coded conditionals

The predicate is a policy, not a detail: which words mean "I am asking for
help" is the sort of thing that gets extended (`man`? `usage`?), and 25 copies
would be extended in 3 places and drift in the other 22. It also gives the
checker something to key on — `scripts/check-usage-status.py` now treats a call
to `end_help_arm` as accounting for a site, matched *by name* rather than by
inlining a copy of what the helper does, so the checker cannot drift away from
the helper it is describing.

**Against:** the status is now set somewhere other than the arm you are
reading, which is exactly the indirection that makes `sed_usage`-style helpers
need an allowlist entry explaining that their callers handle the status. The
difference is that this helper sets the status itself rather than relying on
callers to remember, which is the failure mode the allowlist entry exists to
guard against. Accepted.

### The methodological point, which is the same one as §296

§296 concluded that a sweep needs a second search aimed at the damage the sweep
itself can do. This one adds the other half: **a sweep must not restate the
rule that found its targets.** The transformer's first draft wrote its own
version of "an unaccounted usage site" instead of importing the checker's, and
immediately disagreed with it — selecting, in `cmd_wakesensor`, an inner arm
that already set the status correctly. Two descriptions of one set, maintained
separately, differ silently and neither can report the difference. The rewrite
imported the checker outright, which also meant the transformer's site count
and the checker's debt count could not disagree: 33 and 33, by construction
rather than by coincidence.

The complementary rule is that the mechanical pass must be allowed to *refuse*.
It rejected `cmd_datausage` because its governing match was
`match parts.get(1).copied()` rather than the uniform `match sub`, which is the
difference between an arm that discards a subcommand and one whose `_` covers
both `None` (a query) and `Some(bad)` (an error). A transformer that had merely
tried its best there would have produced a fourth instance of the mirror-image
bug.
