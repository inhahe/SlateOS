### TD-OILS9. `osh` `printf '%(FMT)T'`: time is always formatted in UTC, not local time — ✅ FIXED 2026-08-13

**Superseded by `TD-OILS-BROKEN-DOWN-TIME-IS-ALWAYS-UTC`,** which is the entry
with the fix write-up: `format_strftime` now takes a `&tzrules::Tz` resolved
from the shell's exported `TZ`, and `%z`/`%Z` come from the zone in effect at
the rendered instant. The `%Z`-prints-`UTC`-not-`GMT` note below still stands
(a host-libc naming difference, not an osh bug — the slateos target is
glibc-like and `tzrules` answers `UTC`). The `-2` approximation below is
**still open** and unrelated to zones: `format_printf` is a free function with
no access to the shell's start instant, so `%(…)T -2` renders "now".

The original report follows.

**Where:** `userspace/oils/src/interp.rs` (`format_strftime`, called from
`format_conversion`'s `%(…)T` branch).

**What:** bash's `printf '%(FMT)T'` formats the broken-down time in the
shell's *local* timezone (honoring `$TZ` / the system zone). Our
implementation renders the time in **UTC** because SlateOS has no timezone
database and no `$TZ` handling yet. This shifts not just the zone name/offset
but the actual broken-down values near a day/year boundary: e.g.
`printf '%(%Y)T' 0` (epoch 0 = 1970-01-01 00:00:00 UTC) prints `1970` in osh
but `1969` under a negative-offset local zone like `EST` (bash → 1969-12-31
19:00 local). All calendar math (`civil_from_days` /
`days_from_civil`) and the specifier set (`%Y %C %y %m %d %e %H %I %k %l %M
%S %p %P %A %a %B %b %h %j %u %w %s %z %Z %V %G %g %n %t %F %T %R %D %r %c
%x %X %%`) are correct; only the local-zone offset is missing. Under the UTC
model `%z`→`+0000` and `%Z`→`UTC` (added 2026-07-20 alongside `%k %l %r %V
%G %g %c %x %X`; all verified equal to `TZ=UTC bash` across many epochs and
ISO-week boundaries). Minor: `%Z` prints `UTC` (the glibc/target value)
whereas the MSYS reference bash prints `GMT` for `TZ=UTC` — a host-libc name
difference, not an osh bug (the slateos target is glibc-like). Also, bash's
`-2` argument ("time the shell was started") is approximated as the current
time, since `format_printf` is a free function without access to the shell's
start instant.

**Proper fix:** once SlateOS grows a timezone database / `$TZ` parsing,
apply the local UTC offset (and DST rules) before breaking the epoch down,
and thread the shell start instant through so `%(…)T -2` is exact. Deferred:
UTC formatting is correct and deterministic, and scripts that need a
specific zone can compute the offset explicitly.
