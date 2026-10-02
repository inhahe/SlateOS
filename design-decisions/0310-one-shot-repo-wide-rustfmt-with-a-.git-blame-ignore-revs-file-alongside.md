## §310 — One-shot repo-wide rustfmt, with a `.git-blame-ignore-revs` file alongside

**Date:** 2026-08-15
**Decided by:** Operator (Claude recommended this option — open-questions.md Q42 option A)

**The question.** `CLAUDE.md` sets the convention as "rustfmt defaults, no manual
formatting overrides", but two crates are not rustfmt-clean: `kernel` (16 911
hunks) and `posix` (389 hunks across 244 of 2 299 files). Because `cargo fmt` is
package-scoped with no file filter, formatting your own change in a drifted
crate rewrites hundreds of files you never touched — one ~150-line edit in
`posix` produced a 1 403-insertion / 1 429-deletion diff across 173 files that
could not afterwards be separated from the real change, costing a
revert-and-redo.

**Decision: option A — reformat the whole repo once, and commit a
`.git-blame-ignore-revs` file naming the reformat commits.** Afterwards
`cargo fmt` is safe, the stated convention becomes true rather than
aspirational, and any future drift is a real diff.

**The cost, stated honestly.** This rewrites `git blame` for ~17 000 hunks of
kernel code. Blame is the primary tool for "why is this line here?" in a
codebase with no human reviewer and a 4 600-commit history, and this is the one
part that cannot be undone. `.git-blame-ignore-revs` mitigates it for anyone who
configures it (`git config blame.ignoreRevsFile .git-blame-ignore-revs`) and for
`git blame --ignore-rev`, but **not** for GitHub's plain blame view or a casual
`git log -S`. The operator accepted that trade: the trap is permanent and recurs
on every edit, the blame churn is one-time.

**Execution constraints that shape how it lands.**

- `cargo fmt --all` does **not** run in this workspace — it dies with
  `The filename or extension is too long. (os error 206)`, the Windows
  command-line limit, hit by the sheer number of workspace members. The
  reformat must iterate crates one at a time.
- It is **two commits in two lanes**, not one. `posix/` is Lane B's and
  `kernel/` is Lane A's; a single cross-lane reformat commit would be exactly
  the clobbering the lane split exists to prevent. Each lane reformats its own
  crate, and both commit hashes go into `.git-blame-ignore-revs`.
- Each reformat commit must contain **nothing but** formatting, so that
  `--ignore-rev` is safe to apply wholesale.

**Rejected alternatives.** **B** (format only files you edited, via
`rustfmt --edition 2024 <file>`) was the working stopgap and has zero history
churn, but leaves the trap armed for anyone reaching for the obvious command and
never shrinks the drift. **C** (reformat `posix` only) clears the crate under
daily work for 1.5% of A's blame cost, but leaves the worst offender armed — and
a half-applied convention is the state that caused the incident.

**Where it bites:** every `.rs` file in `kernel/` and `posix/`, the new
`.git-blame-ignore-revs`, `CLAUDE.md`'s formatting convention (now true), and
`known-issues.md` → `TD-REPO-IS-NOT-RUSTFMT-CLEAN-SO-RUNNING-CARGO-FMT-IS-A-TRAP`.
