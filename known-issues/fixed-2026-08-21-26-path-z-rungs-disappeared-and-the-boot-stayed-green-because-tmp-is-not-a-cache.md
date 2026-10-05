## FIXED 2026-08-21 — 26 Path Z rungs disappeared and the boot stayed green, because `/tmp` is not a cache

**Where:** `scripts/create-ext4-rootfs.sh` (tcc discovery, ~line 738) and
`kernel/src/proc/spawn.rs::pathz_report_skips`.

**What happened.** A benchmark boot on `lane-a` reported:

```
=== PATH-Z COVERAGE INCOMPLETE ===
  Path-Z prerequisites: 26 rung(s) SKIPPED — coverage is INCOMPLETE
  [spawn]   SKIP: REAL C compiler (tcc, ring 3, Path Z) — prerequisite missing: /mnt/bin/tcc
  ... and 25 more
=== Boot test PASSED ===
```

Twenty-six rungs of the Path Z ladder — every test that proves an unmodified,
prebuilt C *compiler* runs in ring 3 under our kernel and produces a working
executable — did not run. The boot was green, the streak counter incremented,
and nothing in the summary read as a regression.

**Root cause.** `create-ext4-rootfs.sh` looked for tcc in exactly two places:
`command -v tcc`, and `/tmp/tccinstall/bin/tcc`. tcc is not on a default Ubuntu
install and `apt install tcc` needs root, so in practice it was always the
second one — and `/tmp` is cleared when WSL restarts. When it went, the next
`create-ext4-rootfs.sh` run staged an image with no compiler in it, and the
skips followed silently from there.

**Why it stayed invisible for a while.** The report line said:

> (rebuild rootfs.ext4 via scripts/create-ext4-rootfs.sh; see the SKIP lines above)

That is the wrong remedy for the failure that actually occurs. Rebuilding the
image when the *host* has no tcc produces an identical image with the same 26
skips. Anyone who followed the instruction would have got no new information and
learned that the line means nothing — which is worse than no advice.

**Fix, in two parts.**

1. **A cache that survives a reboot.** The script now searches
   `~/.cache/slateos/tccinstall/bin/tcc` *before* `/tmp`, and the comment gives
   the build recipe pointed at that prefix. `/tmp` is still accepted, since that
   is where the original instructions put it, but it is no longer the
   documented home. tcc has been built there on this host.
2. **A remedy that matches the failure.** `pathz_report_skips` now says that
   each SKIP line names the missing file, that rebuilding helps only if the host
   can supply it, where a tcc is looked for, and to install first and rebuild
   second.

**Deliberately not changed: an absent prerequisite still does not fail the
boot.** That matches the rule already settled for absent-vs-stale fixtures
higher in this file — a skip reports nothing *and says so*, whereas a stale
artifact reports OK and is wrong. Failing a fresh checkout that simply has no C
compiler would be punitive, and the loud `PATH-Z COVERAGE INCOMPLETE` banner is
the right instrument. What was missing was not a gate but a cache and an
accurate sentence.

**The lesson worth keeping.** A tool cached in `/tmp` is a prerequisite that
un-installs itself on a schedule nobody controls, and the failure it produces is
a *quieter* test run rather than a broken one. When a build step degrades
gracefully on a missing input, the input needs a home that outlives a reboot —
graceful degradation plus a volatile cache is a coverage leak with no alarm on
it.
