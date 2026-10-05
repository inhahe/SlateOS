## A-KSHELL-A-BARE-QUOTA-ANSWERED-AND-REPORTED-FAILURE — ✅ FIXED 2026-08-25 (lane A)

**In short:** typing `quota` on its own printed "Quotas are currently enabled."
— the correct answer to the question — and then told the shell the command had
failed. A script that runs `quota && something` never ran `something`, even
though nothing went wrong. Found by a new checker written for exactly this
shape, which is the third instance of it after `elog echo` and `fc algo`.
While fixing it, a second, opposite bug turned up one screen further down in
the same command: `quota banana` scolded the user and reported *success*.

**Where:** `kernel/src/kshell.rs`, `cmd_quota`.

**The gate.** `scripts/check-usage-status.py` has since August guarded one
direction — a diagnostic that reports success. Nothing guarded the other, and
`A-KSHELL-A-QUERY-THAT-ANSWERED-CORRECTLY-REPORTED-FAILURE` recorded that gap
explicitly, because it is the direction the August sweep itself got wrong twice.
`scripts/check-query-status.py` closes it. Its rule:

> A block that can only be reached by the user *asking* — no argument was given
> — and that answers by printing program state must not set a failure status.

Three questions per non-zero `set_exit`: is the enclosing block guarded on "no
argument given"; does at least one print *directly* in that block interpolate
program state (a `foo::bar()` call, not just literal text or the user's own
words echoed back); does it then fail. All three, and the site is reported.
Run from `boot-test.sh` immediately after the usage-status gate.

**Deliberately a second script, not a second rule inside the first.** The two
rules point in opposite directions — "this needs a status", "this must not have
one" — and a single classifier holding both would resolve a disagreement between
them silently. Kept apart, each states a property, and a site that trips both is
a site whose author has to say which it is. `quota` is exactly that site: taking
the status off for the query direction made it trip the usage-status gate, which
is how its `ALLOWED` entry came to be written with a reason attached.

**Verified in both directions before being trusted.** Run against the revision
before the August fix (`git show 9251e5a3d^:kernel/src/kshell.rs`) it reports
`cmd_fcompress` and `cmd_elog` — the two sites that actually shipped the bug —
and `cmd_quota`. Run against the tree today it reports `cmd_quota` alone, which
is how the bug was found. A checker nobody has watched fail is a checker nobody
knows works, so the invocation is written into the script's docstring.

**Fix, half one — the query.** Bare `quota` is a question and this is its
answer, the same reading as `elog echo` and `fc algo`. The `set_exit(1)` is
gone, and the two lines swapped: the state line leads and the synopsis follows
it as a hint ending "to change". The order is part of the fix, not tidying —
printed the other way round the output *reads* as a complaint, and the next
person sweeping usage lines will put the status back.

**Fix, half two — the typo, and a blind spot it exposes.** `cmd_quota`'s
catch-all arm printed

```rust
shell_println!("Unknown subcommand '{}'. Use: on, off, set, …", parts[0]);
```

with no status at all: `quota banana` reported success. The usage-status gate
never saw it because the gate keys on the *word* `Usage:`, and this arm says
`Use:`. The arm now sets `set_exit(1)` and is worded `Usage:` so the gate can
see it.

**That blind spot is general** — and was closed the same day, one commit later.
Re-running the usage-status
walk with its trigger widened from `Usage:` to `Unknown…`/`Unrecognised…`/
`Invalid…`/`Use: ` reports **49 further sites** in `kshell.rs` that print a
diagnostic and report success — `cmd_container`'s seven `Invalid container ID`
paths, `cmd_wakesensor`'s five `Unknown sensor` arms, `cmd_theme`, `cmd_progmgr`,
`cmd_secpolicy`, and more. (Two of the 51 raw matches are report lines rather
than diagnostics: `thumbcache`'s "Invalidated {} entries" and `vlan`'s "Unknown
drops:" counter.) This is the same lesson a third time — a gate keyed on the
shape of the last bug defines its own blind spot. All 49 are fixed and the
trigger is widened; see
`A-KSHELL-DIAGNOSTICS-NOT-WORDED-USAGE-ESCAPE-THE-GATE` below and
design-decisions.md §299.

**Tested by:** `kshell::self_test` rung 60 — `quota` prints the state line and
exits 0, the synopsis survives as a hint, and `quota zz_not_a_subcommand` names
the word and exits 1.
