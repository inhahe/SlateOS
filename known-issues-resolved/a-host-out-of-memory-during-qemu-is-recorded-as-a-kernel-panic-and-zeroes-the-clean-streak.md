## A host out-of-memory during QEMU is recorded as a kernel PANIC, and zeroes the clean streak

**Status:** FIXED 2026-09-01 (see the closing section at the end of this entry).
Observed once, 2026-09-01, on a `--bench` boot in the lane-A worktree. Nothing
in the tree was wrong; the boot history said otherwise. The misdiagnosis is
fixed; the host-side memory spike that caused it is not, and cannot be from
here.

**In short:** the machine running QEMU briefly ran out of mappable memory. QEMU
could not give the emulated GPU the memory it asked for, the kernel correctly
reported that as an I/O error, a self-test correctly called it fatal — and the
boot history wrote it down as "kernel died", which reset the consecutive
clean-boot streak from 9 to 0 and added a failure to the project's headline
health counts. Every step behaved correctly and the recorded conclusion is still
false.

**What actually happened, in order.** QEMU printed, on its own stderr:

```
C:\program files\qemu\qemu-system-x86_64.exe: warning: Failed to CreateFileMapping:
The paging file is too small for this operation to complete.
```

and a shell helper in the same harness died of the same pressure:

```
0 [main] date (85728) child_copy: cygheap read copy failed, 0x0..0x80000CD40,
done 0, windows pid 85728, Win32 error 299
```

(`299` is `ERROR_PARTIAL_COPY`.) The guest then saw its GPU refuse an
allocation:

```
[virtio-gpu] RESOURCE SELF-TEST FAILED: a resource exactly at the bound was
             rejected: I/O error (-600)
FATAL: virtio-gpu render-resource self-test failed: internal kernel error (-1)
```

and `boot-history.py` classified the run:

```
[boot-history] PANIC -- kernel died (PANIC / FATAL in the serial log)
[boot-history] current consecutive clean streak: 0
```

**Why the message is especially misleading.** The self-test that failed is a
*boundary* test — "a resource exactly at the bound was rejected". A host OOM
landing on that allocation produces a log line that reads exactly like a
genuine off-by-one in the bound check. Anyone reading the history later would
have every reason to bisect for a virtio-gpu regression and would find no
guilty commit, which is the same shape as the `http_build_response_1KiB`
mode-split that `bench-history.py:mode_structure` was written to prevent.

**It was not the tree, and it was not a leak.** Inspected minutes afterwards:
no `qemu-system-x86_64` processes were alive, 28.85 GiB of 63.95 GiB physical
was free, and the pagefile was 191191 MiB allocated against 20632 MiB in use
with a 22649 MiB peak — nowhere near exhausted. Windows grows the pagefile
lazily, so a fast allocation spike can outrun that growth and produce this exact
message on a machine with tens of gigabytes free. The likely source of the spike
is a concurrent `cargo build` in another lane's worktree: the boot lock
serialises QEMU across the three lanes, but nothing serialises their builds.

**The proper fix.** `boot-history.py` already has the right concept — a boot
whose outcome "is evidence about the probe, not about the tree" is tagged
`experiment` and kept out of every statistic (`is_experiment`, line 671). A
host-resource failure is the same category and needs the same treatment, but it
must not reuse the `experiment` flag: that flag means "invoked deliberately
under non-default conditions", and silently widening it to mean "or the host
hiccupped" would let a real failure be waved away by an unrelated predicate.

So: a distinct verdict — `HOST_FAIL` — set when the harness sees an
unambiguous host-side signature in QEMU's own output, excluded from the streak
and from the clean/total counts exactly as `experiment` rows are, and printed
with a reason so the row still says what happened. Candidate signatures, all
emitted by QEMU or Cygwin rather than by the guest, so a kernel bug cannot
forge them:

| signature | source |
|---|---|
| `Failed to CreateFileMapping` | QEMU, Windows memory backend |
| `The paging file is too small` | QEMU, quoting Windows |
| `cannot set up guest memory` | QEMU, generic allocation failure |
| `cygheap read copy failed` | Cygwin fork failure under pressure |

Two things to get right when implementing it. The detection must read QEMU's
**stderr**, not the guest's serial log — a guest that printed
`Failed to CreateFileMapping` must not be able to excuse itself. And the
existing `PANIC` classification must stay the default: `HOST_FAIL` is only
reachable on a positive match, because the direction that fails safely here is
the one that over-reports failures, exactly as `is_experiment`'s doc argues for
its own absent-means-no default.

**Until it is fixed,** a boot that dies this way has to be spotted by eye and
re-run, and the streak counter should be read as a lower bound. This run was
re-run and the entry will be updated if it recurs — one occurrence is not yet
evidence about frequency.

### FIXED 2026-09-01 — `HOST_FAIL` is implemented as specified above

`boot-test.sh` now redirects QEMU's stderr to `build/qemu-stderr.txt` (and still
echoes it to the console on the way out, so the redirect costs the operator
nothing it used to show them), passes it to the recorder as `--qemu-stderr`, and
deletes it alongside the serial log so a leftover can never excuse the *next*
boot. `boot-history.py` grew `HOST_FAIL_SIGNATURES` — the four literal
substrings tabulated above — plus `host_failure()`, `read_qemu_stderr()`, and a
`describes_tree()` predicate that is now the single filter behind the streak,
the clean/total counts, and both sets of medians.

Both constraints this entry named are met, and both are covered by tests.
Detection reads QEMU's stderr only: `test_a_kernel_cannot_forge_a_host_failure`
puts the signature in the *guest's* log and asserts the verdict stays `PANIC`.
And the default is unchanged:
`test_a_host_signature_overrides_a_verdict_that_blames_the_tree` classifies this
run's real serial log both ways — `PANIC` without the host's words, `HOST_FAIL`
with them — so the override is doing the work, rather than the sample having
been chosen to classify harmlessly.

Two things went beyond the specification, both in the safe direction. The
override runs *downward only*: a host signature never rewrites a verdict that
clears the tree, because destroying a real clean boot would be worse than the
bug being fixed. And `fingerprints_for` skips `HOST_FAIL` entirely — the
matchers key on the shape of the exception without consulting the verdict, and a
host OOM lands on whatever allocation the kernel happened to be making, so a
host-killed boot could otherwise be filed as a recurrence of a known issue that
did not recur.

Reasoning in full, including the four rejected alternatives, in
`design-decisions.md` §669. The underlying *cause* is not fixed and is not ours
to fix — the boot lock serialises the three lanes' QEMU runs, but nothing
serialises their `cargo` builds — so a recurrence remains possible. What changes
is that it will be labelled rather than blamed on the kernel.
