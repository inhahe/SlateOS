### TD-OILS-BROKEN-DOWN-TIME-IS-ALWAYS-UTC. `printf '%(%T)T' -1` and `PS1='\t'` are hours off — 2026-08-09 — ✅ FIXED 2026-08-13

**Fixed by** extracting the libc's POSIX `TZ` engine into the dependency-free
`no_std` crate `tzrules/` and linking it into *both* `posix` and
`userspace/oils`. `format_strftime` now takes a `&tzrules::Tz`, shifts the epoch
by `tz.lookup(epoch).gmtoff` before breaking it down, and renders `%z`/`%Z` from
that same `TzInfo` — so a DST-observing zone reports `EDT`/`-0400` in July and
`EST`/`-0500` in January instead of `UTC`/`+0000` always. `%s` is deliberately
left unshifted: it names the instant, not a reading of any clock.

Fixing only the libc would have been worse than fixing neither — osh renders
broken-down time itself and never calls `strftime`, so the shell would have gone
on disagreeing with every C program on the machine. Hence the shared crate
rather than a `posix` module; see `tzrules/src/lib.rs`'s "Why this is a crate
and not a module".

The zone is resolved by `Shell::shell_tz` from the shell's **exported** `TZ`,
which is bash's rule rather than an approximation of it: `variables.c:sv_tz`
calls `tzset()` only for an exported (or newly unset) `TZ`, because `strftime`
reads the *process* environment and only the export attribute puts a shell
variable there. So `TZ=EST5 printf '%(%Z)T'` with no `export` really is UTC in
bash and is UTC here, while `export TZ=…`, an inherited `TZ`, and an assignment
*prefix* all take effect. A test shell has nothing in `exported`, so the suite
renders in UTC on every host without pinning `TZ` — the determinism falls out of
bash's rule instead of being carved out of it, which answers the "the corpus
cannot simply stop pinning" worry below.

Tests (`interp.rs`): `printf_time_renders_in_the_exported_zone`,
`printf_time_percent_s_is_zone_independent`,
`printf_time_ignores_an_unexported_tz`,
`printf_time_falls_back_to_utc_for_a_zone_it_cannot_resolve`,
`prompt_time_escapes_render_in_the_exported_zone` — the last uses the DST-less
`NPT-5:45` because a prompt renders *now*, so a DST zone's `%Z` would assert one
thing in July and another in January.

**Residual, tracked separately as `TD-NO-SYSTEM-DEFAULT-ZONE-WITHOUT-TZ`:** an
unset `TZ`, and a zoneinfo name like `America/New_York` (which is not a POSIX
`TZ` string), still resolve to UTC — in the libc and the shell alike. That is
now a *consistent* gap rather than a disagreement, and closing it needs tzdata
on the machine. *(Update 2026-08-13: the reader and the `/etc/localtime`
default now exist on both sides; only shipping tzdata remains — see that
entry.)*

The original report follows.

**Where:** `userspace/oils/src/interp.rs`, `format_strftime` (~55980) — it does
`epoch.div_euclid(86_400)` / `rem_euclid` straight off the epoch seconds, i.e.
UTC. Every caller inherits it: `push_prompt_strftime` (the `\t` `\T` `\@` `\A`
`\d` `\D{…}` prompt escapes, ~17063) and the `printf '%(FORMAT)T'` builtin.

**Reproduce** (dev host is UTC-4):

```
$ bash -c "printf '%(%T)T\n' -1"   ->  04:11:48
$ osh  -c "printf '%(%T)T\n' -1"   ->  08:11:48

$ bash -c "printf '%(%T)T\n' 0"    ->  19:00:00     (1969-12-31 19:00 local)
$ osh  -c "printf '%(%T)T\n' 0"    ->  00:00:00

$ v='\t'; "${v@P}"                 ->  bash 04:11:32,  osh 08:11:32
```

`date +%T` agrees with bash in both shells, so it is not the clock.

**Why.** bash's `printf` builtin and `decode_prompt_string` both format
`localtime (&t)`; osh formats the epoch directly. `%z`/`%Z` are wrong for the
same reason — they answer `+0000`/`UTC` unconditionally.

**Why it has not shown up in a sweep.** Every corpus and unit-test case that
renders a time pins `TZ=UTC` first (`interp.rs` tests ~63898-63965), which makes
the two agree. Any case that does *not* pin it is nondeterministic across
machines, so the corpus cannot simply stop pinning.

**Proper fix.** Give `format_strftime` a zone offset argument and a single
resolver for it:

* `TZ` set to a POSIX zone string (`EST5EDT,M3.2.0/2,M11.1.0/2`) — parse it,
  including the DST rule, which is enough for `%z`/`%Z` too.
* `TZ` set to a zoneinfo name (`America/New_York`) — needs a tzdata reader; on
  SlateOS that means shipping tzdata, which is a decision of its own.
* `TZ` unset — the host's current offset (`GetTimeZoneInformation` on the
  Windows dev build; a system setting on SlateOS).
* `TZ` empty or `UTC` — today's behaviour.

Then the corpus can drop its `TZ=UTC` pins for the POSIX-string cases and gain a
case that sets `TZ` explicitly to a fixed offset, which is deterministic
everywhere.

**Note:** `format_strftime`'s doc comment says "see known-issues TD-OILS" with
no tag — this entry is the referent; point it here when fixing. *(Done: the doc
comment now points at `Shell::shell_tz` and describes the real rule.)*
