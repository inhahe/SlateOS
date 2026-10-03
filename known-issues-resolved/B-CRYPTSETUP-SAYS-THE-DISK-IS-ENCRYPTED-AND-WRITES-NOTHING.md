## B-CRYPTSETUP-SAYS-THE-DISK-IS-ENCRYPTED-AND-WRITES-NOTHING (lane B, 2026-09-10) — RESOLVED by deletion, same day

**In short:** `cryptsetup luksFormat /dev/sda1` prints
`LUKS2 formatted successfully on /dev/sda1.` and exits 0. It has not written
anything to the device. A user who runs it believes their disk is encrypted;
their data is in plaintext.

### The evidence

`userspace/cryptsetup` contains **no I/O of any kind** — no `std::fs`, no
`std::net`, no `Command`, no `libc`, no `unsafe`, no syscall — across 251
output calls. `cmd_luks_format` builds a `LuksHeader` from the command-line
options, calls `header.serialize()`, prints the size and the UUID, and then
prints the success line. `serialized` is never written anywhere.

It also prints the real warning first:

    WARNING!
    ========
    This will overwrite data on /dev/sda1 irrevocably.

so the output is indistinguishable from a real run, including the part that
tells the user to be careful.

`cmd_luks_open` is the same shape: it prompts `Enter passphrase for <device>:`
and then computes `pbkdf2_sha256(b"passphrase", b"salt", 1000, 32)` — a
hard-coded passphrase and a hard-coded salt — under a comment reading
`// Simulate key derivation`.

### Why nothing caught it

`scripts/audit-cli-fabrication.py` did not flag it, and the reason turned out
to be sharper than first written here. The wording is one half: `FACT_PATTERNS`
wants a measurement, a three-digit count, a `PASS`/`OK`/`found`, or a unit like
MB or Hz. But *"successfully"* is in fact one of the words it matches -- so
wording alone does not explain the miss.

The real cause is the **output macro**. Every pattern is anchored on
`println!("`, and `cryptsetup` contains no `println!("` anywhere: all 251 of
its output calls go through `writeln!(out, ...)`, this one being

    writeln!(out, "LUKS{} formatted successfully on {}.", header.version, device)

so rule 1 could not have seen it whatever it said. `hdparm` was invisible the
same way through `print_out(b"...")`. The same blind spot hid
`userspace/bridge`, deleted the same day.

Its own test suite asserted the fabrication:
`assert!(out.contains("formatted successfully"))` -- a test that passes
precisely because the message is printed without the write happening.

It is one of **225** userspace crates with no I/O marker anywhere that the
audit does not flag. `mdadm`, `nmcli`, `dmsetup`, `nft` and `systemd-resolved`
are in the same set and have not been examined individually.

### What the fix is

Under `design-decisions.md` 1006 this is deleted, not stubbed: it performs no
I/O and states a fact, which is the definition, and a refusing stub is what
the operator rejected. That has not been done yet because the whole 225 wants
deriving mechanically rather than picking by hand — the audit needs a second
rule for the no-I/O-at-all shape, and that rule must exclude library crates
(`charwidth`, `bignum`, `ere`, `modechange` are in the set and are not
commands).

### What actually happened

The rule was added and the set derived the same day. `cryptsetup` and 219
other commands were deleted in one change: **220 crates, 666 files, 61,100
lines** -- every one of which builds a binary, is not a pure-argv tool, and
contains no call that could look at anything outside its own arguments.
(36,639 of those lines are non-test source; the rest are the tests and
manifests that came with them. The smaller figure is what the audit reads,
not what was removed, and the two are worth keeping apart.)

The set was checked before it was used, because the first 1006 deletion nearly
took two working programs (`cal` and `earlyoom`) for want of two markers:

* **Dependencies.** Across all 220 there are exactly two declared dependencies,
  `quoting` (20 crates) and `sha2` (1). Neither reads the world, so no crate
  in the set reaches it through another.
* **A wider net than `IO_MARKERS`.** Zero hits for `env::var` unprefixed,
  `current_dir`/`current_exe`, `asm!`, `include_str!`, or a `::`-path into any
  workspace crate. The only `extern "C"` occurrences -- 9 of them -- are
  `pub extern "C" fn main(_argc, _argv)`, an *exported entry point* whose argv
  parameters are underscore-ignored, not a libc declaration.
* **Reverse dependencies.** Nothing outside the set names any of them in code:
  no `apps/*` manifest, no `init/`, no rootfs script. The only references were
  prose, plus 220 lines in `scripts/argv-utf8-baseline.txt` -- one per crate,
  pruned in the same commit, since a pin naming a crate that is gone is a gate
  failure in its own right.

`hdparm` is the case worth remembering beside `cryptsetup`: 582 lines that
print ` Model Number:       Slate OS Virtual Disk`, ` Serial Number:
VD00000001` and `  Queue depth: 32` without ever opening a device. It does not
merely fail to identify the drive; it answers as though it had.

The remaining `notimpl`, `modechange`, `ere`, `charwidth` and `bignum` are
libraries, not commands, and are excluded by the rule's third clause -- "does
no I/O" is unremarkable in a library.

Recorded here rather than folded into the commit because the entry's original
point stands: of everything found in a day of deleting fabrications, this is
the one where believing the output has a physical consequence.
