## B-A-PROGRAM-THAT-REPORTS-A-FAILURE-AND-EXITS-ZERO (lane B, 2026-09-13) -- closed for lane B, 1 filed to C

**The most deceptive shape in this family, and the reason is structural.** A
human reading the terminal is told the truth; a script reading `$?` is told the
run succeeded. Only the one nobody watches is believed. And because the
*message* is right, a grep for it marks the program as already fixed -- which
is why three of these survived a day of work on a closely related class and
were each found by accident.

Found by `scripts/stderr-exit-zero-sweep.py`: every binary run in an empty
directory with a path that cannot exist, flagged if it writes a failure-shaped
line to stderr and exits 0.

### The rule is not new; the instrument is

`scripts/check-usage-status.py` already guards it -- and its own history is the
argument for a second one. That checker exists because the defect was fixed
three times, and the sweep that fixed 710 sites reported itself complete while
87 remained, because those 87 left their match arm by falling off the end
instead of through the `return` the search looked for.

That checker is **static** and scoped to **kshell**. This one is
**behavioural** and scoped to **userspace binaries**. Same rule, different
population, different instrument -- and the three that turned up today were
in neither the population nor the shape the other one reads.

### First run: 10 candidates, 5 real

| program | said | did |
|---|---|---|
| `swapon` | `failed to activate: ...` | exit 0 |
| `column` | `...: No such file or directory` | exit 0 |
| `ctags` | `cannot open '...'` | exit 0 |
| `sanitize` | `error: ... does not exist` | exit 0 |
| `more` | `cannot open ...: No such file` | exit 0 |
| `credentials` | `Failed to set master password: the RNG is unavailable` | exit 0 -- **lane C's**, filed |

`credentials` is a *different* defect from the unknown-option one lane C
closed, and the two got tangled: their fix made my reproduction stop working
without touching the bug. `credentials zzq-no-such-file` now exits 2 on the
operand, correctly, before ever reaching the master-password call. **The probe
went blind and the defect did not move** -- it is still there on a bare
invocation. The sweep now runs two probes, one with a missing path and one
with no arguments at all, because a failure path that needs no operand was
invisible to the first.

`sanitize` is the one to reread. It already counted the failure
(`stats.errors += 1`) and already **printed** the count -- "Errors: 1" -- and
then threw it away. Nothing was missing but the last step, so no amount of
reading the error path would have found it: the count was right, the message
was right, only the status was wrong.

Every fix keeps working past the failure on purpose -- `column a missing b`
still formats `a` and `b` -- so the worst result is carried to the end rather
than returned from the middle.

### Three false positives, and getting them right mattered more than the fixes

A detector that cries wolf is argued with once and ignored afterwards.

| program | why exit 0 is correct |
|---|---|
| `ed` | POSIX: a missing file is a new buffer. Exempted on **evidence** -- `ed-diff.sh` runs 507 cases against GNU ed 1.20.1 comparing stdout, stderr, status *and bytes on disk* -- not on a reading of the standard. |
| `hwinfo` | "cannot read /proc/cpuinfo; CPU details unavailable" on a host with no /proc. The inventory is still produced and still honest. |
| `ftp` | **Probe mismatch** -- a claim about the instrument, not the program. Every binary is handed a *filename*; ftp's operand is a *hostname*, so this measured what ftp does with an unresolvable host. The upstream status is unsettled: the reference ftp on this host hung on the lookup rather than answering, which is recorded instead of a guess. |

### I made the heuristic worse before making it better

To drop `hwinfo` and `selinux` I added an override for lines announcing a
fallback, and put `unavailable` and `not available` in it. That silently
suppressed `credentials: "Failed to set master password: the system random
number generator is unavailable"` -- a real failure -- because those words
usually name the **reason** something failed rather than a substitute used
instead.

**Trading a false positive for a false negative is the worse swap: a noisy
detector gets argued with, a quiet one gets believed.** The rule now requires
an explicit substitution (`defaulting to`, `falling back`, `using instead`),
`selinux` drops out by it, `hwinfo` is exempted by name, and both cases are
pinned in the self-test.

### What a clean run does not mean

A program that fails **silently** writes nothing and is invisible here. This
finds programs that say something and then contradict it. The docstring says
so too, because that is the sentence a passing gate invites a reader to skip.

### Not wired as a gate

One finding is still open (`credentials`, lane C's). A gate that fails on
arrival teaches people to bypass it; it can be wired when that closes.
