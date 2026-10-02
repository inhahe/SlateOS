### TD-OILS-PRINTF-TZ. `osh` renders `printf '%()T'` (and prompt `\d \t \T \@ \A`) in **UTC**, not local time; bash uses the system timezone — ✅ FIXED 2026-08-13

**Superseded by `TD-OILS-BROKEN-DOWN-TIME-IS-ALWAYS-UTC`,** which carries the
fix write-up. This is the third entry filed for one bug (with `TD-OILS9`), which
is itself worth noting: the same defect was rediscovered and re-analysed twice
because the earlier entries were filed under names nobody searched for.

**The "why deferred" reasoning below was wrong on both counts, and it is worth
recording why, because the shape of the mistake generalises.**

*"oils is dependency-free, so this needs an external crate"* — it needed no
external crate. It needed ~900 lines of calendar arithmetic that the libc had
to grow *anyway* for `localtime_r`, which is now `tzrules/`: `no_std`,
allocator-free, zero dependencies, linked into both. The constraint was read as
"no new code from outside", when what it actually says is "no `std`, no
allocator, nothing that breaks the `x86_64-slateos` build" — and in-tree code
that respects those is not a dependency in the sense being avoided.

*"Honouring only `TZ` would be a partial band-aid that still misses bash's
default (system-localtime) case"* — this had it backwards. `TZ` is not the
partial case; it is the *whole* mechanism bash uses. bash has no
system-localtime path of its own: it calls `strftime`, and the libc resolves
the zone from `TZ`, falling back to `/etc/localtime` only when `TZ` is unset.
Implementing `TZ` gets everything except the fallback file — and the fallback
was the part that was genuinely blocked, since it needs tzdata (now
`TD-NO-SYSTEM-DEFAULT-ZONE-WITHOUT-TZ`). The 90% that was available was parked
because it was mistaken for the 10%.

The lesson for other entries reading "infrastructure-blocked": check whether the
blocked part is the *whole* feature or only its last mile, and check whether the
missing infrastructure is genuinely missing or merely not yet written down as
in-tree code.

The original report follows.

**Where:** `userspace/oils/src/interp.rs` — `format_strftime` (~11901) computes
`days`/`hour`/`minute` straight from the raw epoch with no timezone offset, and
has no `%z`/`%Z` handling (an unrecognised `%z` is emitted literally). Callers:
`printf '%(FMT)T'` (~11650) and the prompt time escapes `\d \t \T \@ \A`
(~2759–2794). "Now" comes from `SystemTime::now()` (UTC epoch).

**What:** bash formats `%()T` with the local timezone (via libc `localtime`),
so on a UTC-5 machine `printf '%(%Y-%m-%d %H:%M:%S %z)T\n' 0` prints
`1969-12-31 19:00:00 -0500`; osh prints `1970-01-01 00:00:00 %z`. `%z`/`%Z` are
also unsupported. The date/time *math* is correct — only the zone offset (and
the two zone specifiers) are missing.

**Why deferred (infrastructure-blocked):** oils is intentionally
**dependency-free** (Cargo.toml has zero deps — required for the clean
`x86_64-slateos` build), so pulling in `chrono`/`time` for local-offset support
is off the table. Rust `std` cannot obtain the local UTC offset soundly
(`localtime_r` is unsound across threads and not exposed). The correct fix needs
SlateOS to expose the machine's UTC offset (from its RTC/settings service) and
osh to query it; on the host build it would read the OS offset. Honouring only a
`TZ=±HH:MM`/`TZ=UTC0` env var would be a partial band-aid that still misses
bash's default (system-localtime) case, so it's parked until a real time/zone
facility exists. Impact: low — affects only wall-clock *display* in `%()T` and
the time-bearing prompt escapes.
