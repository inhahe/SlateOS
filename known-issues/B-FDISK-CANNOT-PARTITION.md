## B-FDISK-CANNOT-PARTITION (lane B, 2026-09-10) — open, and honest about it

**In short:** `userspace/fdisk` reads a disk and describes its partition table
accurately. It cannot *change* one. There is no `File::create`, no
`OpenOptions`, and no write to any device anywhere in its 4,789 lines — the
only `write_all` calls in the crate go to stdout and stderr.

**CORRECTED 2026-09-10: it DID claim otherwise, three times, and this entry
said the opposite because the measurement behind it saw 0.4% of the file.**

`fdisk -n` printed `Created partition: start=..., size=...` and then
**`The partition table has been altered.`** `-d` printed `Partition 2 has been
deleted.` and the same sentence. `-t` printed `Changed type of partition 1 to
'Linux filesystem'.` and the same sentence again. No device was opened for any
of them.

**Why this entry got it wrong, which is the part worth keeping.** The phrase
appears **three times** in the 4,869-line file — and **zero times in the first
21 lines**. Line 21 is where fdisk's first `#[cfg(test)]` sits, which is
exactly where `src.split("#[cfg(test)]")[0]` truncates. That idiom was in
`check-read-defaults` until it was fixed two ticks before this correction, and
`audit-cli-fabrication` documents having fixed the same thing earlier.

So the measurement bug did not merely hide a defect. It produced a written
record asserting the defect was **absent**, in bold, and that record then stood
as the reason not to look again. A wrong answer decays; a wrong answer written
down as a finding compounds.

**Fixed:** all three actions refuse and exit non-zero, naming what is
implemented (GPT structures and CRCs) and what is not (opening the device,
writing LBA 0/1 and the mirror header, re-reading to confirm). Verified with
`rustlex.strip_noise(src, keep_literals=True)` — comments blanked, string
literals kept — so the check is about what the program can *print* rather than
what its source *mentions*.

**The viewer is untouched and remains accurate.** Everything fdisk reports
about a table it READ it read off the actual device.

### What is actually there

`build_protective_mbr`, `build_gpt_header` and `serialize_gpt_entry` construct
real GPT structures with correct CRCs, and are tested. Until today they were
reachable only from `build_test_gpt_disk`, an in-memory fixture; they are
`#[cfg(test)]` now, because leaving write-shaped machinery reachable from a
program that cannot write is how somebody later wires it to a device by
accident and discovers the missing half at the worst moment.

So the pieces for a real implementation exist and are half of the job. The
missing half is the dangerous half: opening the device for writing, writing
LBA 0/1, the mirror header at the last sector, re-reading to confirm, and
telling the kernel to re-scan.

### What the fix looks like

Not "make the stub work". A partition writer that is 90% correct destroys
disks, so the order matters: write to a file-backed image first, verify it
round-trips through this crate's own parser *and* through the host's `sfdisk
--json`, and only then allow a block device — behind an explicit confirmation,
with the mirror header and CRCs written before anything else is touched.

The differential harnesses in `scripts/{sed,awk,expr,cat}-diff.sh` are the
model: compare against the real tool on identical input and name every
deliberate divergence.

**Until then the name is the problem.** Worth considering whether the crate
should install as something that does not promise partitioning, the way
`login-cli` and `loginmgr` were separated in 4182acf8d after two programs both
answered to `login`.
