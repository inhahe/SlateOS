## 1565. `capsettings` becomes a view of the enforced groups and tags, and `secpolicy` is parked

**Date:** 2026-10-09 · **Decided by:** Operator (Claude recommended this option) · **Lane:** A

Answering A-Q23, option A. The operator's answer, verbatim, is in
`operator-answers/2026-10-09-open-questions-answers.txt` ("A-Q23: A").

**In short:** two unused "who may do what" modules each carried a whole rule
system of their own, overlapping the rules the kernel already enforces. Rather
than switch both on -- which would have given the system three separate sets
of permission rules, each able to refuse what another allows -- the settings
module (`capsettings`) becomes a way to show and edit the rules that are
already enforced (the capability groups and the file tags), and the second one
(`secpolicy`, a mandatory-access-control framework: administrator-written rules
keyed by labels on programs and files) is left unconnected, kept as the start
of a design for later.

**The alternatives not taken:** connecting both as written (three sources of
truth for "may I", two of them keyed by text patterns a rename slips past);
removing both (loses `secpolicy`'s design).

**What it obliges.**
- `kernel/src/fs/capsettings.rs`: its groups are `cap::groups`' groups, its
  path requirements `cap::file_tags`' tags, and its per-program grants a
  process's capabilities -- one table each, which it reads and edits rather
  than keeping copies. Its per-user permissions with no enforced counterpart
  ("Network", "Reboot" and the like) are not shown as if enforced; each waits
  for a capability the kernel checks, or is removed.
- `kernel/src/fs/secpolicy.rs` stays unconnected, its documentation saying so;
  A-Q21's list (§978) loses it, and a mandatory-access-control system becomes
  a deferred question (`deferred-questions/`), to be asked when something
  wants one: what labels, who writes the policy, what the default is.
- The file tags, which `capsettings` will edit, are kept in memory and keyed
  by path today (`cap::file_tags`); making them the enforced, persistent rule
  that a settings page can stand on is part of this work.
