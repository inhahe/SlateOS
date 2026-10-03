## 743. `utimecmp` is a shared library module with one caller, and it does the on-disk probe rather than assuming a resolution

**Date:** 2026-09-01
**Decided by:** Claude (autonomous)
**Lane:** B

**In short:** `mv -u b/x a/x` between two *different* filesystems has to answer
"is the copy that is already there as new as the thing I would replace it
with?" — and the copy that is already there was rounded off when it was written,
because filesystems do not all record the time to the same fineness. A source
stamped 10.5 seconds, copied to a filesystem that stores whole seconds, arrives
as 10.0; compare 10.0 against 10.5 and the destination looks older *forever*, so
`mv -u` copies it again on every run and never settles. GNU corrects this by
working out how fine the destination's clock actually is and rounding the source
the same way before comparing. Two questions had to be answered to follow it:
where the correcting code lives, and whether it is allowed to write to the disk
to find the answer.

### Where it lives: `coreutils::utimecmp`, not a private function in `mv.rs`

`cp --update` is not implemented yet — it is explicitly refused by name, and
`cp.rs`'s `unimplemented_long_options_are_rejected_by_name` test pins that — so
today the module has exactly one caller, which is normally an argument for
keeping it private. The decision went the other way on three pieces of evidence.

* **The option is one option, not two that happen to be spelled alike.**
  Upstream's `-u` for `cp` and for `mv` is the same field of the same struct
  read by the same line of `copy.c`. Only the *flag* differs, and even that
  differs only because `mv` can rename: for `cp` the flag is
  `preserve_timestamps`, and for `mv` the same expression reduces to "the move
  crosses a filesystem". When `cp --update` lands it will call this, and a
  private copy in `mv.rs` would be moved at that point rather than reused.
* **Upstream itself made this call.** `utimecmp` is in gnulib's `lib/`, not in
  `src/mv.c`, and it is shared by `cp`, `mv` and `install`.
* **It is a self-contained calculation with a cache**, not a fragment of `mv`'s
  control flow. The per-`dev_t` resolution cache is the kind of state that wants
  to outlive one call and be shared by every caller in the process.

This is the opposite conclusion from the one reached for `copytree` a day
earlier, and deliberately so: `copytree` had one upstream caller
(`copy_internal`) and no option struct anyone else read, so making it shared
would have been inventing a boundary upstream does not have. Here the boundary
is upstream's. The test is *"does the code have more than one caller in the
design"*, not *"does it have more than one caller today"*.

### Whether it may write to the destination to find the resolution

gnulib's measurement is not passive. When the trailing zeros of the
destination's three timestamps do not settle the question, it **writes** a
deliberately awkward modification time onto the destination file, `stat`s it
back to see which digits survived, and writes the original time back. That is a
real side effect of a comparison, on a file the user did not ask to modify, and
it is visible to anything watching the filesystem.

**Kept, rather than assuming a resolution.** The alternatives were each worse in
a way that is not recoverable:

* **Assume nanoseconds.** Wrong on exactly the filesystems the flag exists for.
  `mv -u` onto FAT or onto a network filesystem would never converge, which is
  the original bug with extra steps.
* **Assume the worst (two seconds).** Rounds the source down by up to two
  seconds on *every* cross-device `mv -u`, including the overwhelmingly common
  case where both sides keep nanoseconds — turning a correctness fix into a
  correctness bug in the opposite direction, where a genuinely newer source is
  skipped.
* **Ask `pathconf(_PC_TIMESTAMP_RESOLUTION)`.** The honest answer, and gnulib
  prefers it where it exists — but it does not exist on Linux, which is what we
  target, so the `#ifdef` is dead code here and was left out.

Three things bound the cost. The probe runs at most **once per device per
process** — the result is cached by `st_dev`. It runs only after the quick exits
have failed, so the two stamps are already within two seconds of each other and
the trailing-zero deduction was inconclusive. And the original time is written
back **whenever it might not still be there**, including when the read-back
itself failed, since a failed read cannot prove the probe did not land.

What is given up by keeping it: a `mv -u` that is interrupted between the probe
write and the restore leaves the destination stamped `res / 9` nanoseconds late
and possibly on an odd second. That is a real if narrow hazard, and it is
upstream's; the alternative to accepting it is one of the three wrong answers
above.

### One deliberate divergence from upstream

gnulib builds its probe timestamp as a C `struct timespec` and lets `utimensat`
reject anything malformed. The port converts through `SystemTime`, whose
arithmetic can overflow, so `instant()` returns `Option` and a stamp that is not
representable makes the whole comparison answer `Age::Unknown` — which the
caller reads as "not known to be up to date", so the move goes ahead, the same
thing `mv` without `-u` would have done. The important half is that the refusal
happens **before** the write: substituting some other time onto a real file
would be worse than admitting the two cannot be compared.
