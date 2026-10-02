## 79. `osh` identity vars `$EUID`/`$UID` (Q28) — **default root (`0`/`0`) [option A], made per-user configurable** via `OSH_UID`/`OSH_EUID`

**Date:** 2026-07-21
**Decided by:** Operator (Claude recommended A = root default; operator accepted
the recommendation and added that it be the *default* with a per-user override) —
resolves open-question **Q28** (the `$HOSTNAME` half was already resolved
autonomously 2026-07-20).

**Context.** bash always defines readonly-integer `$EUID`/`$UID`; scripts lean on
them constantly (the canonical `[ "$EUID" -ne 0 ]` root check). osh left them
unset, so those comparisons errored on an empty operand. SlateOS has no
`getuid`-equivalent wired into the host or target build yet, so osh can't read a
real credential — the *reported* identity is a policy choice.

**Decision.** Seed `$UID` and `$EUID` as **real readonly-integer shell vars**
(`declare -ir`, matching bash's own attributes), defaulting to **root
(`0`/`0`)** — the shell genuinely *is* the all-powerful system during pre-privilege
bring-up, so root-gated scripts should take their root path. The reported identity
is **per-user configurable** via the `OSH_UID` / `OSH_EUID` env toggles (resolved
once in the free fn `reported_identity`; `OSH_EUID` defaults to `OSH_UID` if
unset), so a login/session layer can inject a real per-user identity. Seeded
*before* `import_environment` so an inherited `UID=` in the environment neither
overrides nor becomes exported (matching bash: UID/EUID are non-exported shell
vars). The `\$` prompt escape now keys `#`-vs-`$` on `$EUID == 0`.

**Faithfulness note (why real vars, not the dynamic `PPID` model).** osh's other
readonly-integer specials (`PPID`/`BASHPID`) are computed dynamically in
`param_value` and are *not* actually readonly-enforced (`PPID=5` is silently
accepted) nor listed in bulk `declare -i`/`declare -p`. EUID/UID instead use real
`self.vars` entries + `self.readonly` + `self.integer_attr`, which makes them
correctly readonly-enforced and correctly present in `declare -i`/`declare -p`/
`set`/`${!prefix*}` listings — strictly more bash-faithful. The remaining
dynamic specials are logged in known-issues (TD-OILS-IDVARS) as a latent
inconsistency to migrate later.

**Rationale.** *Pro:* fixes the extremely common `$EUID`/`$UID` idioms; root
default matches current single-user bring-up reality; the per-user override models
the eventual "shell runs in a user session" norm and lets a future login layer set
a real identity without code change. *Con:* a "are you root? then it's safe" script
proceeds where a real multi-user system wouldn't — masks the absent privilege
model; the override is env-based (process-global), a startup/login choice.

**Operator's exact words.** "I'll go with your recommendation, though I suggest
making that the default and actually giving the user the option of what to report,
per user."

**Where it lives.** `userspace/oils/src/interp.rs` — `reported_identity()` (reads
`OSH_UID`/`OSH_EUID`), the UID/EUID seed in `seed_shell_vars`, the `\$` prompt
escape (~4658). Test: `special_var_identity_uid_euid`. Supersedes Q28 in
open-questions.md (now removed); known-issues TD-OILS-IDVARS updated.
