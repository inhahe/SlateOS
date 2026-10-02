## A-KSHELL-A-USAGE-ARM-THAT-FALLS-OUT-OF-A-MATCH-REPORTED-SUCCESS — ✅ FIXED 2026-08-25 (lane A)

**In short:** the same lie as
`A-KSHELL-A-MISTYPED-COMMAND-REPORTED-SUCCESS` below, in the 87 places that
one could not see. Give a kernel-shell *subcommand* a bad value or leave off
its argument — `speech tts banana`, `dynlock grace`, `wallpaper bgcolor` — and
it printed `Usage: …`, did nothing, and reported success. The diagnostic was
on the screen the whole time; it was the exit status that lied, which is the
worse half, because a script reads the status and not the screen. `cmd &&
next` ran `next` after a typo; `set -e` did not stop.

**Why the earlier sweep missed them.** That sweep matched a `Usage:` print
followed by a bare `return;`. These have no `return` — they are the whole body
of a `match` arm, so control leaves by falling off the end of the arm:

```rust
_ => shell_println!("Usage: dynlock autounlock <on|off>"),
```

A search for the fixed shape cannot see the broken one. The two are the same
defect wearing different syntax, which is the general lesson: a sweep keyed on
a *syntactic* pattern silently defines its own blind spot, and the way to find
the blind spot is to re-derive the site list from the *semantic* property —
here "a `Usage:` print that can be reached and left without a non-zero
`set_exit`" — rather than from the shape that was fixed last time. That is
what `scripts/check-usage-status.py` now does.

**Where:** `kernel/src/kshell.rs`, 87 sites in three shapes:

| Shape | Count | Example |
|---|---|---|
| `_ => println!("Usage: …"),` — unrecognised value | 37 | `dynlock autounlock` |
| `None => println!("Usage: …"),` — argument omitted | 27 | `dynlock grace` |
| the print inside a braced block, not the whole arm | 23 | `wallpaper bgcolor` |

**Fix:** `set_exit(1)` after the diagnostic, in every one. The message is kept
— the point was never that the shell said too little, it is that it then
claimed to have succeeded.

**The mirror-image defect — `A-KSHELL-A-QUERY-THAT-ANSWERED-CORRECTLY-REPORTED-FAILURE`,
✅ FIXED 2026-08-25 alongside this one.** Searching the
*patched* tree for "an inserted status sitting in a block that prints something
before the usage line" turned up two sites where the **August sweep had already
made this mistake and shipped it**:

```rust
"echo" => {
    if parts.len() < 2 {
        shell_println!("Serial echo level: {} (and above)", level.as_str());
        shell_println!("Usage: elog echo <level>  to change");
        set_exit(1);          // <- a query, answered correctly, reported failed
        return;
```

`elog echo` and `fc algo` are not conflations at all: the branch is guarded by
`parts.len() < 2`, so it is reached *only* with no argument — purely the query
path. The usage line there is a hint appended to a correct answer, not a
diagnostic; `elog echo`'s own text says "to change". Both have reported failure
for a successful query since August. **Fixed** by deleting the `set_exit(1)` —
the command did what was asked — and both are pinned by rung 58, which asserts
the query prints its answer *and* exits 0. They are listed in the checker's
`ALLOWED` rather than left to trip it, because the next person sweeping usage
lines will find them again and that list is where the answer needs to be
waiting.

**The unmechanised half — ✅ MECHANISED 2026-08-25 as `scripts/check-query-status.py`.**
`scripts/check-usage-status.py` checks one direction: a diagnostic that claims
success. The other — a correct answer that claims failure — went unchecked, and
was the direction that had produced both shipped bugs. It now has its own gate,
run from `boot-test.sh` next to the first, and the gate found a third instance
on its first run. See `A-KSHELL-A-BARE-QUOTA-ANSWERED-AND-REPORTED-FAILURE`
below.

**One case the mechanical pass had to be pulled back from,** because it is the
shape where the obvious patch is a *regression*:

```rust
} else {
    shell_println!("Current mode: {}", notifgroup::get_mode().label());
    shell_println!("Usage: notifgroup mode <app|category|conversation|none>");
}
```

That `else` is reached two ways: with no argument at all (a bare query — "what
is the mode?" — correctly answered, and a success), and with an argument that
did not parse (an error). Stapling `set_exit(1)` on would make the successful
query report failure. Five sites have this shape — `notifgroup mode`,
`faceunlock security`, `datausage metered`, `wallpaper offset`, `fhist
autoversion` — and they are folded into the still-open item below rather than
patched, since splitting query from error is the same decision.

The last two of those were **patched by the mechanical pass and then
reverted**, caught only by reading the diff line by line rather than skimming
it. The script's guard was "only patch a print that ends its block", which is
syntactic, and it passed all five. What distinguishes them is not visible in
the shape of the arm at all — it is in the enclosing conditional, which decides
whether "no argument" reaches this branch.

**Tested by:** `kshell::self_test` rung 58, which pins one site per shape *and*
the good path of a patched command — the way a mechanical insertion goes wrong
is by landing in the sibling arm that succeeded, so the control is the part
that actually earns its place.

**The help-vs-unrecognised conflation (33 sites) — ✅ FIXED 2026-08-25**, see
`A-KSHELL-A-HELP-ARM-AND-A-TYPO-REPORTED-THE-SAME-THING` below. These were the
residue this sweep could not touch: a status could be added to them, but no
*correct* status existed until the arm was split.

**Deliberately excluded, and why** — these match a `Usage:` search but are not
this defect: `cmd_usagetime`'s `"Usage time subsystem initialised."` and
`cmd_memcg`'s `"  Usage:        {}"` report field (neither is a usage message
at all); `cmd_scrollback`'s `""` arm and `cmd_ksyms` (bare invocation printing
its own synopsis *is* the command's output, the documented `ksyms` precedent).
