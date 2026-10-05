## A-KSHELL-AN-OPERAND-READ-AS-A-NUMBER-AND-DROPPED-IN-SILENCE (lane A, 2026-08-29) — **FIXED 2026-08-29**, 22 sites

**In short:** twenty-two shell subcommands took a number as an argument, and
when the word you typed was not a number they did *nothing at all* — no
message, no error status, no action. `ptime enable zzz` printed nothing and
exited 0, which looks exactly like a configuration that really had been
enabled. A script checking the exit status could not tell the difference; nor
could a person reading the screen. All twenty-two now name the word they could
not read and exit non-zero.

**Where.** `kernel/src/kshell.rs`, in the shape

```rust
if let Some(id_str) = parts.get(1) {
    if let Ok(id) = id_str.parse::<u32>() {
        act_on(id);
    }
} else {
    shell_println!("Usage: ptime enable <id>");
    set_exit(1);
}
```

The `else` belongs to the *outer* `if let`, so it covers the absent operand
only. An unreadable one falls out of both branches.

**Why this is worse than the guessed value.** This is the third shape of
design-decisions.md §600, and the burn-down found it last because it is the
only one that leaves no trace. A guessed value (`…parse().ok().unwrap_or(0)`,
the 332 sites still in the ledger) at least *does* something and leaves a wrong
number on screen for someone to notice. A catch-all match arm installs a
visibly-wrong rule. This one produces a clean, empty, successful-looking
non-event.

**The helper.** Neither existing helper fitted. `required_num` refuses an
absent operand, but for several of these the absence is legitimate and means
something different — `mkeys play` acts on the active session, `mkeys play 3`
on session 3. `optional_num` has a default to substitute, and here the absent
case is not a value at all but a different action. So `readable_num` /
`readable_hex` were added: *present, and must be readable*.

**Three neighbouring defects were fixed with them,** each found only because
the arm had to be read closely to convert it, and each capable of surviving a
fix that addressed only the visible half:

| Where | What it did |
|---|---|
| `ptime enable/disable` | `set_active(id, true).ok()` discarded the `Result`, so a readable id naming no config was still announced as "Config 999 enabled." |
| `qs set` | guessed both operands as `0`, then reused the id's guess as the sentinel for "absent" — so `qs set 3 xyz` set the tile to 0% and reported "Set to 0%", and tile 0 was unreachable |
| `container create net=…,gw=<typo>` | assigned `parse_ipv4_octets`'s `None` straight into the field, so a mistyped gateway did not fail to take effect, it **removed** the gateway |

**Resolution.** 22 sites converted (`ptime`, `mkeys`, `cam`, `speech`, `mlink`,
`store`, `slock`, `qs`, `container create`). Self-test rung 103 pins six of
them with *paired* assertions — the refusal named, and the success message
asserted absent — because checking only the refusal would pass against code
that refuses and then acts anyway. D1's ledger fell 335 → 332 as a side-effect.

**The measurement worth keeping: this shape is 56% precise, and the other 44%
had to be read.** The raw pattern matched 39 sites; 17 were correct code doing
something the shape cannot distinguish — trying one reading and falling through
to another:

| Site | The alternative it falls through to |
|---|---|
| `resolve_container_ref` | a numeric reference, else a container *name* |
| `parse_datetime_to_ns` | epoch seconds, else `YYYY-MM-DD` |
| `cmd_useracct info`, `cmd_template` ×2 | a uid/id, else a name — as the usage line offers |
| `cmd_queryable` | "auto-detect type: try int first, then text" |
| `execute_select` | POSIX `select`: non-numeric input leaves the variable empty |
| `expand_brace_expr` ×3 | bash's own `${x:abc}` and `${arr[abc]}` |
| `cmd_fsearch` | a max size, else a root path |
| `parse_sed_command`, `awk_eval_expr`, `cmd_unset`, `cmd_appdefaults` | a later arm, or an `Err` the caller reports |

None of these was excluded by a heuristic. Each was opened and read. That is
the cost of the shape, and it is the reason the gate below is narrower than the
shape.

**The gate.** `scripts/check-option-refusal.py` gained **D4**, which requires
the nesting *and* that the inner `if let Ok` be the **sole** statement of the
`if let Some(w) = parts.get(N)` block. Soleness is exactly what separates the
22 from the 17: a fall-through that reaches more code is an alternative being
tried; a block that ends right there has nowhere for the word to go. So D4
sits at **zero with no ledger entry** — a gate that starts green with no
allowlist, rather than one that starts with a 22-line backlog.

**And D4's first zero was a false one, which is the part worth keeping.** It
landed at zero on `kshell.rs` immediately — and for the wrong reason:
`close_brace` counted braces per line, so `} else {` netted zero, a block with
an `else` never appeared to close, and D4 was skipping most of the sites it
exists for. The file's own output could not say this; a fixture could, and did,
the moment it was written (two defective fixtures unreported, one correct one
reported). `close_brace` is now character-level, and running D4 against the
pre-fix tree reports 20 sites in 8 functions and none of the 17 lookalikes.

This is design-decisions.md §634's corollary applied to a gate written the day
after it — *a gate's own output cannot audit its coverage* — and it is now the
**fifth** time in four days that a gate here was found blind:

| Gate | Blind to | Hid |
|---|---|---|
| `check-shell-portability.sh` (§630) | anything, if shellcheck was absent | the whole bug class |
| `check-option-refusal.py` D1 | a chain rustfmt wrapped | 466 sites |
| `check-option-refusal.py` D1 | `from_str_radix` | 19 sites |
| `check-selftest-wording.py` `LITERAL` | a message rustfmt wrapped | 61 literals |
| `check-option-refusal.py` D4 | any block with an `else` | 20 of the 20 sites it targets |

Four of the five were found by naming a specific thing the gate should catch
and checking that it agreed. The fifth (D4) was found by writing that check
down as a fixture *before* trusting the zero. The fixture is the cheaper method
and the only one that keeps working.

The first row is now **closed** (2026-08-29): §630's missing-tool arm was the
only blindness in the table that was *designed in on purpose*, and its stated
reversal trigger — all three lanes confirmed to have the binary — was verified
and acted on the same day. `check_shellcheck` now exits 1 rather than returning
0 when the tool is absent, so a removed binary stops the build instead of
quietly restoring the blindness.

**Two things this leaves.** D4 does not reach `container create`'s `cpu=`/
`mem=` options, whose word comes from `strip_prefix` inside a loop rather than
from `parts.get`, so there is no positional nesting and no soleness test to
distinguish defect from alternative; those two sites are fixed but ungated.
And `${x:1:abc}` expands to the rest of the string where bash gives the empty
string — a compatibility divergence noticed while reading the 17, not fixed
here because it needs bash's arithmetic-context rules rather than a guess at
them.

**The second of those is now closed** (2026-08-29): the rules were measured
against bash rather than guessed, and turned out to cover *four* divergences
rather than the one noticed here, all failing in the same direction — see
`A-KSHELL-SUBSTRING-OPERANDS-WERE-PARSED-NOT-EVALUATED` at the end of this
file. Worth recording that the deferral was the right call and not merely a
cautious one: the guess anyone would have made — "treat a non-numeric length as
zero" — fixes the reported row and none of the other three.

### A sixth blindness, in the rung rather than the gate — and this one is not fixable by a gate

The boot that verified this batch passed, and rung 103's six assertions all
ran and all held. But the rung was **invisible in the log**: the block was
committed with no `serial_println!` banner, so the serial output went straight
from rung 102 to `kshell::self_test PASSED`. Deleting the entire rung would
have left the log byte-identical.

That is the same shape as the defect the rung was written to pin, one level up.
The batch-39 bug was a command that did its work and said nothing; this was a
*test* that did its work and said nothing. In both cases the thing that was
supposed to make an event observable was the thing that was missing, and in
both cases the surviving evidence — exit 0, `PASSED` — was indistinguishable
from success.

**It is not statically detectable, and the attempt is documented so nobody
repeats it.** The obvious rule — every top-level brace-block containing
assertions must be preceded by a banner — cannot be written, because a sibling
rung missing its banner and a legitimate *scoping* sub-block are the same
syntax. `self_test` has two real scoping blocks (one saves and restores
`KSHELL_SELFTEST_VAR`, one shadows a `PrintfSpec`), and nothing separates them
from an unbannered rung without reading intent. Numbering does not help either:
with no banner at all, the numbering stayed a clean contiguous 1..102.

So what was gated is the invariant that *is* mechanical, and it starts at zero:
`scripts/check-selftest-rung-numbers.py` requires the banner numbers to be
unique, contiguous from 1, and in file order — 103 rungs, 1..103, clean. Its
value is a different failure that is likelier than this one and equally
invisible: **two batches taking the same next-free number against the same
tip.** Both compile, both run, both pass, and a report naming "rung 103" then
points at either — the reference that was supposed to locate a failure is what
makes it ambiguous. Only a reading of all 103 banners finds that.

For the unbannered rung itself the mitigation is not a gate but a habit: after
adding a rung, **read the boot's serial output and confirm the new banner is
there.** This one was caught that way — by checking the log for rung 103 before
pushing, rather than by trusting that a green boot meant the rung had run.
