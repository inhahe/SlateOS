## §296 — The usage-status rule is guarded by a checker keyed on the property, not the pattern, and the checker carries the remaining debt with counts attached

**Date:** 2026-08-25
**Decided by:** Claude (autonomous)
**Lane:** A

**In short:** when a shell command is used wrongly it prints `Usage: …`. It is
supposed to also report failure, so that a script — which reads the exit
status, not the screen — knows the command did not run. Eight hundred places
in the kernel shell were getting this wrong. A sweep in August fixed 710 of
them and declared the job done; it had in fact missed 87, because it searched
for the *shape* the bug had rather than the *rule* being broken. This entry is
about the two choices made when fixing the remaining 87: guard the rule with a
script that runs on every build, and let that script also hold the list of
places still knowingly wrong — with a count next to each, so the list can only
shrink.

### The defect, and why it recurred

The rule is one sentence: **a `Usage:` message printed because the command
could not do what was asked must be accompanied by a non-zero exit status.**

The first sweep (`A-KSHELL-A-MISTYPED-COMMAND-REPORTED-SUCCESS`) found its
sites by searching for a `Usage:` print followed by a bare `return;`. It fixed
every one. But 87 sites are the entire body of a `match` arm and leave by
falling off the end of it, with no `return` to match on:

```rust
_ => shell_println!("Usage: dynlock autounlock <on|off>"),
```

A sweep keyed on a syntactic pattern **defines its own blind spot and cannot
report it**: the sites it cannot see are exactly the sites it does not count,
so it finishes at 100%. That is the general lesson, and it is why the fix here
is not just "patch the other 87."

### Decision 1 — guard the rule with a checker, keyed on the property

`scripts/check-usage-status.py` runs in `boot-test.sh` alongside the existing
source-level gates. For each `Usage:` print it walks forward to wherever
control leaves the enclosing block — a `return`, or the brace that closes it —
and asks whether a non-zero `set_exit` happens first. That is the *semantic*
property, so it does not care what shape the next offender is written in.

Confirmation that this is not self-congratulatory: the checker takes an
optional path argument, and run against the pre-sweep revision it reports
exactly **87** sites — the number independently arrived at by fixing them. A
checker nobody has watched fail is a checker nobody knows works, so the ability
to point it at an old revision is part of the design rather than a convenience.

**Alternatives rejected:**

- **Fix the 87 and write it up, no checker.** Rejected: that is precisely what
  was done last time, and the write-up said the job was complete. The defect
  has now recurred once; the thing that failed was not diligence but the
  method, and a third sweep would fail the same way.
- **A runtime assertion instead of a source check.** Rejected: it can only fire
  on a path a test actually exercises, and the whole population here is error
  paths that nothing exercises. This defect survived precisely because nobody
  runs `speech tts banana`.
- **A clippy lint.** Better in principle — it would understand control flow
  instead of approximating it by counting braces — but a custom lint needs a
  driver built against the compiler's internals and pinned to the toolchain,
  for one project-specific rule in one file. Reconsider if a second rule of
  this kind appears.

### Decision 2 — the checker also carries the known-open debt, with counts

About 33 arms cannot be fixed by adding a `set_exit` at all, because they are
reached from two directions at once. `cmd_nat`'s own comment admits it:
`// "help" or any unrecognised subcommand falls through to the help text.` So
`nat help` (a request, correctly answered — a success) and `nat banana` (an
error) land in the same arm, and *whichever* status it sets, one of the two
callers is told something false. Splitting the arm is a separate change.

They are listed in the checker as `KNOWN_CONFLATED`, **as a mapping from
function name to a count**, not as a set of exempted functions.

- *For:* an exempted function is a hole that grows. Exempt `cmd_bluetooth`
  because of one conflated arm and a genuinely new unfixed arm added there
  tomorrow is swallowed by an entry that was never meant to cover it. With a
  count, the debt can only shrink: fix one and the count must come down with
  it, add one and the check trips. The checker also reports entries that match
  *fewer* sites than claimed, since a stale entry is exempting something it was
  not written for.
- *Against:* the count is a second place the truth lives, so a legitimate fix
  now requires editing the checker too. Accepted — that edit is the point. It
  is a deliberate speed bump on the path that quietly re-grows the debt, and
  the alternative is an allowlist that rots into a rubber stamp.

**Alternatives rejected:**

- **Keep the open sites only in `known-issues.md`.** Rejected: a markdown list
  does not fail a build. The 87 were documented as "very likely not unique to
  `grep`" for a day and nothing acted on it.
- **Suppress by line number.** Rejected outright: line numbers drift on every
  edit, so within a week the list exempts unrelated code.
- **Fold the conflated sites into the ordinary allowlist.** Rejected: that
  allowlist means "this is correct," and these are not correct — they are
  wrong and known. Blurring "fine" with "broken, tracked" loses exactly the
  distinction the file exists to keep.

### Decision 3 — five sites were pulled back out, and the same mistake was found already shipped

The sweep was mechanical, and mechanical is where a regression comes from.
Five arms have this shape:

```rust
} else {
    shell_println!("Current mode: {}", notifgroup::get_mode().label());
    shell_println!("Usage: notifgroup mode <app|category|conversation|none>");
}
```

The `else` is reached both by `notifgroup mode` with no argument — a query,
correctly answered, a success — and by an argument that did not parse, an
error. Stapling `set_exit(1)` on would make a *working query report failure*:
a new bug, introduced by the fix for the old one, in the same class. They are
the same conflation as decision 2 and are tracked with it.

The generalisable part: the script's guard was "only patch a print that ends
its block," which is a *syntactic* guard, and it passed all five of these.
What caught them was reading the enclosing conditional. A mechanical sweep can
be trusted to find candidates and cannot be trusted to approve them.

**And it is not a hypothetical, which is the part worth keeping.** Two of the
five were only noticed because the diff was read line by line rather than
skimmed — `wallpaper offset` and `fhist autoversion`, both of which I had
already patched. Re-running the search as a *query* over the patched tree
("which inserted statuses sit in a block that prints something before the
usage line?") then turned up two more sites where **the August sweep had
already made this mistake and shipped it**:

```rust
"echo" => {
    if parts.len() < 2 {
        shell_println!("Serial echo level: {} (and above)", level.as_str());
        shell_println!("Usage: elog echo <level>  to change");
        set_exit(1);          // <- a query, answered correctly, reported as failed
        return;
```

`elog echo` and `fc algo` are not even conflations: the branch is guarded by
`parts.len() < 2`, so it is reached *only* with no argument — it is purely the
query path, and the usage line is a hint appended to a correct answer, not a
diagnostic. `elog echo`'s text says so outright: "to change". Both have
reported failure for a successful query since August. Fixed separately, since
that is the mirror-image defect and deserves its own change.

So the fix for "prints a diagnostic, claims success" has now demonstrably
produced instances of "answers correctly, claims failure" — in both sweeps,
the first time undetected. The lesson is not "be careful"; it is that **a sweep
needs a second search aimed at the damage the sweep itself can do**, run over
the patched tree, before the result is believed. `scripts/check-usage-status.py`
checks one direction only, and the other direction is not yet mechanised —
recorded in `known-issues.md` as the remaining half.
