# B → C — tzdata is approved (B-Q1 → §311) and the packaging half is in `pkg/`, which is yours

**Filed:** 2026-08-15 by Lane B. **Action needed:** a `tzdata` package in
`pkg/`. Not urgent, but it is the last piece of a feature that is otherwise
finished and currently lies to the user.

## Why this exists

The libc and osh both resolve `TZ` through real binary zoneinfo already:
`tzrules::TzFile` reads TZif v1/v2/v3 (RFC 8536) with no allocator, `TZDIR`
defaults to `/usr/share/zoneinfo`, an unset `TZ` follows `/etc/localtime`
exactly as glibc does, and the search order matches glibc's. All of it is
tested.

**There is nothing on disk to read.** So `TZ=America/New_York` silently answers
**UTC** — the user gets UTC while believing they selected Eastern. That is the
failure mode that makes this worth finishing rather than parking: it is wrong,
and it is quiet.

## The decision (operator, 2026-08-15 — `design-decisions.md` §311)

- **A1 — full tzdata**, including `backward` compatibility links (`US/Eastern`,
  `Asia/Calcutta`). ~450 KiB, ~1 800 files. The slim variant was rejected
  specifically because a script saying `TZ=US/Eastern` — a very common
  spelling — would fall back to UTC *silently*, which is the bug we are fixing.
- **B1 — vendor the prebuilt TZif binaries** from the IANA distribution,
  checked in and version-pinned. **Do not** port `zic` and **do not** write our
  own TZif generator: `zic` is a real compiler for the tzdata source grammar,
  and getting it subtly wrong yields a wrong clock that nobody notices for
  months. Vendoring is the option whose failure mode is "stale", which is
  detectable, rather than "subtly wrong", which is not.
- **C1 — ship and update it as a `pkg/` package.** tzdata changes several times
  a year at short notice; that cadence is what `pkg/` is for. C3 (a dedicated
  fast channel for tzdata alone) is the escalation if C1 proves too slow in
  practice — build it then, not now.

## What I need from `pkg/`

1. A `tzdata` package carrying the full IANA release, version-pinned by release
   name (e.g. `2026a`) so "which tzdata is this machine on?" has an answer.
2. Installed to `/usr/share/zoneinfo`, preserving the link structure — the
   `backward` names are links, and flattening them into copies would quadruple
   the size for no benefit.
3. Whatever the installer needs in order to write `/etc/localtime` for the zone
   the user picks. That file is the unset-`TZ` path and is what makes a fresh
   machine honest about its wall clock.

## The signal that it worked

Two tests currently assert the UTC fallback and **must start failing the day the
data lands**:

- `test_zoneinfo_names_resolve_to_utc_until_tzdata_is_shipped` (libc)
- `printf_time_falls_back_to_utc_for_a_zone_it_cannot_resolve` (oils)

They are named that way on purpose. If they go red, that is the feature
arriving, not a regression — ping me and I will update both in Lane B rather
than you touching `posix/` or `userspace/oils/`.

## Residual risk the operator accepted

A user who never runs `pkg update` drifts into a stale tzdata and a wrong wall
clock, with nothing loud to tell them. Worth keeping in mind if `pkg/` ever
grows a staleness warning — tzdata is the package that most deserves one.
