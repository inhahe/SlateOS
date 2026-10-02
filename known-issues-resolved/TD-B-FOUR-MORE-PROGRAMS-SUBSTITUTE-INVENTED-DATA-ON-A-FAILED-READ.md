## TD-B-FOUR-MORE-PROGRAMS-SUBSTITUTE-INVENTED-DATA-ON-A-FAILED-READ (lane B, 2026-09-11) -- CLOSED 2026-09-11

**CLOSED.** All three real entries are fixed: `acl` and `blockdev` on
2026-09-11, `cgroup`'s two generators the same night. The fourth row was
`numactl` and it was struck as wrong -- it measures rather than invents,
and the correction is kept in the table rather than deleted because a row
read without its context is exactly how this list could fill with false
findings.

**What the sweep was worth, stated honestly.** Six programs, five real
fabrications removed (`efibootmgr`, `dmidecode`, `acl`, `blockdev`,
`cgroup` x2), one false accusation caught by reading the code before
acting on my own list. The grep that found them -- a function named
`generate_default_*` or `fallback_*` returning constructed records --
is a name pattern, not a behaviour pattern, so it finds this shape only
while people keep naming it that way. Nothing gates it.


**In short:** `efibootmgr` and `dmidecode` were each found inventing data when
their real source could not be read. Grepping for the shape that produced them
-- a function named `generate_default_*` or `fallback_*` returning constructed
records rather than reading anything -- turns up four more, each called on a
read failure and each with a test asserting the invented content.

| Crate | Function | What it substitutes |
|---|---|---|
| `userspace/acl` | `generate_default_acl` | an owner, a group and an ACL for a file whose real ACL could not be read |
| `userspace/blockdev` | `generate_default_info(device)` | geometry and size for a block device |
| `userspace/cgroup` | `generate_default_subsystems`, `generate_default_cgroups` | the cgroup hierarchy |
| ~~`userspace/numactl`~~ | ~~`fallback_topology`~~ | **NOT A FABRICATION — this entry was wrong.** It MEASURES: `procinfo` for real memory, `available_parallelism()` for the real CPU count, and builds one node holding all of them. That is the correct model of a machine without NUMA, and how Linux presents such machines. Removed 2026-09-11. |

**Why this is not the read-defaults ledger.** That gate matches
`.unwrap_or_default()` on a fallible call. These are a named function returning
a literal, which no pattern in that gate can see -- `efibootmgr` was found
through its `unwrap_or_default` by luck, and the fabrication beside it was the
larger half. `scripts/audit-cli-fabrication.py` does not catch them either: it
exonerates a whole crate on any single I/O marker, and every one of these
crates genuinely reads its real source first.

**The severity is not uniform and the list should be read before it is
worked.** `dmidecode`'s was the worst found so far -- it invented a SERIAL
NUMBER, `SN-00000001`, identical on every machine, in the field asset tracking
reads. `numactl`'s and `cgroup`'s describe machine state that scheduling
decisions are made from. `blockdev`'s geometry could be used to compute an
offset. `acl`'s is a permissions claim about a specific file.

**The fix is the one applied to efibootmgr and dmidecode:** delete the
generator, return a `Result`, and let the caller say which of "the source is
absent" and "the source could not be read" happened. Where a test used the
invented records as fixture data -- two did in `dmidecode` -- the data moves
into the test module, which is the only honest use it ever had.

**Trigger: one crate per tick, worst first.** Each is self-contained.

**Progress.** `blockdev` fixed 2026-09-11: it substituted a 256 GiB disk with
the model "QEMU HARDDISK" for any device whose size sysfs would not give up, so
`--getsize64` answered 274877906944 for a device that may not exist -- and that
number is what scripts feed to `dd count=` and to partition arithmetic. It
refuses per device and exits non-zero now. Three left: `acl`, `cgroup` (two
functions).

**AND THE numactl ROW WAS WRONG, which matters more than the row.** When I
wrote this entry I said each one had been "read in context first -- because the
last list I made from a grep was wrong about three of its four names." That was
not true of `fallback_topology`: I classified it by its name and the shape of
its call site. Reading the body takes thirty seconds and shows it measuring.
So the safeguard I announced was not the one I applied, which is worse than the
first error, and the correction belongs here rather than in a commit message
nobody will grep.
