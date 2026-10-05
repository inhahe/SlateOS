## `A-KSHELL-THE-USAGE-GATE-WALKS-STRAIGHT-THROUGH-} else {` (lane A, 2026-08-25) — **FIXED** 2026-08-25, all 195 sites resolved

> **Resolution.** The walk is now `walk_block` in `check-recursive-locks.py`,
> the directory's shared self-tested scanner, counting braces a character at a
> time and handing each caller a per-line *column range* rather than a line —
> text outside the range belongs to a sibling block, which is the whole bug.
> It also takes the match's column, so a one-liner is not walked from column
> zero and does not count the `{` its usage print is already inside. Five
> regression cases, all of which a line-granular count answers wrongly.
>
> Rewiring `check-usage-status.py` onto it took the gate from 0 findings to
> 195 — the number this entry predicted from an in-process probe, reproduced
> independently by the real fix. Of the 195: **192** were the mechanical
> "print a synopsis, exit 0" and got their `set_exit(1)`; **3** would have
> been made worse by one and are handled in their own commit —
>
> * `bright set [display_id] <level>`, the one synopsis here whose *optional*
>   operand comes first. Reading `parts[1]` as the display made the documented
>   one-word form print its own usage line, and under that sat the guess:
>   `bright set 1 abc` parsed to 0, turned the backlight off, said
>   `Brightness → 0%` and exited 0. Both operands now go through
>   `required_u32`.
> * bare `cred autofill`, which claimed in a comment to list every autofill
>   rule and did not — it called `list_all()` (the *credential* list), dropped
>   it with `let _ = all`, and printed two hint lines alone. Now lists, via a
>   new `credentials::list_all_autofill()`.
> * bare `lockdep`, which prints its synopsis *as* its output, like `ksyms`.
>   Correct as it stood; now in `ALLOWED` with the reason.
>
> Pinned by `kshell::self_test` rung 67: one command per guard shape, each
> paired with a control naming an operand that does not exist, which must not
> print a synopsis — so the rung cannot pass by having broken the command.
>
> **The rule this produced, which is the part worth keeping:** a walk over a
> *balanced* block (start on its `{`, stop when depth returns to 0) is safe at
> line granularity, because `} else {` nets to zero truthfully. Only a walk
> that must notice depth going *negative* — leaving a block it never entered —
> is broken by it. That rule cleared the other three line-granular counters in
> `scripts/`, and it is what the shared helper's docstring now states. **It
> did not clear `check-option-refusal.py`, whose problem is a different
> granularity bug in the same family** — see
> `A-KSHELL-THE-OPTION-GATE-COUNTS-ONE-LINE-AND-RUSTFMT-USES-FOUR` below.

**In short:** the gate that exists to catch "the shell printed a complaint and
then told the caller it succeeded" counts curly braces one *line* at a time.
The line `} else {` closes one block and opens another, so a line-at-a-time
count nets to zero and the walk never notices it left the block it was asked
about. It carries on into the `else` branch, finds the `set_exit(1)` that lives
there, and marks the site as accounted for. **195 real sites across 50 shell
commands are invisible to it for this reason** — and the gate reports itself
clean.

**Where.** `scripts/check-usage-status.py`, the forward walk in `main`:

```python
depth += struct[k].count("{") - struct[k].count("}")
if depth < 0:
    break
```

**The shape it cannot see.** Every one of the 195 is this, give or take the
condition:

```rust
"enable" => {
    if parts.len() < 2 {
        shell_println!("Usage: scriptlang enable <engine_id>");
        //             ^ no set_exit: falls out of the `if` and off the arm
    } else {
        match parts[1].parse::<u64>() {
            Ok(id) => match scriptlang::set_enabled(id, true) {
                Ok(()) => shell_println!("Enabled engine {}", id),
                Err(e) => {
                    shell_println!("Error: {:?}", e);
                    set_exit(1);      // <-- what the walk finds instead
                }
```

`scriptlang enable` with no operand prints its synopsis and exits **0**. So
does `bootcfg params`, `wallpaper animated`, `ime reg`, `tmon priority`,
`appnotify critical`, and 189 others.

**How it was found.** Not by the gate, and not by reading it — by reading the
*code* it guards. `cmd_appnotify`'s `critical` arm was obviously missing a
status, and the gate said the file was clean, so one of the two had to be
wrong.

**This is the third time this mirror pair has been out of step, and the second
time on exactly this axis.** `check-query-status.py` — the mirror of this
script, guarding the opposite rule — counts braces *character* by character,
and its `block_start` docstring says why, in as many words:

> Braces are counted a character at a time, not a line at a time. `} else {`
> closes and opens on one line, so a line-granular count nets to zero and the
> walk sails straight past the brace that actually delimits the block.

One script of the pair wrote the hazard down and defended against it. The other
had the same hazard, in the same file, in the forward direction, and neither
script can see the other. (The previous instance was comment-blindness, fixed
2026-08-25 in `A-KSHELL-TWO-CHECKERS-READ-COMMENTS-AS-CODE`; the entry above
notes that the mirror pair had silently drifted apart there too.)

**Measured impact.** Re-running the checker with the walk made
character-granular, changing nothing else:

| | sites reported |
|---|---|
| as shipped | 0 |
| character-granular | 195 (50 functions) |

Worst-affected: `cmd_useracct` 15, `cmd_progmgr` 14, `cmd_appnotify` 14,
`cmd_netsettings` 11, `cmd_kernelbuild` 11, `cmd_scriptlang` 11,
`cmd_bootcfg` 10, `cmd_vdesktop` 9, `cmd_osreset` 8, `cmd_soundmixer` 8.
Five drawn at random were checked by hand and all five are true positives.

**Why it matters.** The same reason the 710-site sweep mattered, quoted from
this checker's own docstring: *the diagnostic is on the screen either way; it
is the status that lies.* `cmd && next` runs `next` after a typo, `set -e`
does not stop, an `ERR` trap does not fire.

**The fix.** Two parts, and the second is the work:

1. Make the walk character-granular, the way its mirror already is. Better
   still, lift the brace walk into the shared, self-tested scanner both
   scripts now import (`check-recursive-locks.py`), so the pair cannot drift
   on this axis a fourth time — the hazard has now been discovered
   independently three times and written down twice, which is the signal that
   it belongs in one place rather than in two docstrings.

   A sweep of every brace counter in `scripts/` found three more written at
   line granularity — `check-option-refusal.py`'s `loop_bodies`,
   `check-self-tests-wired.py`'s nesting stack, and `scan-unwired.py`. **None
   of those three is wrong today**, and the reason is worth recording because
   it is the rule for the shared helper: they scan *balanced* blocks — start
   at the opening brace, walk to where the depth returns to zero — and
   `} else {` is balanced, so it nets to zero correctly. Only a walk that must
   detect the depth going **negative**, i.e. leaving a block it did not enter,
   is broken by line granularity, because the dip happens *within* the line
   and a line-sum never sees it. `check-usage-status.py` is the one script of
   the four doing that, which is why it is the one that is wrong.
2. Triage the 195. Most want a plain `set_exit(1);`. A few are the documented
   exception — a bare invocation printing its own synopsis *is* the command's
   output, the `ksyms`/`scrollback` precedent — and those belong in `ALLOWED`
   with a reason, not silently.

   Classified by the guard directly above each diagnostic, the population is
   narrower than "195 assorted sites" suggests:

   | Guard | Count |
   |---|---|
   | `if parts.len() < N {` | 111 |
   | an emptiness test on the operand (`.is_empty()`) | 68 |
   | `if <id> == 0 {` after a `parse().unwrap_or(0)` | 15 |
   | one-offs | 1 |

   Every one of those guards means *the operand the subcommand needs is not
   there*, so essentially all 195 are complaints and want a status; the
   `ksyms` exception does not appear in this population at all. Note the third
   row: those fifteen are the D1 guess (`parse().unwrap_or(0)`) and this
   defect stacked — the parse invents `0`, the `== 0` test then treats an
   unreadable id as a missing one, and the resulting complaint exits 0.

   The single "one-off" is `cmd_brightness`'s `set` arm, and it is worth
   reading because all three defects are visible in six lines:

   ```rust
   "set" => {
       let id    = parts.get(1).and_then(|s| s.parse::<u32>().ok()).unwrap_or(1);
       let level = parts.get(2).and_then(|s| s.parse::<u32>().ok()).unwrap_or(0);
       if level == 0 && parts.get(2).is_none() {
           shell_println!("Usage: bright set [display_id] <level>");
       } else {
           match brightness::set_brightness(id, level) { … }
   ```

   - `bright set 50` — the documented `[display_id]` form. `50` is read as the
     *display id*, the level is absent, and the command prints its usage
     instead of setting anything. **The optional-argument form does not work
     at all.**
   - `bright set 1 abc` — the level becomes `0`, `parts.get(2)` is `Some`, so
     the guard is skipped and the screen is set to **0% brightness**, reported
     as `Brightness → 0%` with status 0.
   - `bright set 1` — prints the usage line and exits **0** (this defect).

**Not a regression.** True since the checker was written; every site it hides
predates it.
