## 1038. A util-linux port matches the program Ubuntu 24.04 ships, the upstream fixes it backports included

**Date:** 2026-09-26
**Lane:** B
**Decided by:** Claude (autonomous)

**In short:** each util-linux program in this tree is a port of util-linux
version 2.39.3, checked by running it side by side with the same program as
installed in the test machine (Ubuntu 24.04 under WSL). Ubuntu does not ship
2.39.3 exactly: to `lscpu` it adds five later changes taken from util-linux
itself -- new ARM processor names, and a rework of how `lscpu` sorts CPUs into
kinds, which decides among other things whose feature flags the summary shows
when the CPUs' flags differ. The port includes those five changes, so it
matches the program it is tested against, and util-linux's own later
behaviour.

| Option | *What changes:* | For | Against |
|---|---|---|---|
| **Port what Ubuntu ships: 2.39.3 plus the upstream commits Ubuntu backports (chosen)** | `lscpu` on a machine whose CPUs differ only in their flags shows the first CPU's, not the last's; it names Cortex-X925, A725, Neoverse-V3/N3 and NVIDIA Olympus cores | the reference then agrees byte for byte, so every difference the harness finds is a bug in the port; the backports are util-linux's own later fixes (the regrouping fixes CPU types left without a vendor on hybrid ARM machines, util-linux issue #3062) | "2.39.3" is no longer the whole description; each new port has to look through Ubuntu's patch list for its program |
| Port the 2.39.3 tarball exactly | the last CPU's flags; those five cores unnamed | one well-defined source | the harness can no longer tell a bug in the port from an Ubuntu patch; carries a bug upstream has fixed |

How to find them: `/usr/share/doc/util-linux/changelog.Debian.gz` lists
Ubuntu's patches, and Launchpad serves them
(`git.launchpad.net/ubuntu/+source/util-linux`, `debian/patches/`). For
`lscpu`: LP #2111723 (upstream 7a136d59, new Arm Cortex part numbers;
eb6514b4, CPU-type de-duplication; and two test-data commits) and LP #2123886
(upstream 90877747, NVIDIA Olympus). The earlier ports -- `lsmem`, `prlimit`,
`column`, `lsirq`, `getopt`, `flock` -- are untouched by Ubuntu's list, whose
other patches are to `cfdisk`, `fincore`, `fadvise`, `setarch`, `wall`, `su`,
`sulogin`, libuuid, libblkid and libmount.

The two test-data patches carry a nineteenth `lscpu` snapshot, a hybrid ARM
machine with four kinds of core, which is exactly what exercises the
regrouping; `scripts/util-linux-source.sh` fetches them with checksums and
applies them.

**Where:** `userspace/lscpu/src/cputype.rs` (`deduplicate_cputypes`),
`userspace/lscpu/src/arm.rs` (the part tables), `scripts/util-linux-source.sh`.

**Revisit** when these ports move to a later util-linux release, whose base
then already holds the backports.
