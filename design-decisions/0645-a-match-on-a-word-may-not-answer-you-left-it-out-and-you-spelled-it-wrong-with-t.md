## 645. A `match` on a word may not answer "you left it out" and "you spelled it wrong" with the same arm

**Date:** 2026-08-30
**Lane:** A
**Decided by:** Claude (autonomous), lane A

**In short:** Lots of shell commands take a word rather than a number —
`wpa2`, `keepfiles`, `deny`. The code that reads those words was written as a
list of the words it knows, ending in a catch-all "anything else" case that
quietly picked a default. That meant typing nothing and typing the word wrong
produced exactly the same result, with no complaint: `progmgr uninstall myapp
keepfilez` (one letter off `keepfiles`) deleted the user's files, and
`wifiscan discover … wep2 …` recorded an unprotected Wi-Fi network as
password-protected. The decision is that the default now has to be written out
as its own named case, and "anything else" has to refuse and list the words
that would have worked. The cost is that a few commands that used to accept a
sloppy word now stop and complain.

This is the same defect as §600's guessed value — a value invented for a word
the shell could not read — but wearing a `match` instead of a
`.parse().unwrap_or(…)`. It is called out separately because **the gate cannot
see it.** `scripts/check-option-refusal.py` finds guessed values by looking for
`.parse()` and `from_str_radix`; a `match` on a `&str` contains neither, so
seven of these sat unlisted in `scripts/option-refusal-ledger.txt` while the
ledger's total looked healthy. They were found by reading the code around
sites the gate *did* flag.

### The shape

```rust
// before
let opt = match parts.get(2).copied().unwrap_or("full") {
    "keepfiles" => UninstallOption::KeepFiles,
    // …
    _ => UninstallOption::Full,          // <- absent AND misspelled land here
};

// after
let opt = match parts.get(2).copied().unwrap_or("full") {
    "full" => UninstallOption::Full,     // <- absent: the default, named
    "keepfiles" => UninstallOption::KeepFiles,
    // …
    other => {                           // <- misspelled: refused
        refuse_operand("progmgr", "uninstall", format_args!(
            "`{other}' is not an uninstall option (full, keepfiles, keepsettings, keepall)"));
        return;
    }
};
```

The `unwrap_or("full")` stays. That is the *point*: absence still has a
default, and the default is now visible as a word rather than hidden in the
catch-all. A reader asking "what happens if I leave this out?" gets an answer
that no longer silently also covers "what happens if I get it wrong?".

### Why not simply pick a safer default

The tempting repair for the security cases is to leave the catch-all and make
it fail safe — `_ => Effect::Deny` for the policy engine, `_ =>
SecurityType::Open` for the Wi-Fi scanner — on the grounds that a wrong guess
in the cautious direction is harmless. It is not, and this is written into
`parse_policy_effect`'s doc comment so it is not re-proposed:

* It still acts on a word nobody typed. The operator wrote `allo`; the machine
  did something, and did not say what.
* A policy engine that quietly denies is a machine that mysteriously stops
  working — which is *harder* to diagnose than one that quietly allows,
  because the failure surfaces somewhere else entirely, hours later, as a
  program that no longer opens a file.
* It would make the two directions of the same typo behave differently
  (`deny` misspelled → Deny, `allow` misspelled → Deny), so the operator
  cannot form a rule about what the shell does with a word it cannot read.

Refusing is the only answer that is the same in every direction.

### Alternatives considered

| | *What changes* | |
|---|---|---|
| **(a) Refuse, default named as its own arm** — chosen | a misspelled word stops the command and lists the valid words; a missing word behaves exactly as before | The only option under which absence and error are distinguishable. Costs one arm per parser. |
| **(b) Keep the catch-all, warn on stderr, proceed** | the command still runs, with a line of explanation above the result | Rejected: §600's whole finding is that the *result* is what gets believed. A warning above a receipt for the wrong action is a receipt for the wrong action. |
| **(c) Keep the catch-all, choose a safe default** | a misspelled word does the cautious thing silently | Rejected for the three reasons above. |
| **(d) Teach the gate to find `_ => Expr` arms** | the ledger grows by an unknown number of entries; nothing else changes today | Rejected as a *substitute*, worth doing as an *addition* — see below. |

### Why the gate was not extended to catch these

It was considered and deliberately deferred rather than dismissed. A detector
for "`match` on a `&str` whose last arm is `_ => <expr>`" would flag a large
number of arms that are entirely correct — dispatch tables, label functions,
`match` on an enum's own string form — and a gate whose output is mostly false
positives is a gate people learn to skim. Doing it properly means deciding
which `match`es are reading *operator input*, which is the same call-graph
question `scripts/check-selftest-wording.py` answers for a different purpose;
that is a real piece of work and not a line of regex.

The honest interim position is recorded where it matters: the ledger header
already says its number is a floor rather than a measurement, and this is now
the third independent reason it is one — and the first that is not a bug in
the detector but a limit of the syntax it keys on. Written up in
`known-issues.md` under the burn-down log for batch 43a.

### How to reverse it

Restore the `_ => <default>` arms and delete the `other =>` blocks; the
`unwrap_or("<default>")` on each site is already correct and does not change.
kshell rung 110's `wifiscan`, `progmgr` and `policyengine` blocks go with them.
Note that reversing this reinstates a **silent** behaviour, not a noisy one:
there will be nothing in the output to indicate the shell is guessing again,
which is why the rung exists.
