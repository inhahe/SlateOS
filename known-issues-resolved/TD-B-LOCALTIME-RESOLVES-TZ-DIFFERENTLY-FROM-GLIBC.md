## TD-B-LOCALTIME-RESOLVES-TZ-DIFFERENTLY-FROM-GLIBC (lane B, 2026-09-25) — FIXED 2026-09-26

**In short:** every program that prints a time reads `TZ` through
`localtime::Zone::resolve`, whose module docs say it follows glibc's four
rules. Two of them are not glibc's. `TZ=` (set but empty) is *UTC* to glibc,
spelled `Universal`; ours reads `/etc/localtime`. And for a value that is both
a zoneinfo file and a POSIX rule -- `EST5EDT`, `CST6CDT`, `MST7MDT`, `PST8PDT`
-- glibc tries the **file first**; ours parses the rule first. So `TZ= date`
prints the machine's zone instead of UTC, and `TZ=EST5EDT date -d
'2000-03-20 12:00'` says EDT (2007's rules) where GNU says EST (the file's
history).

**Measured** in WSL (glibc 2.39, `/etc/localtime` → America/New_York):

| command | GNU | ours |
|---|---|---|
| `TZ= date +%Z%z` | `Universal+0000` | `EDT-0400` |
| `TZ=EST5EDT date -d '2000-03-20 12:00' +%Z` | `EST` | `EDT` |
| `TZ=EST5EDT,M3.2.0,M11.1.0 date -d '2000-03-20 12:00' +%Z` | `EDT` | `EDT` |

**What glibc actually does** (`time/tzset.c`, `tzset_internal` and
`__tzset_parse_tz`; `time/tzfile.c`, `__tzfile_read`):

1. Unset: `/etc/localtime`; if that cannot be read, UTC named `UTC`.
2. Empty: the name `Universal`, then as below.
3. A leading `:` is dropped.
4. **A file is tried first**, as `TZDIR/NAME` (or the absolute path).
5. Only if there is none, the POSIX rule -- and a rule that fails part-way
   keeps what it parsed: a standard name of three or more letters survives an
   offset that does not parse (`TZ=Foo/Bar` is UTC *named `Foo`*), and a rule
   with a DST name but no transition dates takes them from `posixrules` if
   that file exists, else the US rules. Measured: `TZ=AAA3BBB` follows New
   York's *history* (1974's year-round DST, 1990's April start), and after the
   file's last transition uses New York's own footer, names and offsets
   included -- `TZ=AAA3BBB date -d 2040-07-01` says `EDT -0400`.
6. A POSIX rule's transitions for any year up to 1970 are computed from
   1970-01-01 (`compute_change`: `if (year > 1970) … else t = 0`), so under a
   northern rule no instant before 1970 is daylight time, and under a southern
   one every such instant is. `TZ='CET-1CEST,M3.5.0,M10.5.0/3' date -d
   0021-06-15` is CET to glibc and CEST to us -- and the instant differs by the
   hour. `localtime` computes each year's own transitions.

**Where:** `userspace/localtime/src/lib.rs`, `Zone::resolve` (rules 1 and 3 of
its module docs), and `tzrules::Tz::parse`, which refuses rather than keeps a
partial rule (and refuses an hour over 24 where glibc clamps it).

**The proper fix** is to make `resolve` glibc's order -- empty is `Universal`,
file before rule -- and to give it glibc's fallback for a rule that does not
fully parse. The part that lives in `tzrules` is not lane B's (it is no lane's;
A-Q11), so the partial-rule fallback belongs in `localtime`, built from
`tzrules`' pieces, unless its owner takes it. `scripts/parse-datetime-diff.sh`
avoids the affected `TZ` values until then, and says so in its header.

**Severity: medium.** Silent and wrong rather than refused, but confined to
`TZ` values that are empty, invalid, or one of four legacy names -- and `TZ=`
is plausible in a script.

**How it was closed (2026-09-26).** Not by adjusting `resolve`: the list above
was what the harness happened to reach, and reading glibc 2.39's
`time/tzset.c` and `time/tzfile.c` found more than it listed. `localtime` now
has a `tzset` module that ports them function by function, with `tzrules`
kept as the TZif decoder only (four additive raw accessors: `transition`,
`type_count`, `local_type` with the indicator flags, `footer`). What the port
reproduces that the entry did not know about, all measured against glibc:

* **Anything after the standard offset that is not a DST name** leaves an
  unnamed, zero-offset DST half, and two zeroed rules that read as a southern
  zone -- so `TZ=EST5x` is UTC with an empty name for all but five hours of
  every year.
* **`posixrules` re-anchoring is anchored by a process-wide static.**
  `__tzfile_read` never sets `rule_dstoff` for a file with transitions, so the
  first use in a process moves New York's fall transitions by the whole DST
  offset (`TZ=AAA3BBB`: 04:00Z, not 06:00Z); `__tzfile_default` then sets it
  to the user's DST offset, and every later read -- which is every `mktime`,
  because `__tzfile_default` leaves `old_tz` NULL -- anchors differently. So
  `date -d @1604203200` and `date -d '2020-11-01 02:30'` disagree about the
  same zone, in glibc and now here.
* **gnulib switches `TZ`** for a date string's `TZ="..."` (`set_tz`,
  `revert_tz`), which reads that zone and then the process's own again; `Zone`
  is read lazily, can be re-read in place (`Zone::tzset`, `reread`,
  `switched`), and `parse_datetime`'s `mktime_z`/`localtime_rz` do what
  upstream's do.
* `compute_change` runs in the **UTC year** of the instant, and its per-rule
  cache starts from the `memset`, which glibc's first year-0 lookup can see.
* Before the first transition a zone file gives its **first standard type**,
  and a file with **no transitions never reads its footer**.
* The programs whose upstreams call glibc's non-reentrant `localtime()`
  (`find`, `ps`, `tar`, `pinky`) now convert through `Zone::localtime`, which
  does the `tzset ()` that call does.

Checked by the new `scripts/tz-diff.sh`: 59 `TZ` values over 26 instants,
local-time strings one per process and as one `date -f`, and `TZ="..."`
strings under five process zones, against GNU `date` 9.4 on WSL's glibc -- no
difference but one, which is a decision (below). `parse-datetime-diff.sh`'s two
markers for this entry are gone and its rows pass.

**What is deliberately not reproduced** (the `tzset` module's docs have the
reasons): leap seconds from `right/` zones; a zone-file footer with a DST name
and no dates (glibc would run `__tzfile_default` mid-lookup; `zic` never
writes one); `M0`/`M13` rules, which make glibc read outside `__mon_yday`;
abbreviations over 32 bytes, cut to 32 where they reach a `TzInfo`; and a
`TZ` naming a file through a `..` component, which is never read as a file
here -- glibc refuses one only in a setuid program, and the libc
(`posix/src/tz.rs`) refuses it always, so the two readers of one `TZ` stay in
agreement. `tz-diff.sh` runs that one as an xfail.
