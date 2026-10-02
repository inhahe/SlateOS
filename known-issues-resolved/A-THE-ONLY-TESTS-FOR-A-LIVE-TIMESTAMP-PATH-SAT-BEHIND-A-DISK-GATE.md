## A-THE-ONLY-TESTS-FOR-A-LIVE-TIMESTAMP-PATH-SAT-BEHIND-A-DISK-GATE (lane A, 2026-09-12) — FIXED

`dos_datetime_to_ns` converts a FAT on-disk date/time into a Unix timestamp and
is called in production by the stat path (fat.rs:3282-3285) to report the times
a user sees. Its only tests lived inside `fat::self_test`, which is dispatched
under `if fat_ok` and has never run on this harness -- so a function on a live
path had **no executing coverage at all**. fat.rs has no `#[cfg(test)]` module
either; the one match for that string is inside a doc comment.

Found while writing up the RAN-IF entry above, by reading the dead suite's
section headers rather than its size: one is titled `pure computation, no disk
I/O`. A previous session moved seven whole dispatches out from behind `fat_ok`
for exactly this reason. It could not have found this one -- this is a pure
*section nested inside* a disk-dependent suite, not a suite of its own.

Now `fat::self_test_datetime()`, dispatched unconditionally beside the other
`runs on any root` suites. Two judgement calls in the move:

- The bare `assert_eq!(dos_datetime_to_ns(0, 0), 0)` became an `Err` return. A
  panic halts the kernel and reports nothing about the suites queued behind it.
  That cost nothing while the code never ran and costs the rest of the boot now
  that it runs every time.
- The `rtc_to_dos_datetime` -> `dos_datetime_to_ns` round-trip was left behind
  deliberately. A round trip can pass while both directions are wrong in
  compensating ways; it is a mirror, not a witness. What was worth rescuing is
  the absolute values -- 1980-01-01 -> 315532800s and 2000-06-15T14:30Z ->
  961078200s -- which can only pass by being right.

**Is the rest of the dead suite rotten too? Audited: no, and the reason is structural.**

The wrong constant is an argument for auditing the other ~1,100 lines behind
`if fat_ok`, which have also never run. Done, and the answer is that almost
nothing there can rot the same way: the expectations are *self-referential*
-- write N bytes and expect N back, set an attribute and read it back -- so
both sides of every comparison come from the same source and cannot drift
apart while unobserved. The only hand-written absolute left is the DOS epoch
(315532800), checked and correct.

That is what made the 2000-06-15 vector uniquely vulnerable: it is the one
value a human typed from arithmetic done in their head. The flip side is that
a round-trip is a mirror, so most of that suite would be weaker coverage than
its length suggests even once it runs -- which is worth knowing before anyone
spends a day giving the harness a FAT volume to unlock it.
**On its first boot the rescued test failed -- and the code was right.**

```
[fat]   dos_datetime_to_ns FAILED: 2000-06-15 14:30 = 961079400000000000,
                                   expected 961078200000000000
```

The delta is 1200s exactly. `961078200` is 2000-06-15T**14:10**Z: the *expected*
constant was twenty minutes early, and the kernel had been right all along.
Checked against Python's `datetime` before touching anything, because the
tempting read -- a brand-new failing test means broken production code -- would
have had me 'fixing' a correct conversion. The DOS-epoch vector in the same
block (315532800) is right, so the two disagree and only one could be wrong.

This is the argument against leaving dead tests in place, made by the tests
themselves. A suite that never runs is not inert: it rots quietly, and what it
accumulates is *accusations against working code*. Had this ever executed, the
wrong constant would have been caught the day it was written. Instead it sat
behind `if fat_ok` looking like coverage.
Verified the wiring gate *sees* it rather than trusting its exit 0, since a pass
and a silent skip are the same observation: self-tests defined went 1319 -> 1320,
run at boot 1317 -> 1318, reachable from nothing 0.
