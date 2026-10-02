## §311 — Ship full tzdata, vendored as prebuilt TZif binaries, updated as a `pkg/` package

**Date:** 2026-08-15
**Decided by:** Operator (Claude recommended this combination — open-questions.md B-Q1, A1 + B1 + C1)

**The situation.** Both the libc and osh resolve `TZ` through real binary
zoneinfo: `tzrules::TzFile` reads TZif v1/v2/v3 (RFC 8536) with no allocator,
`TZDIR` defaults to `/usr/share/zoneinfo`, and an unset `TZ` follows
`/etc/localtime` exactly as glibc does. Every piece was built except the data,
so `TZ=America/New_York` silently answered UTC — the user gets UTC while
believing they selected Eastern. Shipping the data is a packaging decision, not
a coding one, which is why it went to the operator.

**Decision, in three parts:**

- **(a) A1 — full tzdata**, including the `backward` compatibility links
  (`US/Eastern`, `Asia/Calcutta`). ~450 KiB and ~1 800 files in every base
  image. Chosen because ~450 KiB is nothing against being *wrong* about
  `US/Eastern`, and because the entire reason to use TZif rather than invent a
  format is that ported programs expect exactly what everyone else ships.
  A2 (`zic -b slim`, no backward links) saves ~200 KiB and breaks a very common
  spelling **silently, back to UTC** — the same failure mode this work exists to
  end. A3 (minimal at install, rest as a package) leaves a fully-installed-looking
  machine unable to resolve a zone the user did not personally pick.
- **(b) B1 — vendor the prebuilt binaries** from the IANA distribution,
  checked in and version-pinned. Reproducible, no build dependency, ~450 KiB of
  binary per update in git history. B2 (port `zic`) and B3 (write our own TZif
  generator in Rust) were both rejected for the same reason: `zic` is a real
  compiler for the tzdata source grammar, and getting it subtly wrong produces a
  **wrong clock that nobody notices for months**. B3 is the most likely of the
  three to be subtly wrong and would put that risk on our own code.
- **(c) C1 — updated as a `pkg/` package.** tzdata changes several times a year
  at short notice; that cadence is exactly what `pkg/` exists for. C2 (ship with
  the OS image only) ties a timezone fix to a full release. C3 (a dedicated fast
  channel for tzdata alone) is infrastructure to build only once C1 has proven
  too slow in practice — not before.

**The residual risk this accepts.** A user who never runs `pkg update` drifts
into a stale tzdata and therefore a wrong wall clock, with nothing loud to tell
them. If that proves common, C3 is the escalation, and it is additive.

**Cross-lane note.** The reader, the libc paths and osh are Lane B; the `pkg/`
packaging is **Lane C's tree**, so the C1 half lands via a `requests/` entry
rather than directly.

**Where it bites:** `pkg/` (Lane C), `posix/src/tz.rs` (`TZDIR_DEFAULT`,
`LOCALTIME_PATH`, `load_zoneinfo`), `userspace/oils/src/interp.rs`
(`TZDIR_DEFAULT`, `Shell::zoneinfo_dir`), `tzrules/src/tzif.rs` (the reader,
already done), the installer (which must write `/etc/localtime`), and the two
tests that assert the current UTC fallback and **must start failing the day the
data lands** — `test_zoneinfo_names_resolve_to_utc_until_tzdata_is_shipped`
(libc) and `printf_time_falls_back_to_utc_for_a_zone_it_cannot_resolve` (oils).
