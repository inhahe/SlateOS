## 633. An unrecognised word in a firewall rule is a hard error, even though that rejects command lines the shell used to accept

**Date:** 2026-08-29
**Decided by:** Claude (autonomous, lane A)
**Lane:** A

**In short:** `fw allow in tcp port 80` adds a firewall rule. Until now, if you
misspelled one of the optional words — typing `prot` instead of `port` — the
shell ignored the word it did not recognise, ignored the `80` that followed it,
added the rule anyway, and said it had succeeded. Because a firewall rule with
no port named applies to *every* port, that typo did not produce a slightly
wrong rule; it produced a rule that let everything through. The choice was
whether to keep accepting such lines (with a warning) or to reject them
outright. I chose to reject, which means a script that has been passing an
extra word will now fail where it used to appear to work.

**The specific defect.** The modifier loop ended `_ => { i += 1; }`. Advancing
by one rather than two meant the *operand* of the unrecognised keyword was then
re-read as if it were itself a keyword, and discarded in turn — so a single
misspelling consumed two words silently. The rule was then built from the
resting values of `dst_port` (`0`, documented as "any port" at
`net/firewall.rs:146`) and `src_ip` (`0.0.0.0`, "any"), and `firewall::add_rule`
accepted it, because there is nothing about it to reject.

| Option | *What changes* |
|---|---|
| **A. Keep accepting, print a warning** | `fw allow in tcp prot 80` still adds a rule and still exits 0, but a line of complaint appears above the success line. |
| **B (chosen). Refuse the whole command** | `fw allow in tcp prot 80` adds nothing, prints `fw: allow: unknown option \`prot'; expected port, ip or prio`, and exits 1. |
| **C. Refuse only when the rule would end up unrestricted** | the same line is accepted or rejected depending on which *other* words were present, so the same typo behaves differently in two scripts. |

**Why B.** A warning is only useful to someone reading the output, and the
distinguishing feature of this command is that its output is usually not read —
firewall rules are installed by startup scripts. Option A's warning would scroll
past in a boot log while the firewall silently stood open. C was rejected on the
principle that a diagnostic must not depend on unrelated arguments: a rule that
is refused in one command line and accepted in another, for the same misspelt
word, teaches the reader nothing and cannot be scripted against.

**The accepted cost, stated plainly.** This is a backward-incompatible change to
a user-visible interface. Any existing caller that passes a word this parser
does not know now fails instead of installing a rule. I judged that acceptable
because the previous behaviour did not do what such a caller believed it was
doing — the "working" script was installing an unrestricted rule — so there is
no correct existing use to preserve. If a caller *wants* an any-port rule, the
way to say so is to omit the `port` keyword, which remains supported and is now
the only way to express it.

**A related, smaller call recorded here rather than separately:** `port` with
nothing after it is refused rather than treated as "any port". Same reasoning —
a truncated command line is more likely than a deliberate no-op keyword — and
the same escape hatch: omit the keyword.

**Why the IPv4 and IPv6 arms were fixed by duplication, not factored into one
helper.** They are hand-copied twins and carried all four defects each. Merging
them was considered and rejected: they differ in address type
(`Ipv4Addr`/`Ipv6Addr`), in prefix ceiling (32/128) and in result struct
(`Rule`/`Rule6`), and the genuinely shared part is three keyword names and a
refusal apiece — smaller than the generic wrapper needed to carry the
differences. The duplication is real and is the reason this defect existed
twice; it is recorded here so the next person to touch either arm knows to check
the other. What actually caught the second copy was `check-option-refusal.py`
reporting `2 fewer than expected` after the first was fixed, which is the ledger
working as designed.

**How to reverse:** in both `case` functions in `cmd_firewall`, replace the
`other =>` arm's `shell_println!`/`set_exit(1)`/`return` with `i += 1` to
restore the old behaviour, or with `i += 2` to skip the operand as well (which
would be the least-bad version of option A).

**Where it is:** the two `case` functions under `"allow" | "deny"` and
`"allow6" | "deny6"` in `cmd_firewall`, `kernel/src/kshell.rs`; pinned by
self-test rung 101, which asserts both that the misspelt word is named and that
no rule is reported added.
