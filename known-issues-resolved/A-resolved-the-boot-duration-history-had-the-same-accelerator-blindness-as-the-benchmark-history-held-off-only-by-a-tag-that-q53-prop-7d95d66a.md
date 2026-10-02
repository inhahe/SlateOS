### [A] RESOLVED — the boot-duration history had the same accelerator blindness as the benchmark history, held off only by a tag that Q53 proposes to stop applying — 2026-08-19

**Status:** RESOLVED 2026-08-19, immediately after the sibling defect above.
Found by asking whether the fix just made to `bench-history.py` had a twin, and
it did: `boot-history.py` grouped boot durations by *build* alone.

**In short.** `bench/boot-history.jsonl` records how long each QEMU boot took,
and prints a median per population so that "this boot was slow" means
something. It already knew that a KASAN-instrumented build costs ~3.4x an
ordinary one and kept those apart. It did **not** know that the *accelerator*
matters: on this host a boot under Hyper-V/WHPX takes ~173 s against ~121 s
under QEMU TCG. Both were about to be called the same population.

**The measured cost of pooling.** 12 timed non-experiment boots of the
uninstrumented build, median 121 s. 17 WHPX boots, median 173 s. Pooled, the
median is 171 s — **41% above** the figure that describes a normal boot, and
four times CLAUDE.md's 10% regression threshold. A median that wrong does not
merely mislead; it makes the number useless in both directions, since a real
40% regression on TCG would land inside the pooled spread and a healthy WHPX
boot would look like one.

**Why it had not bitten, and why the reprieve is expiring.** All 17 WHPX boots
carry an `experiment` tag, and `wall_populations` drops tagged boots outright.
That is the same coincidence that protected the benchmark history — a fact
about how the probes were invoked, not a rule the file applies — except here it
has a *scheduled* expiry: **Q53 is a live proposal to make WHPX the ordinary
way to boot the tree.** The day that is answered "yes", the tag stops appearing
and the 41% shift arrives untagged, with nothing to attribute it to. The old
docstring said the two 2026-08-19 WHPX boots were excluded, and it was true;
what it did not say was that nothing in the code was doing the excluding.

**The fix.** `Serial` gained an `accel` field, `build_record` writes `accel`
unconditionally (`null` included, on the same three-state rule as `sanitizer`:
absent = row predates the field, null = the log did not say, string = it did),
`accel_of` collapses the two ways of not knowing without ever folding either
into a named accelerator, and `population_of(rec)` is the **pair** — the label
a human reads and the key the numbers are grouped by are one function, so a
legend cannot come to name a different partition from the one it describes.
`wall_populations`, `report_wall`, `report` and `cmd_list` all use it; `--list`
gained an accelerator column (`tcg`/`whpx`/`metal`/`?`) beside the sanitizer
one, for the same reason that column exists — a duration that looks wrong is
almost always a row from the other population, and until now only half of that
population was visible.

**The banner is parsed once, in the other file.** `_parse_accel` delegates to
`bench-history.py`'s `parse_accel` rather than carrying a second copy of the
two `[hypervisor]` patterns (`design-decisions.md` §240). A copy would drift
*silently* here, because a pattern that stopped matching returns the same
`None` a pre-banner log does. The delegation is asserted by a test that reads
the expected answer out of `bench-history.ACCEL_BARE_METAL`, not out of a
literal — two parsers that agree today would otherwise satisfy it.

**Failure to load the delegate costs the label, not the boot.** That `try` is
the one place in this file where swallowing an exception is correct:
`boot-test.sh` calls this script from its EXIT trap with `|| true`, so a raise
does not surface anywhere — it silently loses the record of the boot, which for
a *failing* boot is the most expensive outcome the script has. The failure is
printed to stderr and the row is written with `accel: null`. Both halves of
that are under test, including the stderr message.

**Every existing row reads `?`, and that is the honest answer.** All 182 rows
predate the field. `accel_of` therefore reports one unknown population rather
than back-filling the 165 non-experiment boots as TCG — the same reasoning as
§241, and for the same reason it is not merely conservative: the WHPX boots in
this very file predate the banner too, so "no banner" is demonstrably not
evidence of TCG.

**Mutation ledger for this change: 15 mutations, 15 caught.** One survived the
first pass — *"the listing never says a row cannot answer"*, which turned the
`?` accelerator token into `tcg`. It survived because the fixture omitted
`sanitizer`, so the column *beside* the one under test was also printing `?`
and satisfied a check that merely looked for a `?` on the line. The fixture now
names the sanitizer and the assertion reads the accelerator column by position.
That is precisely the failure mode this project keeps rediscovering: a test
that passes for a reason unrelated to the thing it is named after.
