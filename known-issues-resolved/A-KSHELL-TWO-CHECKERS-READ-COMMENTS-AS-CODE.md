## `A-KSHELL-TWO-CHECKERS-READ-COMMENTS-AS-CODE` (lane A, 2026-08-25) — ✅ **FIXED** 2026-08-25

**In short:** two of the build's own gate scripts searched the source with
plain text matching and never skipped comments — so an English sentence
*about* code counted as the code. One shell command had an option loop with no
way to refuse an unknown flag, and the gate that exists to catch exactly that
had been quietly passing it for as long as it existed, because a comment next
to it contained the words it was grepping for.

**Where.** `scripts/check-option-refusal.py` and `scripts/check-usage-status.py`.
Both read `lines[i]` raw and ran a regex over it.

**How it hid.** The comment in `cmd_fsck_ext4` read, in full:

```rust
// No `set_exit(1)`: `--help` succeeded at what it was asked.
```

`check-usage-status.py` looks for `set_exit(` near a usage message to decide
the command reports failure. `check-option-refusal.py`'s D3 detector looks for
the same token to decide an option loop can refuse. **A comment stating that
there is deliberately no `set_exit(1)` here satisfied both greps for
`set_exit(1)`.** The clearer the note, the more completely it defeated the
check — and "explain why this arm deliberately does *not* do X" is exactly
what a careful author writes.

The same shape ran the other way in `check-option-refusal.py`'s `ALLOWED`
table, which carried two exemptions whose stated reason was *"prose in the doc
comment describing the bug that was removed, not code"*. Two hand-granted
exemptions for the same root cause is the signal that it is a defect in the
detector, not a series of special cases — and the workaround it was pushing
toward (stop writing down what the bug was) would have deleted the single most
valuable line in each of those fixes.

**What it was hiding.** With comments stripped, the option-refusal checker
immediately reported `cmd_fsck_ext4`'s flag loop, which had no refusal at all:

| Invocation | What happened |
|---|---|
| `fsck.ext4 /dev/sda --verbse` | the typo fell through to `device = w`, so it checked a device *named* `--verbse` |
| `fsck.ext4 --verbse /dev/sda` | worse — the real device overwrote the typo, so the check ran, verbosely-not-verbose, with nothing to say a flag had been ignored |
| `fsck.ext4 /dev/sda /dev/sdb` | the second device silently replaced the first |

All three now refuse. Pinned by rung 65.

**The fix.** Both checkers strip comments before matching, blanking string
literals first so the `//` inside a `"https://…"` is not mistaken for one.
The two prose exemptions were deleted, since the detector no longer needs
them: 3 allowed → 1.

**One further consequence worth keeping.** Once the `--help` arm was correctly
seen as a usage message that returns 0, it needed a real exemption — and it
could not have one, because `check-usage-status.py`'s `ALLOWED` is keyed by
enclosing function plus a fragment of the line, and the help arm and the
missing-device arm printed *character-for-character the same* synopsis. One
entry would have exempted both, so removing the `set_exit(1)` from the error
arm later would have gone unnoticed. The text moved into an
`fsck_ext4_usage()` formatter — the precedent six other entries in that table
already follow — so the exemption names only the formatter, and the two
callers differ in the one way that matters and that duplicated text cannot
show: their exit status.

**Residual.** Both checkers now strip comments with a *line-local* scanner,
which does not understand `/* … */` block comments or raw strings. A block
comment containing `set_exit(1)` would still fool them. `scripts/rust_scopes.py`
already has a correct lexer (`_strip`) that tracks block-comment nesting and
raw-string hash counts across lines — but it discards string literals too, and
these detectors need those (`USAGE` matches `"Usage: …"`, `OPTION_LITERAL`
matches `"-x"`, D2 matches `.starts_with('-')`). The proper fix is a
`keep_strings` mode on `_strip` and a shared `code_only(lines)` helper that all
the detector-style checkers use, replacing both ad-hoc copies. Filed as
`TD-A-CHECKERS-STRIP-COMMENTS-WITH-A-LINE-LOCAL-SCANNER` below.

**Not a regression.** Both checkers were blind from the day they were written.
