### TD-OILS-TRAP-LONE-SIGSPEC. `trap EXIT` showed the EXIT trap instead of taking it away, so a trap the script had cleared still fired — 2026-08-03 — ✅ **RESOLVED 2026-08-03**

**Where:** `userspace/oils/src/interp.rs` — `Shell::builtin_trap`.

**The bug.** `trap` reads its operands by *count*, not by shape: bash's "if arg
is absent and a single signal_spec is supplied, the trap for that signal is
reset to its original disposition". osh instead sniffed the *first* operand and,
whenever it happened to name a signal, treated the whole call as a listing —
`let first_is_spec = rest.first().is_some_and(…); if print || rest.is_empty() ||
first_is_spec { return self.trap_print(…) }`.

So every one-operand reset silently became a print and the trap stayed set:

```sh
trap 'echo E' EXIT
trap EXIT          # bash: clears it.  osh: printed it, and E still fired.
```

That is the damaging shape — a script that disarms its own EXIT handler before
doing something the handler must not see. `trap USR1`, `trap 0` and
`trap -- INT` were wrong the same way.

The sniff was also wrong in the *other* direction for two operands, since
`rest[0]` naming a signal says nothing: `trap USR1 INT` traps INT with the
command `USR1`, and osh printed instead of trapping.

**The fix.** Decide on the operand count, as bash does. A lone operand that
names a signal is a reset (action `-`); otherwise the first operand is the
action and the rest are the signals. A lone operand that names *no* signal has
no reading left, so it falls through to the same usage error (status 2) an
action with no signals gets — which is what `trap -`, `trap ""`, `trap ":"` and
`trap BOGUS` all are. Covered by
`trap-with-a-lone-signal-resets-it-rather-than-showing-it.sh` and a lib test.
