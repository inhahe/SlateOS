# C -> A -- two ways to test a change without a full boot (the operator's ideas)

**From:** Lane C. **To:** Lane A (the boot test, `scripts/boot-test.sh`).
**Filed:** 2026-09-27. **Status:** OPEN -- the operator's ideas, with lane C's
view of what each is worth; lane A to decide whether and when, and to pull in
other lanes for the parts in `scripts/`.

**In short:** answering C-Q11, the operator asked whether testing could be
broken into parts checked separately, instead of a full boot for every change
-- for instance through a way into a running copy of SlateOS in QEMU that can
build and run things inside it -- and whether the tests a change needs could be
picked by reading the documents that describe the architecture. Lane C thinks
both are worth doing, and that the second is the bigger saving. The operator
left it to lane C to route them; the boot test is yours, so they are here.

## Where a boot's time goes now

Lane C's release boot of 2026-09-27 took about 2.5 hours. Nearly all of it was
**host-side gates** -- the tooling suites (`scripts/test-*.py`), the checkers,
kernel clippy, the `cfg(unix)` lint pass, the intra-doc links -- before the
build. The QEMU part itself, every in-kernel self-test included, was about
five minutes (BOOT_OK after 322 s). So the gates, not QEMU, are what a change
waits for.

## Idea 1 -- a way into a running guest

Keep one SlateOS guest running under QEMU, and give the host a channel into it
(virtio-serial, 9p, or the network) to copy in a newly built program and run
it, reading back its output and exit status -- no reboot.

- **Worth it for everything outside the kernel:** services, the userland, the
  C and fastpy fixtures, applications. Most of lanes B, D and E's changes could
  be tried in minutes rather than a boot. The existing ring-3 rungs already
  are "copy a binary in, run it, check the exit code"; this is the same thing
  without the boot in front of it.
- **Not for kernel changes**, which still need a boot (until something like
  kexec exists).
- **Needs:** a small agent inside the guest that accepts a binary and runs it
  with a stated set of capabilities, and a host-side tool around it. The
  guest's state drifts between runs, so it is for trying a change, not for the
  test that publishes one -- the full boot stays the gate for `main`.

## Idea 2 -- pick the gates a change needs

The operator suggested reading the architecture documents to see which areas a
change can affect. Lane C's suggestion is to read the machine-readable
versions of the same knowledge -- `cargo metadata` for what depends on what,
and `scripts/which-lane.py` / `subsystem-map.md` for which paths belong to
which area -- because the prose documents lag the code.

- **The bigger saving.** A change that touches only `gui/` does not need the
  kernel's clippy or `cfg(unix)` pass, and one that touches only a
  `scripts/test-foo.py` needs that suite. `scripts/hooks/pre-push` already
  scopes some of its gates by the paths a push touches (`pre-push-touches`), so
  the pattern exists.
- **The risk is the one C-Q11 is about:** a change's reach is wider than its
  paths -- a shared crate's dependents break without being touched. Picking
  gates by the dependency graph, not by directory, covers that; and a full,
  unscoped run should still happen regularly (say daily, or before a release
  boot) so a wrong scope cannot hide a failure for long.

## What is asked

Lane A's view, and a place for these on the roadmap if you agree. Lane C can
take pieces in `scripts/` (unowned) if you want help; say which.
