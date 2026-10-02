## 78. `osh` advertises itself as bash via `$BASH_VERSION`/`$BASH_VERSINFO` (Q27) — **option A (advertise), made a per-user toggle defaulting on**, mirroring upstream Oils' `bash_compat`

**Date:** 2026-07-21
**Decided by:** Operator (Claude recommended A and proposed the toggle; operator
chose A and asked that it be a per-user option defaulting to A) — resolves
open-question **Q27**.

**Factual finding that informed the call (operator asked "what does the original
osh itself do?").** Upstream Oils' `osh` **does advertise as bash by default.**
`core/shell.py` sets `BASH_VERSION='5.3'` and `BASH_VERSINFO=(5 3 0 0 release
unknown)`, gated on a `bash_compat` flag that defaults **on for `osh`** and off
for `ysh` (Oils' honest non-bash language). So option A matches upstream `osh`
exactly, and making it a toggle mirrors upstream's own `bash_compat` switch.

**Decision.** `osh` sets both `BASH_VERSION` and `BASH_VERSINFO` by default
(option A), controlled by the per-user env toggle **`OSH_BASH_COMPAT`** (default
**on**; `0`/`off`/`false`/`no` disable it, whereupon both variables are left
unset so bash-detecting scripts see a non-bash shell — like `ysh`). We keep the
reported level at **`5.2.0(1)-release`** / `BASH_VERSINFO=(5 2 0 1 release
x86_64-slateos)` — deliberately **5.2, not upstream's 5.3** — because osh targets
bash-5.2 semantics and must never claim a 5.3-only feature it doesn't implement.

**Rationale.** *Pro:* the dominant real-world use of `$BASH_VERSION` is the "is
this bash? then run the bash branch" gate that a bash-superset shell *wants* to
satisfy; matches upstream `osh`; the toggle lets a user who wants honesty opt out.
*Con:* advertising bash is a deliberate half-truth — a script may then assume a
specific bash behaviour osh implements slightly differently; the toggle is
env-based (process-global) so it's a startup/login-time choice, not per-invocation.

**Operator's exact words.** "I lean A, but what does the original osh itself do?
… Perhaps Q27 should be a user option, too, that defaults to A?"

**Where it lives.** `userspace/oils/src/interp.rs` — `bash_compat_enabled()`
(reads `OSH_BASH_COMPAT`), gating the `BASH_VERSION`/`BASH_VERSINFO` seeds in
`seed_shell_vars`; `BASH_VERSION` const at interp.rs:108. Supersedes Q27 in
open-questions.md (now removed).
