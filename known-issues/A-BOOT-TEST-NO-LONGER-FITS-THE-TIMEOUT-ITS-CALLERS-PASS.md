## A-BOOT-TEST-NO-LONGER-FITS-THE-TIMEOUT-ITS-CALLERS-PASS (lane A, 2026-09-04)

**Two warm measurements from 2026-09-10, which do NOT close this.** Recorded
because the figure above is the only one in the entry and a single datum
invites being read as the current state.

| run | gate phase (harness's own `Gates OK (Ns)`) | build | QEMU to BOOT_OK | total |
|---|---|---|---|---|
| `bccb3tmwl` @ `bf4e7d981` | **1161 s** | 44 s | 465 s | 1692 s |
| `bftu0rc0h` @ `fc1bea8ff` | **1244 s** | 52 s | 569 s | 1891 s |

Both passed `python scripts/run-timeout.py --poll 120 3600 ./scripts/boot-test.sh`
and finished with roughly half the budget unused, so the specific trap this
entry describes — killed at the moment QEMU starts, after everything passed —
did not happen in either.

**Why that is not evidence the entry is stale.** The 6178 s above was a COLD
run and these two are warm, and the largest single component of the cold
figure — `cfg(unix)` arms compile+lint, 2071 s — is precisely the part that
caches. Comparing a warm number against a cold one is the error of treating a
measurement as a property of the code rather than of the run that produced
it. Subtracting the cfg(unix) term still leaves 4107 s against this 1161 s,
which is a 3.5x gap I cannot attribute: six days of gate work landed in
between, and the cold run may have been competing with another lane's build,
which this tree has documented causing several-fold swings elsewhere.

**So what would close it:** one cold run — `rd /s /q target` first, no other
lane building — reporting its own `Gates OK (Ns)`. Until then the honest
summary is that the phase fits comfortably when warm, and nobody has measured
it cold since the day this was filed.

**In short:** The boot test's checking phase has grown to 103 minutes. A caller
that gives it the customary two hours now has the checks eat almost the whole
budget, so the run gets killed *after* every check passed and the kernel built
— at the moment QEMU is starting. The run reports failure, but nothing was
wrong with the code: the harness simply ran out of time before it could look.

Measured on `eb2003eb7`, `python scripts/run-timeout.py 7200 bash
scripts/boot-test.sh`:

| phase | seconds |
|---|---|
| gate + self-test phase (`=== Gates OK ===`) | **6178** |
| of which `cfg(unix)` arms compile+lint | 2071 |
| of which kernel clippy | 386 |
| kernel build + stage + lock | ~660 |
| QEMU booting when the outer timeout fired | ~360 of its own 2400 |
| **outer timeout** | **7200** |

So the gate phase alone is 86% of a 7200s budget, and the phase the test exists
to run — the boot — got 15% of the time it is allowed to take.

Why this is a trap rather than a slow test: the failure is indistinguishable at
a glance from a real one. `run-timeout.py` exits 124 and the last lines of the
log are QEMU starting, so it reads as "the boot hung". It did not; it never got
to finish. The verdict rule (`bench/boot-history.jsonl`, never the exit code)
protects against misreading it as a *pass*, but nothing protects against
misreading it as a **kernel fault**, which is the more expensive mistake — it
sends the next reader looking for a hang that does not exist.

**What to do until this is fixed:** pass at least `14400` to `run-timeout.py`
for a full boot test on this tree (gates ~6200 + build ~700 + QEMU up to its own
2400 = ~9300s, plus headroom). Do not lower QEMU's own `--timeout` to make the
outer number fit; that trades a harness timeout for a real one.

**The proper fix, not yet done.** Two candidates, and they are not exclusive:

1. **Let a caller skip a gate phase that has already passed on this exact
   tree.** `scripts/pre-boot.py` already runs the gate phase without the boot,
   so the split exists in one direction but not the other: there is no
   `--no-gates`, and `boot-test.sh`'s flag list is `--no-build`, `--no-stage`,
   `--bench`, `--usb-image`, `--no-rootfs` and friends — none of them skip
   checking. The obvious shape is to key it on `BT_SRC_DIGEST`: a run may skip
   the gates only if a previous run recorded them green *for the identical
   digest*, which makes the skip unforgeable rather than a flag that turns
   checking off. A plain `--no-gates` would be the wrong fix and should not be
   added: a way to skip the gates is a way to skip the gates.

2. **Make the cost visible before it is paid.** `boot-test.sh` knows the gate
   phase's historical duration; it could print the expected total against the
   time actually available and say plainly that the budget is short, instead of
   discovering it 6841 seconds in. A test that cannot reach its own subject
   should say so at the start, not at the end.

Filed rather than fixed because the tree was mid-re-run when it was found, and
because option 1 touches the gate/boot seam, which deserves its own change
rather than a rider on a rebooted test.
