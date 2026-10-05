### TD-OILS-PROCSUB-DEVFD. Process substitution as a *word* expands to a temp-file path, not `/dev/fd/N`, on the Windows dev host — 2026-07-19

**Where:** `userspace/oils/src/interp.rs` process-substitution expansion
(the `osh_psub_*.tmp` temp-file backing).

**What:** `echo <(echo hi)` prints `/dev/fd/63` on Linux/bash but
`C:/Users/…/Temp/osh_psub_<pid>_0.tmp` on osh's Windows dev host. The
*content* is correct — reading the path yields `hi` — only the path
*format* differs. This only manifests when a script inspects the
substitution's filename itself (rare); using it as an input file works.

**Why deferred:** `/dev/fd/N` requires a `/dev/fd` filesystem and real
fd-passing, which the Windows host lacks; osh backs process substitution
with temp files there. On the slateos target (which has proper fd
support) this should present as `/dev/fd/N` and match bash. This is a
host-platform artifact, not a shell-logic bug.

**Proper fix:** on slateos, back `<()`/`>()` with anonymous pipes exposed
via `/dev/fd/N` rather than temp files; the Windows dev host keeps the
temp-file fallback.
