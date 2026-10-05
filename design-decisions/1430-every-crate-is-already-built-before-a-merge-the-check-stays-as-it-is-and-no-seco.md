## 1430. Every crate is already built before a merge; the check stays as it is and no second one is added (C-Q11)

**Date:** 2026-09-27 &middot; **Decided by:** Claude (operator-approved scope: the
operator left C-Q11 to Claude, asking that the check's cost be measured under the
machine's normal load and set against the time it has saved) &middot; **Lane:** C

**In short:** C-Q11 asked whether something should build every crate before work
is merged, after a shared-library change broke the lock screen and nothing
noticed for a day. Something now does: every boot test -- which is required
before anything reaches `main` -- compiles and lints the whole workspace for the
Linux target (`clippy --workspace --exclude kernel --all-targets`), and a push
also compiles the whole workspace for the host. It costs about two and a half
percent of a boot test and has caught real breaks since. So the answer is
option A, already in force; nothing is added.

**Measured**, from lane C's boot logs, with other lanes running:

| Run | The whole-workspace clippy | The whole boot test | Share |
|---|---|---|---|
| lane C, release boot | 181 s | 7,876 s (gates 6,228 s) | 2.3% |
| lane C, debug boot | 90 s | 3,795 s (gates 2,903 s) | 2.4% |

Earlier figures (15-49 s for `cargo check --workspace`, 2026-09-06) were
taken with fewer lanes and projects running; the operator asked for the
normal-load number, which this is. The cost is dominated by the gate phase's
other checks, not by this one.

**What it has saved.** It refuses a push or a boot the moment a crate anywhere
stops compiling or linting, which is exactly the lock screen's failure. Commits
that exist because it refused, among others: `3dd7a7e64` (lane C -- a toolkit
type grew and broke `apps/finance`'s lint, a crate lane C never builds),
`dc02d5189` (a record type's new fields broke literals elsewhere),
`5bcc49e7d` and `8d35e6a88` (Linux-only lints in code the Windows host never
compiles). Each of those would otherwise have reached `main` and cost every
other lane a red boot -- about two and a half hours each -- to find.

**Options, as C-Q11 put them:**

| Option | Verdict |
|---|---|
| A. A whole-workspace check before a merge | **in force**, through the boot test and the push |
| B. A nightly sweep that only reports | not needed: A catches the same breaks before they land rather than after |
| C. "grep before claiming no caller changes" | still good practice; no longer the only defence |
| D. Nothing | not the state of the tree |
