## TD-B-A-LINUX-TARGETED-HARNESS-CANNOT-MEASURE-A-SLATEOS-GATED-SUBJECT (lane B, 2026-09-11)

**Every `*-diff.sh` here builds its subject for `x86_64-unknown-linux-gnu`, and
47 of the standalone `userspace/` crates gate their real syscalls on
`#[cfg(target_vendor = "slateos")]`.** Built for Linux those crates do not run
the operation at all — they return a refusal stub — so a differential measures
the stub and reports a landslide that says nothing about the program.

`chown` is where this surfaced. `scripts/chown-diff.sh` reported:

    coreutils   64 passed,  0 differed
    standalone   3 passed, 61 differed

and **43 of those 61 are the single sentence** `chown: cannot change ownership
of 'file.txt': chown syscall unavailable on this platform`, which is
`userspace/chown/src/main.rs:379` behind `#[cfg(not(target_vendor =
"slateos"))]`. On the SlateOS target that arm does not exist. So the pair is
**not decided** and the standalone has not been measured — only 18 of the 61 are
real differences (the `user.group` dot form, unknown-user diagnostics, five
`--from=` cases, the missing-operand messages, a `--ref=` abbreviation).

**This is the same category as `uname -o`: a comparison against the wrong
reference.** There the baseline was entitled to disagree; here the *subject* is
not the program that ships. Both manufacture defects in proportion to how
thorough the harness is, and neither is caught by any threshold.

**What was checked rather than assumed, because a wrong answer here would
invalidate work already done:**

  * **Of the pairs already retired, only `df` ever contained the gate**, and its
    recorded run contains **zero** platform-gate refusals — the standalone `df`
    ran, produced a real five-filesystem table with ANSI escapes, and failed on
    its own merits. That retirement stands.
  * `date`'s standalone gates only its clock-**setting** path, which
    `date-diff.sh` never exercises; its run has zero gate refusals too. That
    record stands.
  * `coreutils`' bins carry **no** such gate — they are portable, which is why
    `coreutils` can score 64/64 here at all.

**Affected among the pairs still open: `chown` (measured, mostly artifact) and
`kill` (predicted — its gate will be on exactly the signal-sending path a `kill`
harness would test).**

**What would actually decide these.** Either build the standalone for the
SlateOS target and run both halves under the boot test, or compare only the
surface that is not gated — argument parsing, diagnostics, exit statuses — and
say plainly that the behaviour was not measured. The second is cheap and is what
the 18 real `chown` cases already are; the first is the only thing that settles
the pair.
