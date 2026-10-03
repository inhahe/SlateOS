### TD-A-A-BARE-METAL-RUN-RECORDS-ITS-ACCELERATOR-AS-ABSENT, WHICH IS THE ONE CONFLATION §237 EXISTS TO PREVENT (lane A, 2026-08-19) -- FIXED 2026-08-19; see "Resolution" at the end of this entry. The `boot-history.jsonl` sibling at the end remains OPEN.

**In short:** Every benchmark record now stores which emulator it ran on, so
that a fast run under hardware acceleration is never compared against a slow one
under software emulation. It reads that from a line the kernel prints at boot.
But the kernel only prints that line **when it is running under something** --
on real hardware it prints a different sentence, so the field comes out empty.
Empty is also exactly what an old log looks like. So the day we finally run the
benchmarks on a real PC, those results would be filed as "unknown", pooled with
years of old emulator runs, and compared against them.

**Where:** `scripts/bench-history.py` -- `ACCEL_RE` (line ~160) and
`parse_accel()` (line ~604). Kernel side, `kernel/src/hypervisor.rs:235-243`.

**The defect.** `detect()` prints the parseable banner only inside
`if hv.is_virtual()`:

```rust
if hv.is_virtual() {
    serial_println!("[hypervisor] Detected: {} (signature: {:?})", hv.name(), signature_str());
} else {
    serial_println!("[hypervisor] Running on bare metal (no hypervisor detected)");
}
```

`ACCEL_RE` matches only the first. So `parse_accel()` returns `None` for a
bare-metal boot -- and `None` is documented, correctly, to mean *"the log
predates the banner"*. §237's whole argument is that absent must be its own
value because folding it into a known one widens a band and a wider band
dismisses real regressions in silence. This is that same fold, arriving by the
back door: two genuinely different platforms both render as absent, and the
grouping key `(commit, accel)` cannot tell them apart.

**Why it matters more than it looks.** This is not a hypothetical platform. It
is **Q53 option D** and the fallback in **Q54** -- "get real regression numbers
off real hardware" is the only option on the table that fully restores
`CLAUDE.md`'s 10% threshold, because the layout noise behind it is a TCG
artefact. The first thing that work would produce is a set of records that this
code mislabels. It would not be noticed either, because the mislabelling is
silent and the resulting group is *plausible*.

The saving grace today is only that `(commit, accel)` includes the commit, so an
old absent-accel record and a future bare-metal one at different commits do not
in fact share a group. That is luck, not design: it fails the moment anyone
benchmarks a commit that also has pre-banner history, which is every commit
before 2026-08-19.

**The fix** is script-side and works on logs already written, which is why it is
preferable to changing the kernel's output: teach `parse_accel()` the second
sentence and return `"bare metal"` for it. Two anchored patterns, tried in
order, with a test for each and a test that the two do not cross-match. The
kernel already prints enough to distinguish them; only the reader is blind.
`Hypervisor::name()` already has `"bare metal"` as the `None` variant's string,
so the returned value stays in the same vocabulary as every other accelerator
and no new token is invented.

**Not fixed on the spot** only because a six-arm WHPX layout sweep was in
flight, and `bench-history.py` is re-invoked by `boot-test.sh` once per arm, so
an edit lands mid-run. The change is provably a no-op for those arms (they are
all `Hyper-V/WHPX`), but a typo in it would not be, and the sweep is ~1 h of
machine time. Fix immediately after it lands.

**Smaller sibling, same file family.** `bench/boot-history.jsonl` records no
accelerator at all. The new `experiment` field covers the decision-relevant case
today, because `QEMU_EXTRA` is how the accelerator gets overridden and that now
marks the row as a probe. But that is an indirect proxy: if the default
accelerator in `boot-test.sh` ever changes -- which is precisely what Q54 asks
-- boots on either side of the change become indistinguishable, and clean
streaks would span both. Record `accel` there too, from the same banner, when
Q54 is answered either way.

#### Resolution (2026-08-19)

Fixed as designed, script-side, in `scripts/bench-history.py`: a second
anchored pattern `BARE_METAL_RE` for the kernel's other sentence, and a named
constant `ACCEL_BARE_METAL = "bare metal"` — the same string as
`Hypervisor::name()`'s `None` variant (`kernel/src/hypervisor.rs:77`), so the
field's vocabulary stays the kernel's own and the harness invents no token of
its own. `parse_accel()` tries `ACCEL_RE` first and falls back to the second,
so the three platforms now get three answers: a vendor label, `"bare metal"`,
and `None` for a log that predates both banners.

**Three tests, and the third is the one that matters.** The first two check
the parser; on their own they would pass for a parser that gets the *relation*
between the three answers wrong, so both assert the relation directly
(`parse_accel(metal) == parse_accel(old)` is `False`, and likewise against
TCG). The third — `test_a_bare_metal_arm_does_not_band_with_a_pre_banner_one`
— reconstructs the exact case the entry identified as surviving only by luck:
identical source, three arms from real hardware and three from a pre-banner
kernel at the *same* commit. Before the fix those six formed one group. They
now form two. That is the assertion the entry was actually about; the parser
returning a new string is only the means.

**Non-crossing is asserted, not reasoned about.** The two patterns cannot
match each other's line, and the test says so of both directions. This is
belt-and-braces — the literals diverge immediately after `[hypervisor] ` and
the kernel emits exactly one per boot — but the failure it rules out is worse
than the bug being fixed: a loosened `ACCEL_RE` would capture some fragment of
the bare-metal sentence as a *vendor name*, filing the run under an
accelerator that does not exist. `None` at least announced that something was
unknown; a plausible-looking wrong vendor announces nothing. The anchoring is
tested the same way, with a line that merely quotes the banner.

**Mutation-verified, four mutations, all caught:** bare-metal detection
removed entirely, i.e. the original bug (2 failures); `ACCEL_BARE_METAL` set
back to `None` (3); the line anchor dropped so a quoted banner counts (1);
`ACCEL_RE` loosened to swallow the bare-metal line and capture a nonexistent
vendor (2).

**Not retroactive, and it does not need to be.** No row on disk was written by
a bare-metal boot — this machine has never run one — so no existing record
changes meaning and none is rewritten. The fix is entirely forward-looking, by
design: it works on logs as they are *read*, which is why it was preferable to
changing the kernel's output. The day the first bare-metal log is produced, it
parses correctly without anyone remembering this entry existed.

**The `boot-history.jsonl` sibling above is still open** and is deliberately
left so: it is gated on Q54 being answered, since what makes it bite is the
default accelerator *changing*, and recording a field to guard a change nobody
has decided to make yet is guesswork about which change it will be.
