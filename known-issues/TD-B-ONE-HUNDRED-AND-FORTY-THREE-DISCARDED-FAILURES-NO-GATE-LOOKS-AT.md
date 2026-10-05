## TD-B-ONE-HUNDRED-AND-FORTY-THREE-DISCARDED-FAILURES-NO-GATE-LOOKS-AT (lane B, 2026-09-11)

**In short:** `check-read-defaults` catches `.unwrap_or_default()` on a call
that could fail. `.unwrap_or(0)` is the same defect with a literal in place of
`Default`, and nothing looks for it. There are **143 such sites in 24 crates**,
against 68 in the ledger that is watched.

**How it surfaced.** `userspace/acpi` had seven sites pinned in the read-defaults
ledger. Reading all seven found them fine — `status` maps its empty string
straight to `BatteryStatus::Unknown`, and the descriptive fields print only when
non-empty, which is right for an absent sysfs attribute. The real defect in that
file was one line the gate cannot see: `read_sysfs_i64(&path.join("temp"))
.unwrap_or(0)`, so a thermal zone whose sensor did not answer printed `0.0
degrees C` — and for a thermal sensor zero reads as very cool. Fixed
2026-09-11.

**DO NOT WIDEN THE GATE TO `.unwrap_or(<literal>)` WITHOUT READING A SAMPLE
FIRST.** That was my first instinct and the sample says it is wrong. The 143
are at least two unlike populations:

| Family | Example | Is the default wrong? |
|---|---|---|
| Reads of the world | `read_sysfs_u32(&path.join("cur_state")).unwrap_or(0)`, `get_file_mtime(p).unwrap_or(0)` | Usually yes — same as the pinned ledger |
| Parsers over a buffer | `read_u16_le(buf, 16).unwrap_or(0)`, `read_u32_le(data, base).unwrap_or(0)` | A truncated ELF/DNS packet field becomes 0. A defect, but a different question with a different fix |
| Documented defaults | `parse_size(value).unwrap_or(4096)` | No — 4096 is the stated default for a missing config value |

The buffer parsers dominate the count (`gdb` 32, `dig` 20, `file` 6, `readelf`
6 are mostly these), so a naive widening would bury the world-read family under
them and pin a large number of sites whose correct treatment has not been
decided. `check-read-defaults`' own docstring records what that costs: the
lookbehind that separated 39 findings from 171 exists because "a gate becomes
noise and then becomes bypassed".

**The proper next step** is to widen only the FIRST family — a literal default
on a call that reads the world — which means the std readers and local
functions that touch the filesystem, not every local function returning
`Option`. Measure the count for that subset alone before pinning anything.

**DONE 2026-09-11, and the subset is small: 18 sites in 7 crates**, against 143
for the unscoped pattern. The filter is transitive — a local function reads the
world if its body does, or if it calls one that does — which keeps
`read_sysfs_u32` and `get_file_mtime` and drops the buffer parsers
(`read_u16_le(buf, 16)`) that dominate the raw count.

All 18 were READ rather than pinned, which is what a population this size is
for. Fixed: `lspci` (an unreadable `vendor` became PCI ID 0000, printed as
"Unknown vendor 0000"), `acpi` and `thermald` (a sensor that did not answer
became 0 degrees and the classifier called the zone OK), `lsof` (a process
whose uid could not be read was attributed to ROOT), `vmstat` (an unreadable
`/proc/uptime` became a 1-second DIVISOR, so since-boot rates printed as raw
totals — wrong by 86,400 on a machine up for a day).

Left alone deliberately, with the reason recorded at each site: `ar`'s mtime
(zero is already its `-D` deterministic value), `acpi`'s `cur_state`/`max_state`
and `lspci`'s class/revision/IRQ/subsystem (zero is a legitimate reading for
every one), `ntpd`'s drift (a missing drift file means zero drift, which is
what ntpd itself assumes).

**No gate was built for this subset, and that is the recommendation.** Eighteen
sites, now all either fixed or annotated, is a population where a ratchet would
cost more than it caught — and the interesting half, deciding whether a literal
default is wrong, is exactly the judgement a regex cannot make. The larger 143
stays unpinned for the reason above.
