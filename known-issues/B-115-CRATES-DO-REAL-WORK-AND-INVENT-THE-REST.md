## B-115-CRATES-DO-REAL-WORK-AND-INVENT-THE-REST (lane B, 2026-09-10) — open

**In short:** After deleting 225 commands that never looked at anything, the
harder half is left: **115 of the 249 remaining `userspace/` crates do genuine
I/O and, somewhere else in the same crate, admit in their own comments to
making something up.** Neither audit rule can see them. Rule 1 exonerates any
crate holding an I/O marker; rule 2 only fires on crates that are wholly
inert. So this is the class that has to be read, not derived.

### Why the audit cannot do it

This is structural and has been since the instrument was written: *any single
I/O call anywhere in a crate exonerates every invented answer beside it.*
`ALSO_FABRICATING` is the manual escape hatch and currently holds one name
(`snapper`). That is a floor, not a measurement.

`cal` is the precedent for the good outcome — real calendar arithmetic over a
real clock, with one fabricated fallback, fixed rather than deleted. `snapper`
is the precedent for the bad one — real timestamps, invented diff.

### The measurement

Crates holding an I/O marker whose non-test source also matches
`simulate[d]`, `fake_*`, `placeholder`, `would (walk|read|query|scan|open)`,
`in a real (implementation|system)`, `for now`, `not actually`, `pretend`,
`dummy_*`, or `stub`. Highest first, count = number of matches:

| n | crate | tells |
|---|---|---|
| 32 | `coreutils` | not actually, placeholder, pretend, stub |
| 14 | `audit` | in a real system, simulated |
| 13 | `sshd` | placeholder, stubbed |
| 13 | `avahi` | simulated |
| 12 | `wpa` | pretend, simulate |
| 12 | `gdb` | placeholder, simulate, stub |
| 11 | `scp` | stub |
| 11 | `dbus` | in a real implementation, placeholder |
| 10 | `firejail` | dummy_path, placeholder |
| 9 | `systemctl` | simulated, would read |
| 8 | `udevd` | simulate |
| 7 | `fdisk` | would read |
| 6 | `ldd` | fake_load_addr |
| 6 | `cron` | simulated |

…and 101 more with 1–5 each.

**Four names left this table by being deleted outright, not fixed**, and how
that happened is the useful part. `fstrim` (22), `modprobe` (9), `smartctl`
(8) and `iw` were in this class only because the audit counted `unsafe {` as
an I/O marker — and their every unsafe block is `cstr_to_slice` /
`from_raw_parts`, i.e. walking their own argv. Once that marker was dropped
they were not "real work plus invention" at all; they were wholly inert, rule
2 caught them, and they went with the other 220. So a crate landing in this
table on the strength of a single weak marker is worth re-checking against
`--markers` before anyone spends time reading it.

**The regex over-reports and must not be used as a delete list.** `quoting`
appeared in it with five hits, all doc-comment prose about how a byte sequence
"would read back" — no admission at all. That is the same error the 1006
deletion nearly made with `cal`, so the rule is the same as before: re-derive,
then read what it flags.

### What the fix looks like

Per crate, not in bulk. For each: find the fabricated path, decide whether the
crate's real work justifies keeping it (fix the path, as `cal`) or whether the
crate is mostly fiction with a real call in the corner (delete, as 1006). Add
the name to `ALSO_FABRICATING` when it must stay flagged meanwhile.

Start with the ones whose fabrication has a physical consequence, in the way
`cryptsetup`'s did: `fdisk` (partition tables), `sshd`/`ssh`/`scp` (claims a
session is authenticated), `dbus` and `udevd` (claim a device or service is
present). `coreutils` is the largest but its placeholders are likelier to be
in rarely-taken branches of otherwise real tools, so it is not the place to
begin.

`fstrim` and `smartctl` were the two originally named here and both turned out
to be inert rather than partly-real, which is worth expecting again: some of
these 115 will collapse into rule 2 the moment a weak marker is examined,
rather than needing a judgement call at all.
