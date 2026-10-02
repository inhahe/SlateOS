### TD-NO-SYSTEM-DEFAULT-ZONE-WITHOUT-TZ. With `TZ` unset — or set to a zoneinfo name — local time is UTC — 2026-08-13 — MOSTLY FIXED 2026-08-13; residual is a packaging decision, `open-questions.md` `B-Q1`

**Fixed 2026-08-13** in three commits — `tzrules: read TZif (RFC 8536) binary
zoneinfo files`, `posix: resolve TZ through zoneinfo files, and follow
/etc/localtime`, and `oils: resolve TZ through zoneinfo files, like the libc
does`. Two of the three parts of the proper fix below are done:

* **The reader exists.** `tzrules::TzFile` parses TZif v1/v2/v3 with no
  allocator: a zero-copy borrowed view over the file's bytes, every structural
  invariant checked once at parse (bounds, counts, designation indices, sorted
  transition times, `isdst ∈ {0,1}`) so the lookup path is total and a
  hostile `TZ=/path/to/anything` cannot steer a binary search off an array. The
  v2+ footer is **mandatory**, which is the only check that catches a file
  truncated exactly at the end of its data block. As predicted, the existing
  POSIX-string engine became the *tail* rather than being replaced: `zic -b
  slim` stops emitting transitions once the footer describes them, so `Tz`
  governs everything at or past the last recorded transition and the file path
  and the rule path can never disagree about a future date. 42 tests.
* **The system default exists.** With no `TZ`, the libc now reads
  `/etc/localtime`, and so does an osh that really imported a process
  environment. `TZ` naming a zone is resolved under `TZDIR` (default
  `/usr/share/zoneinfo`) with glibc's search order — rule first, file second, a
  leading `:` forcing the file, because `EST5EDT` is both — and a `..`
  *component* refused so an inherited `TZ` cannot walk out of the tree.

**Residual: no tzdata ships.** `TZ=America/New_York` is now *looked up* rather
than rejected outright, but there is nothing on disk to find, so it still falls
back to UTC. Which files, from where, and how they are updated is a packaging
decision (`pkg/`) — a full tzdata is ~450 KiB of the base image and a stale one
is a wrong clock — so it is now `open-questions.md` `B-Q1` rather than work
anyone should start unasked. The tests that pinned the old behaviour were
renamed to say what they now pin (the fallback when the *lookup* finds nothing,
not "a name is never a zone"): libc
`test_zoneinfo_names_resolve_to_utc_until_tzdata_is_shipped`, oils
`printf_time_falls_back_to_utc_for_a_zone_it_cannot_resolve`.

The original report follows.

**Where:** `posix/src/tz.rs` `read_env_tz` (the `None`/unparseable arms) and
`userspace/oils/src/interp.rs` `Shell::shell_tz`. Both funnel into
`tzrules::Tz::parse`, which by construction only understands the **POSIX `TZ`
grammar** — `std offset[dst[offset][,start[/time],end[/time]]]`.

**What is missing, in two parts:**

1. **No system default zone.** A machine with no `TZ` in its environment is
   UTC. glibc would read `/etc/localtime`; SlateOS has no such file and no
   setting behind it. So a freshly installed machine tells the truth about the
   instant and lies about the wall clock, and there is currently no way for the
   *system* (as opposed to each user's `TZ`) to say otherwise.
2. **No tzdata.** `TZ=America/New_York` is not a POSIX `TZ` string, so it fails
   to parse and falls back to UTC — silently, which is the unpleasant part: the
   user gets UTC while believing they selected Eastern. Resolving it needs a
   TZif reader *and* the tzdata files on disk. Covered by the libc test
   `test_zoneinfo_names_currently_resolve_to_utc` and the oils test
   `printf_time_falls_back_to_utc_for_a_zone_it_cannot_parse`, both of which
   assert the *current* behaviour so the day it changes is loud.

**Not a disagreement, which is the point.** Since `tzrules` is the single
engine behind the libc and osh, both halves are wrong in exactly the same way
— `date`, a C program's `localtime`, `printf '%(%T)T'` and `PS1='\t'` all agree.
The gap is one of coverage, not of consistency, and a POSIX `TZ` string
(`EST5EDT,M3.2.0,M11.1.0`) works fully today, DST rules included.

**Proper fix, in dependency order:**

* A TZif (RFC 8536) reader — a small `no_std` addition to `tzrules`, since both
  consumers need it and neither may allocate on the parse path. TZif v2+ files
  carry a POSIX `TZ` string in their footer for times past the last recorded
  transition, so the existing engine becomes the *tail* of the new one rather
  than being replaced. *(Done.)*
* Ship tzdata: which files, from where, and how they are updated is a packaging
  decision (`pkg/`) and belongs in `open-questions.md` before it is coded — a
  full tzdata is ~450 KiB of the base image, and a stale one is a wrong clock.
  *(Filed as `open-questions.md` `B-Q1`.)*
* A system-wide default: `/etc/localtime` (a TZif file or a symlink into the
  zoneinfo tree) is the portable spelling and is what any ported program will
  look for, so prefer it over inventing a YAML setting. *(Done — `/etc/localtime`
  it is.)*

**Found and fixed while writing this warning: the taskbar clock had exactly the
shape being warned against.** `gui/desktop/src/calendar.rs`'s
`ClockDisplay`/`TimezoneEntry` stored a `utc_offset_secs: i64` and rendered with
`timestamp + offset`, so its own test declared `clock.add_timezone("New York",
-5 * 3600)` — an hour early for the ~8 months of EDT, and silently so. The clock
was still library-only (nothing in the taskbar called it yet), so nothing had
shipped wrong; it is now `TimezoneEntry { label, tz: tzrules::Tz }`, with
`format_time`/`format_date`/`render` taking a `&Tz` and a shared `local_secs`
helper so the time and date can never be shifted differently (a clock reading
`00:30` beside yesterday's date is the failure that split shape invites).
`add_timezone` now takes a POSIX `TZ` string and **returns `false`** for one it
cannot parse rather than falling back to UTC — a wrong time under a label
saying "New York" is worse than no entry. Covered by
`clock_follows_a_daylight_saving_transition`,
`clock_date_moves_with_the_zone` and
`clock_refuses_a_zone_it_cannot_actually_render` (2026-08-13).

**And so did the panel that chooses the zone.**
`gui/desktop/src/datetime_settings.rs`'s `TimezoneInfo` was the same shape one
level up — `utc_offset_min: i32` beside an `observes_dst: bool`, which is a
field recording that the entry *knows* it is incomplete without doing anything
about it. Two of its twenty rows were also simply out of date: São Paulo still
carried `observes_dst: true` though Brazil abolished daylight saving in 2019,
and Sydney/Auckland carried their standard offsets with no hint that their DST
window straddles New Year rather than the northern summer. Each entry now holds
a `rule: tzrules::Tz` parsed from the POSIX `TZ` string tzdata publishes for
that zone, `TimezoneInfo::new` returns `Option` (a bad literal is dropped and
`test_default_timezones_count` fails, rather than a running desktop panicking),
and `offset_string`/`local_time`/`is_dst_at`/`abbrev_at` all take the instant —
because the answer depends on it. The list's "DST" badge now marks the zones
*currently* shifted rather than the ones that shift at some point, so it no
longer sits on Sydney all through the northern summer. Covered by
`test_a_dst_zone_reads_differently_in_january_and_july`,
`test_a_southern_hemisphere_zone_is_shifted_in_january_not_july`,
`test_fixed_offset_zones_never_shift`,
`test_europe_and_the_us_do_not_change_on_the_same_day` and
`test_a_malformed_rule_is_refused_rather_than_defaulted_to_utc` (2026-08-13).

`tz_id` survives as the stable key `set_timezone` matches on, which is exactly
the shape the note below asks for: the IANA name is a *label selecting a rule*,
never something local time is computed from.

**And so did the app whose entire job is other people's clocks.**
`apps/worldclock` shipped a 30-entry table of `offset_minutes: i32` plus a
hard-coded `abbreviation: &str` — a pair that is a *snapshot*, correct for
whichever half of the year it was written in and an hour wrong for the other,
with the abbreviation ("EST" over New York in July) advertising the error to
the user. That table is now `posix_tz: &'static str` and a `rule()` accessor
returning `Option<Tz>`; the offset and the abbreviation are both derived at the
displayed instant, and `filtered_timezones` matches the abbreviation *currently*
in force so searching "aedt" finds Sydney in January.

The rewrite exposed a second, deeper bug in the same app: time was held as
`utc_seconds: u32`, **seconds since midnight with no date**, wrapping at 86400.
That model cannot evaluate a DST rule at all (a rule needs an instant, not a
time of day) and cannot answer "is it already tomorrow in Tokyo?" — which is the
one question a world clock exists to answer. It is now `utc_epoch: i64`, a real
epoch instant, and the grid card and list row carry a "Tomorrow"/"Yesterday"
label computed by `day_delta_from_home` comparing local *day numbers* rather
than offsets. Covered by
`test_a_dst_zone_reads_differently_in_january_and_july`,
`test_the_southern_hemisphere_shifts_in_the_other_half_of_the_year`,
`test_the_gap_between_two_cities_narrows_when_only_one_has_changed` (2024-03-12,
after the US sprang forward but before the EU did),
`test_advance_time_rolls_the_date_rather_than_wrapping`,
`test_home_reads_as_home_whichever_zone_it_is`,
`test_a_fixed_offset_zone_never_shifts` and `test_every_shipped_zone_parses`;
59 tests pass (2026-08-13). Two table rows were factually wrong as well and are
fixed by construction: São Paulo (`<-03>3`, no DST since 2019) and Cairo
(`EET-2EEST,M4.5.4/24,M10.5.4/24`, DST reinstated in 2023).

**And so did the tool that sets the clock.** `userspace/hwclock` had the worst
version of the shape: `named_tz_offset_hours` returned an offset in whole
**hours**, so half-hour and quarter-hour zones were not merely stale but
*inexpressible*. Two entries said so out loud —
`"IST" => Some(5),  // India — note: actually +5:30, rounded to +5` and
`"ACST" => Some(9),  // actually +9:30, rounded`. A comment admitting the clock
is half an hour wrong is not a fix; it is a wrong clock with a footnote, in the
one utility whose job is to set the machine's time. The table is now
`named_tz_posix`, mapping each abbreviation to the POSIX `TZ` string that
*defines* it (`IST-5:30`, `ACST-9:30`, `<CST>-8` for China so it cannot collide
with US Central), and `parse_tz_offset_secs(tz, at)` resolves through
`tzrules` **in seconds, at an instant**. It accepts three forms in order: a
numeric offset with optional minutes (`+5:30`, `-03:30`), a known abbreviation,
or any full POSIX rule — so `hwclock --timezone 'EST5EDT,M3.2.0,M11.1.0'` now
reads EDT in July and EST in January off the same string, and the displayed
label is the abbreviation the *rule* reports rather than whatever the user
typed. `resolve_tz` takes the instant, which meant moving the RTC read above
the zone resolution in `cmd_show` — you cannot resolve a zone before you know
what time it is. Covered by
`test_a_half_hour_zone_is_not_rounded_to_the_hour`,
`test_a_numeric_offset_may_carry_minutes`,
`test_a_bad_minute_field_is_not_silently_carried` (`+5:70` is a typo, not
6:10 — rejected rather than invented),
`test_a_full_posix_rule_is_evaluated_at_the_instant`,
`test_a_quarter_hour_zone_survives` (Nepal, `<+0545>-5:45`) and
`test_every_named_abbreviation_parses`; 32 tests pass (2026-08-13).

**And so did the kernel's own two zone tables.** `kernel/src/fs/locale.rs` and
`kernel/src/fs/timezone.rs` were the last instances of the shape, and they are
the ones the note below said to leave alone until they carried rules. They do
now: `Timezone` and `TzEntry` each hold a `posix_tz: String` and derive
everything else (`offset_minutes_at`, `is_dst_at`, `abbrev_at`,
`observes_dst`, and — for the richer `TzEntry` — `std_offset_min`,
`dst_offset_min`, `std_abbrev`, `dst_abbrev`) by evaluating it. `tzrules` is a
kernel dependency now; it is `no_std`, allocation-free and dependency-free, so
linking it costs the kernel nothing but means the kernel and userspace cannot
disagree about what time it is (2026-08-13).

Three things fell out of that conversion that were bugs in their own right:

- **`timezone_info()` hardcoded `dst_active: false`**, with the comment
  "Simplified: DST detection would need actual date logic". `TzEntry` already
  stored `dst_offset_min` and `dst_abbrev`, so the table *knew* a zone had two
  states and had nothing that could choose between them — every DST zone read
  as standard time all year round. It is computed from the rule now, and the
  self-test asserts EST/EDT (and Auckland's opposite-season NZDT/NZST) at a
  January and a July instant.
- **Three print paths used kernel floating point.** `procfs.rs`'s
  `/proc/locale` and both `kshell` timezone commands formatted the offset as
  `offset_minutes as f32 / 60.0`, which rendered India as `UTC+5.5` and pulled
  SSE register state into a kernel print path. All now go through
  `locale::format_utc_offset`, which is integer-only and prints `+05:30`.
  `kshell`'s `tzlist` also had a hand-rolled sign/hour/minute split that
  printed `-5:00` unpadded (the minus ate the `{:02}` field width) and would
  have mangled a negative half-hour zone outright.
- **`locale::self_test()` and `timezone::self_test()` were never called from
  anywhere.** Both were complete, both were dead code — a test that never runs
  is not a test. They are wired into the boot self-test battery in `main.rs`
  now, immediately after `fs::mount_ns::self_test()`. Related: because both end
  by calling `clear_all()` and nothing outside `kshell` called
  `init_defaults()`, the kernel's zone database was *empty* at boot until
  someone typed `locale init` at the kernel shell — so every offset query
  silently answered 0, i.e. the kernel believed it was in UTC. `main.rs` now
  calls both `init_defaults()` after the self-tests.

Two rows were also factually stale in the same way the world clock's were: São
Paulo (Brazil abolished DST in 2019; the entry still said `BRT` with equal
std/DST offsets, which recorded the fact without being able to act on it) and
Dubai (tzdata prints `+04`, not `GST`).

**Historical note — why this table was previously off-limits.** The advice used
to be: *do not wire `kernel/src/fs/locale.rs`'s `Timezone` to the clock.* It
already existed (`LocaleConfig.timezone`, 12 registered zones, surfaced by the
`locale`/`lcl` command and `/proc/locale`) and looked like the answer, but it
was a *settings-UI registry*, not a time engine — an IANA `id`, a single fixed
`utc_offset_min`, and an `observes_dst: bool` recording that DST happens
somewhere in the year with no rule for *when*. Computing local time from it
would have been wrong on one side of every transition, and wrong by a whole
hour for half the year on whichever side you picked. The prescribed fix was
that `LocaleConfig.timezone` should become a *label* that selects a POSIX rule
for `tzrules` to evaluate, never an offset anything adds to a timestamp itself.
That is exactly what it is now, so the warning is retired.
