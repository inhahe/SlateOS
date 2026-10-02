## `A-KSHELL-A-TOGGLE-WORD-IT-DOES-NOT-RECOGNISE-MEANS-OFF` (lane A, 2026-08-25) — **fixed 2026-08-29**, 21 + 37 sites in 35 commands

**In short:** 21 shell settings are switched with a word — `on`, `off`, `yes`,
`no`. Each tests the word against a short list of "true" spellings and treats
**everything else** as *off*. So a spelling the list happens to omit does not
produce a complaint; it silently turns the setting off and reports success.
The lists are also inconsistent between commands, so `1` means *on* in nine of
them and *off* in the other twelve.

**Where.** `kernel/src/kshell.rs`, 21 instances of

```rust
let on = matches!(parts[1], "on" | "true" | "yes" | "1");
```

in `cmd_progmgr` (3), `cmd_wakesensor` (2), `cmd_battery` (2),
`cmd_appnotify` (2), and one each in `cmd_useracct`, `cmd_tasksched`,
`cmd_swapcfg`, `cmd_restorepoint`, `cmd_reslimit`, `cmd_osreset`,
`cmd_nightlight`, `cmd_kernelbuild`, `cmd_fileshare`, `cmd_datausage`,
`cmd_crashreport`, `cmd_bootcfg`.

**The vocabularies actually in use**, which is the part that makes this more
than a missing `else`:

| Accepted as true | Commands |
|---|---|
| `on` `true` `yes` `1` | 9 |
| `on` `yes` `true` | 8 |
| `on` `yes` `true` `enable` | 2 |
| `on` `true` `connected` | 1 |
| `on` `true` `yes` `1` (nested) | 1 |

Nothing anywhere accepts `off`/`no`/`0` *explicitly* — they are simply the
absence of a true word, which is also what a typo is.

**Why it is a defect and not a style.** It is the same rule as
`A-KSHELL-…-GUESS-A-VALUE-FOR-A-WORD-THEY-COULD-NOT-READ`, in boolean clothing:
a word the command could not make sense of is answered with a value of its own
rather than a refusal. The difference is that a boolean has only two values, so
the guess is *always* the opposite of one of the two things the user might have
meant, and there is no unreadable-looking output to hint that anything went
wrong.

**The direction is sometimes the unsafe one.** Most of these fail towards
"off", which is usually the conservative answer. Not all:

- `datausage limit block <name> 1` — `1` is not in that command's list, so
  `block` becomes `false`. The operator asked for traffic over the cap to be
  **blocked** and got no block, with `Block-on-exceed for 'x': false` printed
  as confirmation.
- `kernelbuild auto <id> 1` — same list, so automatic rebuilds are silently
  left off for a component the operator just turned them on for.

**The fix.** One shared helper in the `required_u32` / `optional_u32` family —
`required_bool(parts, idx, cmd, sub)` — accepting one agreed vocabulary in both
directions (`on`/`true`/`yes`/`1`/`enable` and `off`/`false`/`no`/`0`/`disable`)
and refusing anything else with `is not on or off`. That fixes the silent-guess
half and the inconsistent-vocabulary half in the same edit, which is the reason
to do it as one helper rather than 21 local `match`es.

**Found while** clearing the `cmd_reslimit` D1 sites; `reslimit enforce <gid>
<on|off>` is one of the 21 and was deliberately left for this entry rather than
folded into a numeric-operand commit.

**Not a regression.** True since each site was written.

**Fixed in two halves, because the defect had two spellings.**

The first half (`df9252ffc`, 2026-08-26) cleared the 21 `matches!` sites
described above. `toggle_word` became the one place that decides which words
mean what; `required_toggle` reads an operand with no query form, `toggle_arg`
one that has one. Pinned by `kshell::self_test` rung 79.

The second half (2026-08-29) cleared **37 more sites across 19 further
commands** that this entry missed entirely, because they were written as `==`
chains rather than `matches!` and so did not match the grep that produced the
count in the title:

```rust
let on = parts[2] == "on" || parts[2] == "true";              // 16 sites
let val = parts.get(2).copied().unwrap_or("");  … val == "on" //  6 sites
let on = parts.get(1).copied().unwrap_or("on") == "on";       //  9 sites
if let Some(v) = parts.get(1) { let on = *v == "on" || …; }   //  6 sites
```

The 19 commands: `schedtune`, `mmtune`, `vpn`, `ia11y`, `usbpolicy`,
`touchscreen`, `policyengine`, `colorblind`, `gestures`, `energysaver`,
`vpnprofile`, `taskbar`, `installer`, `screenlock`, `oobe`, `wallpaper`,
`location`, `spatialaudio`, `appdefaults`.

An `||` chain of `==` has the same two outputs as `matches!` and the same
missing third, so the analysis above applies unchanged. Two of the new sites
fail in the unsafe direction, as `datausage` and `kernelbuild` did in the first
half: `vpn killswitch <id> 1` **disabled** the kill switch it was asked to
enable, and `policyengine enforce 1` turned enforcement off — both printing the
setting as applied and exiting 0. `usbpolicy block 1` is the same shape.

Three things came out of the second half that the first did not have to
consider:

- **`optional_toggle` had to be added.** Nine of the new sites read
  `unwrap_or("<default>")`, i.e. a documented value for an *absent* operand.
  `toggle_arg` is wrong for those (absence there means *report*), so the
  boolean twin of `optional_num` now draws the same absent-vs-unreadable
  distinction: `installer user 1 alice` still means "password login, no
  autologin"; `installer user 1 alice ys` is refused.
- **`taskbar config` echoed the input rather than the flag.** Its four toggles
  printed `val` — the word the user typed — so `taskbar config names 1` printed
  `Show names: 1` while storing *no*, and the bare `taskbar config names`
  printed `Show names: ` with nothing after the colon, having turned names off.
  An output that quotes its input cannot disagree with it, so reading the
  transcript could never have revealed the guess. They now read the flag back.
- **Two sites were queries that mutated.** `spatialaudio headtrack` and
  `doppler` read `unwrap_or("on")`, so the shortest way to *ask* what the
  setting was **enabled** it. Both now take the `toggle_arg` query path the
  rest of the shell gives a `[on|off]` operand. `location history <word>`
  entered its set arm only for the two exact words `on`/`off` and fell through
  to the history *listing* for everything else, so a request to start recording
  was answered with the last ten fixes.

Also folded in, because they were the same statement: `ia11y mouse on 4O`
dropped the speed with `if let Ok(speed) = …` under a success line, and
`energysaver autoswitch on 2O` guessed a 20% threshold via
`unwrap_or(20)`. Both now refuse.

Pinned by `kshell::self_test` rung 97, whose controls run against
`taskbar config names` for the §604 reason — `taskbar::set_show_names` is a
lock-and-assign with no `Result` and no subsystem init behind it, so a success
assertion there cannot fail because a fresh boot has not initialised something.

**The lesson for the next sweep of this shape.** The title of this entry said
"21 sites" for four days and was wrong by 35, because the census was a grep for
one *syntax* rather than for the *defect*. A boolean read that cannot report
failure looks like `matches!`, like `a == x || a == y`, like
`unwrap_or(d) == x`, and like `if let Some(v) = … { v == x }`; counting the
first spelling and stopping produced a number that read as complete. Count the
call sites of the thing being set, not the instances of the idiom you noticed
first.
