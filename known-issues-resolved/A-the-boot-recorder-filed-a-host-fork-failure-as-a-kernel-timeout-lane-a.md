## The boot recorder filed a host fork failure as a kernel TIMEOUT (lane A) — FIXED

**Status:** fixed in `a85a5b5b4`; the one corrupted row is corrected in place.

**In short:** on 2026-09-02 a boot ran for 395 of its allowed 4500 seconds and
was in the middle of a userspace test when the *host* refused to fork a
process — Windows was at 96% of its commit limit, so `bash` could not start the
next helper and `boot-test.sh` exited 127. `boot-history.py` recorded that as a
`TIMEOUT`: a claim that the kernel failed to reach its marker. It reset the
consecutive-clean streak that four entries in this file use as their closure
bar, and it matched the run to lane B's `B-FORKEXEC-BOOT-HANG` — a *hang*
fingerprint, filed against a boot whose own stored tail shows it still making
progress at the moment the harness died.

**Why the existing guard missed it.** There was already a `HOST_FAIL` verdict
for exactly this class of event, and it is built on a good principle: it reads
QEMU's stderr and nothing else, because the guest cannot write to that stream,
so a kernel that prints `Failed to CreateFileMapping` cannot excuse itself. But
the fork that failed here was not QEMU's. QEMU was running fine; the harness
around it was what could not allocate. There was no host evidence to read,
because the process that would have produced it was never started.

**The fix.** The harness's own exit status has the same unforgeable property
from the other side: it is set by the shell, above the emulator, after the
kernel has had its say. `boot-test.sh`'s vocabulary is 0 (pass), 1 (kernel or
self-test failure), 2 (wedge), 3 (booted but produced no artefact) and 4 (boot
lock held). 126 and 127 are not in it — they are the shell's own statuses for
"could not execute" and "could not run at all", which is what a refused `fork()`
looks like from outside. `classify()` now treats those as a host failure.

**The trap that the first draft fell into, and the boot that caught it.** The
obvious rule — "any 126/127 means the host failed" — is wrong, and the history
already contained the proof. The boot of 2026-09-01T11:06 exited 127 **and**
its log ends `FATAL: virtio-gpu render-resource self-test failed`. Both things
happened: the kernel died, and then the harness stumbled on its way out.
Excusing every 127 would have rewritten that row to `HOST_FAIL` and removed a
real FATAL from the counts — the precise direction `boot-history.py` exists to
prevent, since a manufactured clean streak is what closes issues.

So the override is bounded by *what the verdict rests on*:

| Verdict | Rests on | Retracted by exit 127? |
|---|---|---|
| `PANIC` | the kernel's own words in the log | **No** — a harness that stumbled afterwards cannot un-say them |
| `TIMEOUT` | a marker that never arrived | Yes — absence is what a stopped harness cannot testify to |
| `SELFTEST_FAIL` | a gate that reported a failure | Yes — under 127 the gate never reported at all |
| `PASS` / `PASS_TOOLING` / `BENCH_INCOMPLETE` | the marker being reached | Unreachable: each needs an exit status the harness *does* produce |

`124` stays outside the rule even though it is also outside the vocabulary: it
is `run-timeout.py`'s expiry, which genuinely *is* a statement about something
not finishing, and excusing it would delete the loudest hang signal the harness
has.

The stderr half keeps its wider reach and still overrides `PANIC` — "cannot set
up guest memory" is evidence about *why* the guest died, whereas a fork that
failed minutes later is evidence about nothing but the fork.

**Where it lives:** `scripts/boot-history.py` — `HARNESS_ABORT_EXITS`,
`harness_abort()`, `LOG_EVIDENCED_VERDICTS`, `not_about_the_tree()`, and the
restructured `classify()`. Tests in `scripts/test-boot-history.py`, including
the 2026-09-01 counter-example as a regression case.

**The corrupted row:** corrected in place, mechanically, by applying the new
rule to fields the row already stored — verdict to `HOST_FAIL`, the false
`B-FORKEXEC-BOOT-HANG` fingerprint dropped, and a `corrected` field recording
what changed and why. The 2026-09-01 `PANIC` was left untouched, which is the
check that the new rule is narrow rather than convenient.
