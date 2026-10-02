## `A-KSHELL-THE-USAGE-GATE-KNEW-FIVE-WORDS-AND-A-SYNOPSIS-NEED-USE-NONE-OF-THEM` (lane A, 2026-08-25) — **FIXED** 2026-08-25, all 57 sites resolved

**In short:** the gate that checks "if a command complains about what you typed,
it must also report failure" decided what counted as a complaint by looking for
five words — `Usage:`, `Unknown`, `Unrecognised`, `Invalid`, `Use:`. A message
that says the same thing without any of them was invisible to it. Typing
`pmgr zznosuchsub` printed the list of real subcommands and then exited 0;
`cam privacy 1 zzmode` printed `Modes: open, prompt, disabled` and exited 0.
57 sites hid there. To a user those read exactly like refusals. To a script
they read as success.

**Fourth instance of one bug, and the first that is not about line
granularity.** The three before it were all "counted by the line what should
have been counted by a statement or a column walk." This one is the same
*shape* of error one level up: the gate was complete over the thing it could
name, and the thing it could name was not the thing it was for.

| # | What the gate was actually complete over | What it was for | Hidden |
|---|---|---|---|
| 1 | `{`/`}` depth counted per line, where `} else {` nets to zero | a character-column walk (`walk_block`) | 195 |
| 2 | a regex spanning `.parse().ok().unwrap_or(…)` on one line | a statement (`statements`) | 466 |
| 3 | a regex spanning `shell_println!(` and its first argument on one line | a statement | 192 |
| 4 | **messages containing one of five words** | **diagnostics** | **57** |

**How it was found.** Not by an audit — by reading `cmd_partmgr` to start the
*next* guessed-value burn-down and noticing that its catch-all arm prints
`partmgr: disks/adddisk/rmdisk/table/…` and falls out with no status, while the
usage gate was reporting clean. Instance 3 was found the same way, from the same
command. An anomaly noticed in passing has now beaten a deliberate audit twice,
which is worth remembering: the gates are read far more often than they are
re-derived, and a green gate is not evidence about what it does not look at.

**The 15 slash-form commands are one author-batch.** `power`, `display`,
`vdesktop`, `keylayout`, `screenshot`, `a11y`, `ime`, `netindicator`, `winsnap`,
`colorpicker`, `cursorsettings`, `kbsettings`, `detailcols`, `partmgr`,
`locale` — dispatched from *consecutive* lines 7369–7383. They share a
convention nothing else in the file uses: one line naming every subcommand
separated by slashes, no `Usage:` prefix. That is why the miss is clustered
rather than scattered, and why widening the trigger by one shape recovered all
fifteen at once.

**Fix.**

* `scripts/check-usage-status.py`'s `USAGE` gained a second alternative for the
  enumeration shape: `Label: a/b/c` or `Label: a, b, c` with at least three
  members. Two structural rules keep help *bodies* out, both established by
  measurement rather than by taste — a naive version of this trigger fired on
  65 ordinary help lines:
  * the list must begin the message after at most a **single-word label**, so
    an indented `"  arrange <sort>   Auto-arrange (name/size/type/date)"` is
    not a match;
  * the label is **one word**, so `"cut -d/-f/-c  Extract columns"` is read as
    the flag list it is, not as a label.
* The 57 were triaged by hand into five shapes, not fixed mechanically:
  * **15** slash-synopsis catch-alls, now ending in `end_help_arm(cmd, sub)` —
    which sets 1 only for a subcommand that was not itself a request for help.
  * **6** help-body catch-alls whose only enumerations are trailing
    `Foo: a, b, c` lines (`autostart`, `schedtune`, `mmtune`, `vpn`, `dyndns`,
    `loginscreen`), same fix.
  * **18** value-list refusals that knew the word was unusable, printed the
    alternatives, and returned success. Each now names the unusable word —
    ``cam: privacy: `zzmode' is not a mode`` — and sets 1. Naming it matters:
    the list alone does not say *which* of several operands was rejected.
  * **2** `cmd_sysdiag` arms that already failed but set the status between the
    complaint and the list explaining it. Reordered, as `cmd_overlay` and
    `cmd_container_network` were in instance 3.
  * **2** genuinely correct sites added to `ALLOWED` with reasons: `cmd_profile`
    (bare `profile` reports the current profiles and then names the ones that
    exist — a query, guarded by `arg.is_empty()`) and `cmd_colorpicker model`
    (the `parts.get(2)` else-branch is asking what the models are; its sibling
    branch prints the same list under a refusal and does set 1).
* `cmd_schedtune` and `cmd_mmtune` were fixed **structurally rather than
  allowlisted.** Their help bodies live in a nested `#[inline(never)] fn case()`
  whose block the walk cannot leave, so the `end_help_arm` sitting outside it
  was invisible — the same situation `cmd_firewall` and `cmd_container` carry as
  exemptions. Passing `sub` into `case` and making `end_help_arm` its last
  statement removes ten findings without adding ten `ALLOWED` entries. Removing
  an exemption beats documenting one; the two older ones should be converted the
  same way when they are next touched.
* `kshell::self_test` rung 70 covers one command per shape, each refusal paired
  with a control that runs the same command with a usable operand — because the
  failure mode of *this* fix is to make a correct invocation start reporting
  failure. The rung also re-checks that bare `pmgr`, bare `stune` and bare
  `deskicons arrange` still succeed, since three of the five shapes work by
  adding a status to an arm that also serves the no-argument case.

**What is still not covered (Stage 2).** The trigger is now complete over
messages that *enumerate*, but a catch-all arm can print a help body with no
enumeration at all and still fall out with no status. A survey of all 163
catch-all arms that print without setting a status found roughly 88 such
help-body arms, plus a third shape that must not be treated like the others:
*default-action* arms (`cmd_alloc_trace`, `cmd_irqstorm`, `cmd_filepicker`,
`cmd_startmenu`, `cmd_ipc_stat`) where the catch-all does the command's default
work rather than complaining, and where `end_help_arm` would be wrong. A
structural trigger — "a catch-all containing ≥3 prints" — was tried and
rejected for conflating exactly those two. The remaining work needs the
scrutinee and command name determined by hand for the 10 arms with no detectable
`match` scrutinee and the 41 with no `"<name> — …"` first help line.

**Severity.** Moderate, and the same as instance 3: no correctness consequence
inside the kernel, but a command that complains and exits 0 tells `&&` and
`set -e` that it worked. 57 more of those, on commands including a partition
manager whose subcommands create and delete partitions.
