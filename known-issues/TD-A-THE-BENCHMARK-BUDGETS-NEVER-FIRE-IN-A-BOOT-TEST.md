## TD-A-THE-BENCHMARK-BUDGETS-NEVER-FIRE-IN-A-BOOT-TEST (lane A, 2026-09-10) — **the title is false and kept for searchability; the budgets DO fire, and two have moved under deliberate change. Read the state-of-play below, not the chronology.**

> **State of play, 2026-09-11.** This entry is 900 lines of chronology with four
> corrections in it. The corrections are worth reading — each records a conclusion I
> drew from a population I had not inspected — but nobody should have to read 900
> lines to learn where things stand. Here is where they stand:
>
> * **The budgets fire.** `--bench` exists, waits for `BENCH_OK`, and has recorded
>   150 runs. `over_target` went **10 → 9** when `dashboard_api_metrics` came under
>   budget from a deliberate change, which is the thing the title says never happens.
> * **Three of the original ten were never kernel findings**: `net_arp_lookup`,
>   `net_ns_arp_lookup` and `isr_latency` are hypervisor-bound, identified by
>   accelerator ratio (a cost larger under WHPX than under TCG is a VM exit, not work).
> * **Two now meet their budgets** — `dashboard_api_metrics` under WHPX,
>   `dashboard_api_health` under TCG — and `dashboard_api_status` is within 6% on TCG.
> * **The surface must be chosen per benchmark, not per accelerator.**
>   `dashboard_api_status` reads 1.65× over on WHPX and 1.06× on TCG; `vfs_stat_root`
>   reads 1.28× on WHPX and 4.19× on TCG. One policy is wrong for one of them either
>   way.
> * **Two real fixes came out of it.** `sched::task_count` removed a `Vec<TaskInfo>`
>   built to read `.len()` (−55–74% on three endpoints under WHPX, −97% under TCG),
>   and `journal::record` stopped timestamping every file write from the HPET
>   (**42×** on that phase, **−29%** on every file write).
> * **Still open and real:** `vfs_throughput_16k_write` at 2.3× over budget, whose
>   remaining cost is `invalidate_negative_prefix` scanning all 1024 dcache entries
>   per write plus memfs's own write; and `vfs_stat_deep`, localised to procfs
>   traversal rather than path resolution.
> * **Unresolved question for the operator or the lanes:** `QEMU_EXTRA` stamps WHPX
>   runs as experiments, and `comparable_records` excludes experiments from history
>   windows — correctly. A gate wanting a WHPX baseline needs either another way to
>   pass the accelerator or `-accel whpx` recognised as a surface rather than an
>   experiment.
>
> The recurring methodological fault, stated once here so it is not buried: **four
> times I drew a confident conclusion from a population I had not looked at** — one
> boot log, one benchmark series, eight layout-sweep rows read as ordinary runs, and a
> baseline straddling an unrelated change. Each arrived with enough supporting detail
> to look measured. The defence that worked every time was to list the rows and read
> them before computing anything over them.

**In short:** the kernel's micro-benchmarks each carry a cycle budget, so a change that
makes something twice as slow is supposed to fail the build. They do not run during a
boot test — the boot finishes first and the benchmarks are still queued — so the
budgets have never refused anything. A gate that exists and cannot fire reads exactly
like a gate with nothing to report.

### What is true

`kernel/src/bench.rs` scores results against explicit budgets, e.g.
`score("tcp_checksum_v4", &result, 2000)` and `score("tcp_checksum_v6", &result, 2200)`.
The budgets are real, the comparison is real, and the failure path works.

But `bench::run_all` is deferred to a background kernel task so init can start
immediately — a deliberate and correct decision for boot latency. The boot test waits
for its success marker, which arrives first. Measured on the run of 2026-09-10: the
serial log holds **8 `[bench]` lines, all from the bench infrastructure self-test**, and
no occurrence of `tcp_checksum_v4` or `tcp_checksum_v6` at all.

### Why it matters now rather than in the abstract

It was found while adopting `netproto`'s checksum helpers into
`kernel/src/net/checksum.rs`. That module exists *because* duplicated copies of the
data loop got different unrolling decisions and produced a 34% gap that was misread as
a protocol difference — so moving the loop across a crate boundary is a codegen change
of exactly the class the budgets would be expected to catch.

The budget was the reason I believed the migration was safe to try. Checking whether it
would actually fire is what stopped me shipping it unmeasured, and the migration now
rests on `#[inline]` in another crate plus a comment on each side of the boundary. That
is the whole protection, and it is weaker than a number.

### The shape, which is the part worth keeping

This is the third form of one failure seen on 2026-09-10, and they have different
causes and the same outcome:

| | exists | does not run |
|---|---|---|
| a gate | `check-read-defaults.py` | nothing invoked it |
| a refusal | `pre-push`'s `REFUSING` blocks | set a variable nothing read |
| a budget | `tcp_checksum_v4` vs 2000 cycles | the boot ends before it runs |

Lane C hit a fourth on 2026-09-09 (§826 changed four colour constants; only the edited
crate was tested, and a dependent crate's suite sat red on main), and raised it as
`open-questions.md` → C-Q11, *"should something build every crate before a merge?"*
They suggested adding this as a second instance with no lane boundary in it, which is
right: two data points with different causes and the same outcome argue for a general
answer rather than two local fixes.

### Options

* **Run the benchmarks synchronously in the boot test only**, gated on a flag the
  harness sets. Keeps boot latency for real boots, makes the budgets real under test.
  *Cost:* a boot-test-only code path, which is a thing that can itself rot unobserved.
* **Have the boot test wait for a second marker** emitted after `run_all` completes.
  *Cost:* lengthens every run by the benchmark suite's duration.
* **Move the budgets out of the boot path** into a host-side check over recorded
  history, beside `bench/boot-history.jsonl`. *Cost:* needs the numbers to be recorded,
  which they currently are not.

Not chosen yet. Whichever it is, the property to preserve is the one the budgets were
written for: a number that refuses, rather than a number that is printed.

### Correction 2026-09-11 — the title is wrong, and so were all three options

**Everything above is premised on the benchmarks not running under a boot test.
They do, and they have 144 times.** The entry should be read as: *`over_target` is
recorded on every run and has never been a verdict.* That is still a real finding,
and it is a different one with a different fix.

What I got wrong, and how:

| the entry says | measured 2026-09-11 |
|---|---|
| "they do not run during a boot test — the boot finishes first" | `scripts/boot-test.sh` line 1795: `--bench) BENCH=1; WAIT_MARKER="BENCH_OK"`. The flag already exists and already waits. |
| "no occurrence of `tcp_checksum_v4` at all" | true of the run I looked at, which was not `--bench`. **All 144 rows** of `bench/history.jsonl` carry `tcp_checksum_v4`. |
| option: "have the boot test wait for a second marker" | implemented; one flag. |
| option: "move the budgets … beside `bench/boot-history.jsonl`. *Cost:* needs the numbers to be recorded, which they currently are not" | they are recorded — 144 rows × 63–99 entries, plus a per-run `over_target` count. |

I measured one boot log, found the benchmarks absent, and wrote down the first
explanation that fit — never checking whether the obstacle I had named was the
one in force. It was not even present. The same mistake as this morning's request
triage: diagnosing from the first blocker found, and never asking whether
removing it would be sufficient.

### What is actually true, and why nobody made the budget a verdict

`over_target` runs **9 to 21 per boot**, on every run, and nothing has ever
refused because of it. The reason is good: across all runs the numbers are not
comparable. `tcp_checksum_v4` carries a 2000 ns budget and the recorded series
includes 1840, 1944, 2012, 2633, 3241 and 3835 — **the same code spanning 2×**. A
gate on that refuses at random, which is worse than one that never refuses,
because it teaches everyone to re-run until green and that habit disarms the gate
for every genuine regression too.

**But the spread is not noise, and the tree already records the variable that
explains it.** Restricted to `run_verdict == "clean"`, the same benchmark reads

```
1712 1717 1676 1713 1676 1676 1717 1700 1717 1672 1716 1875 1711 1719 1716 …
```

— 37 clean runs inside ±2%. Then 8 runs at **246–253**, then back to ~1950. That
is not a 7× optimisation and a 7× regression; it is `accel`:

| runs | `accel` | `tcp_checksum_v4` | `over_target` |
|---|---|---|---|
| 37 | (unrecorded, TCG-era) | ~1716 | 12–15 |
| 8 | `Hyper-V/WHPX` | ~249 | 9–10 |
| 7 | `QEMU TCG` | ~1950 | 13–18 |

A cycle budget is meaningful **per accelerator**, and the row already says which
one ran. `bench-history.py` even has `cmd_accel_compare` for exactly this.

### So the fix is composition, not construction

Every ingredient exists: `--bench`, `run_verdict`, `dispersion`, `accel`,
`cmd_accel_compare`, and 144 rows of history. The missing step is the one that
turns them into a refusal:

* judge **only** runs where `run_verdict == "clean"` — on a contaminated run,
  emit **no verdict** rather than a pass, since "could not measure" is not
  "did not regress" (the same rule this file closed sixteen call sites over today);
* compare each benchmark against the median of recent clean runs **at the same
  `accel`**, not against the static budget;
* ratchet `over_target` per `accel`, since its clean-run range is tight (12–15
  under TCG, 9–10 under WHPX) and a jump is a real signal.

Not built yet, and this time the reason is stated rather than assumed: it needs a
decision about what happens on a host where no clean run at that `accel` exists
yet — refuse to judge, or fall back to the static budget — and that choice changes
whether a fresh machine can ever push. Recorded as the next step rather than
guessed at, because guessing at the mechanism is what produced the three wrong
options above.

### Second correction, same day — the design above is viable only on hardware acceleration

The correction above says to "compare each benchmark against the median of recent
clean runs at the same `accel`". I wrote that after reading **one** benchmark's
series — `tcp_checksum_v4`, which is stable to ±2% on clean runs — and treating it
as the population. It is not. Measured across all 86 benchmarks with ≥4 clean
samples at a fixed `accel`, worst-sample ÷ median is:

| `accel` | clean rows | 50th | 90th | 99th |
|---|---|---|---|---|
| `Hyper-V/WHPX` | 8 | **1.03×** | 1.23× | 1.53× |
| `QEMU TCG` | 9 | 1.29× | 2.13× | 2.78× |
| (unrecorded, TCG-era) | 36 | 1.58× | 2.64× | 3.33× |

So on a *clean* run at a *fixed* accelerator, the median benchmark's worst sample
is still **1.3–1.6× its own median under emulation**, and ~1.03× under hardware
virtualisation. Two consequences:

* **`CLAUDE.md`'s "investigate any regression over 10%" is inside the emulator's
  noise by a factor of three to six.** It is a sound rule that cannot be applied
  to a TCG measurement at all.
* **A "nothing may double" gate is quiet under WHPX and unusable under TCG.** 2× sits
  above the 99th percentile (1.53×) on WHPX, so it would fire only on something
  real; under TCG it sits near the 88th percentile, which is roughly ten false
  alarms per run across 86 benchmarks.

### Which reframes the whole item

This is not "wire up a gate that was left unwired". It is: **whether a performance
regression gate is possible at all is decided by which accelerator the boot test
runs under**, and the default is TCG (`QEMU_EXTRA="-accel tcg,…"`; the eight WHPX
rows come from a sweep on 2026-08-19). Under TCG no threshold separates a real 2×
regression from ordinary run-to-run spread, so there is nothing to wire.

The actionable form is therefore a prerequisite, not an implementation: if the
budgets are to refuse, benchmark runs need to happen under WHPX, and then the gate
is ~30 lines over data that already exists. Until then `over_target` is the right
thing to record and the wrong thing to gate on, which is what the tree already
does — and `print_scorecard`'s own comment, "labelled as reference rather than as a
verdict", turns out to have been exactly right for a reason it does not state.

### The method note, because it is the third instance today

One boot log gave me the wrong cause. One benchmark series gave me the wrong
stability. Both times the sample was real, the reasoning from it was sound, and the
population behaved differently. The error is not carelessness about the sample; it
is treating *n*=1 as a measurement instead of as a hypothesis — and the cost is
that each wrong conclusion arrived with enough supporting detail to look measured.

### Third correction, same day — the WHPX evidence is one layout sweep, so the prerequisite is smaller than stated

The correction above concludes that a regression gate is *possible under hardware
acceleration and not under TCG*, and makes "run the benchmarks under WHPX" the
prerequisite. The WHPX half of that rests on eight rows, and I did not check what
the eight rows were.

**Every Hyper-V/WHPX row in `bench/history.jsonl` — all fourteen — is an arm of a
single layout sweep**, carrying `experiment: "layout sweep: textpad=N (identical
source, deliberately perturbed…)"`. Five of the eight clean ones share one commit
(`334124dbf`). **There are zero ordinary WHPX runs on record.**

So `1.03×` is the within-sitting spread of one build under deliberate perturbation
— which is a genuine and interesting number, answering the question that sweep
asked (*does code layout move these?* yes, by ~3%) — and it is **not** run-to-run
spread across days, reboots and host states. It is the smaller quantity, and it
flatters the conclusion I drew from it.

**What this changes, and it is mostly good news.** The prerequisite is no longer
"enable WHPX" — this host already runs it, 14 times, and the knob is
`QEMU_EXTRA="-accel whpx" ./scripts/boot-test.sh --bench`. The prerequisite is
**a handful of `--bench` runs under WHPX on unperturbed source**, which is hours of
wall-clock and no decision at all. Only after that is there a basis for saying
whether a 2× gate would be quiet there.

Note also that `QEMU_EXTRA` auto-stamps a run as an experiment
(`boot-test.sh:2769`), so such runs will be marked — deliberately, and correctly,
since non-default emulator flags are exactly what a reader needs warned about. The
baseline a gate uses would have to decide whether an `-accel whpx` experiment row
counts as ordinary. That is the one real design question left in this entry.

*(Fourth correction to the same entry in one day. The first three each replaced a
conclusion drawn from a population I had not inspected: one boot log, one benchmark
series, and now eight rows whose `experiment` field was sitting in the same JSON
object I was reading `accel` out of.)*

### 2026-09-11 — the first unperturbed WHPX run, and it settles the prerequisite question

`QEMU_EXTRA="-accel whpx" ./scripts/boot-test.sh --bench` **works, passes, and
records a full row** — 99 entries, `accel: Hyper-V/WHPX`, `profile: release`,
`BENCH_OK` reached, commit `b770d800f`. It is the first WHPX row in
`bench/history.jsonl` that is not a layout-sweep arm. Two facts about the row before
the interesting part: `QEMU_EXTRA` auto-stamps it `experiment: "QEMU_EXTRA=-accel
whpx (non-default emulator flags)"` (`boot-test.sh:2769`), and its `run_verdict` came
back `unknown` rather than `clean`.

**The 2026-08-19 characterisation in `comparable_records` holds, almost exactly.**
Against the median of 32 unperturbed release TCG rows, over the 99 benchmarks both
have:

| | documented 2026-08-19 | measured 2026-09-11 |
|---|---|---|
| median benchmark | ~3.5× faster | **4.20× faster** |
| best | ~10× | **10.8×** (`io_ring_nop`, 75.5 ns → 7 ns) |
| device-bound | ~30× *slower* | **33× slower** (`hpet_read`, 448 ns → 13 693 ns) |

### The part that decides the gate question: three benchmarks collapse into one number

Exactly three benchmarks land in the 12–16 µs band under WHPX, and they agree to
within 0.8%:

| | WHPX | TCG median |
|---|---|---|
| `net_ns_arp_lookup` | 13 578 | 803 |
| `net_arp_lookup` | 13 585 | 901 |
| `hpet_read` | 13 693 | **448** |

Under TCG these three span 2× **and rank in the opposite order** — `hpet_read` is the
fastest of the three there and the slowest here. Under WHPX they are one number,
because one HPET read costs a VM exit and that exit is ~13.6 µs. So the two ARP
benchmarks under WHPX **are not measuring ARP**; they are measuring a VM exit with
some ARP work lost inside the error bars. The ~450 ns of real signal that separates
`hpet_read` from `net_arp_lookup` under TCG is 3% of the WHPX number.

### What follows, and it is not what the previous correction said

That correction made the prerequisite *"a handful of `--bench` runs under WHPX on
unperturbed source"*, implying WHPX is the better measurement surface and the gate
follows once rows exist. **One run is enough to show the prerequisite was the wrong
shape**: the answer is per-benchmark, not per-accelerator.

* **Compute-bound benchmarks:** WHPX is 4–11× faster and a plausible gating surface.
  Whether it is *tighter* run-to-run is still unmeasured — this is one row, and the
  1.03× figure from the layout sweep measures something else (see the correction
  above).
* **Device- or timer-bound benchmarks:** WHPX is the wrong surface at any sample
  size. A gate there would grade Hyper-V's exit latency, and a 4% move in it would
  swallow the entire quantity the benchmark exists to measure. These want TCG, where
  the HPET is emulated inline and the three benchmarks separate.

So a gate cannot pick an accelerator; it has to pick one **per benchmark**, and the
tree has no field recording which class a benchmark is in. Deriving it is cheap and
now has a method — a benchmark whose WHPX/TCG ratio sits near the VM-exit floor
instead of near the median is device-bound — but it is a real addition rather than a
configuration change, which is what the previous two corrections each assumed it
would be.

**`over_target` on this run: 10.** Against the 13–21 range of TCG runs, which is the
shape one expects if the budgets are hardware-derived and WHPX is closer to hardware.
One row cannot carry that conclusion and it is recorded as a number, not a finding.

### 2026-09-11 — three unperturbed WHPX runs, and the budgets can be a verdict after all

**`over_target` is 10. Three times out of three.** Commits `b770d800f`, `1ec6842eb`,
`d7ebfa9b7`, each a separate `QEMU_EXTRA="-accel whpx" ./scripts/boot-test.sh --bench`.
Against 13–21 under TCG, that is the difference between a number and a verdict.

And the run-to-run spread is stable across both available pairs, so the figure is not
a one-pair fluke:

| | n | median | p90 | worst |
|---|---|---|---|---|
| pair 1→2 | 99 | 1.022× | 1.060× | 1.33× |
| pair 2→3 | 99 | 1.017× | 1.051× | 1.43× |
| pooled | 198 | **1.020×** | 1.052× | 1.33× (p99) |

### The ten, named — and two of them are measuring Hyper-V

| benchmark | measured | budget | over |
|---|---|---|---|
| `net_ns_arp_lookup` | 13 984 | 1 000 | **14.0×** ⚠ |
| `net_arp_lookup` | 13 571 | 1 000 | **13.6×** ⚠ |
| `dashboard_api_status` | 59 068 | 10 000 | 5.9× |
| `vfs_stat_deep` | 6 739 | 1 400 | 4.8× |
| `dashboard_api_health` | 59 135 | 15 000 | 3.9× |
| `isr_latency` | 34 796 | 10 000 | 3.5× |
| `vfs_throughput_16k_write` | 129 151 | 50 000 | 2.6× |
| `dashboard_api_metrics` | 77 623 | 55 000 | 1.4× |
| `vfs_stat_root` | 893 | 700 | 1.28× |
| `vfs_stat_3comp` | 2 200 | 2 100 | 1.05× |

⚠ **The two ARP rows are artifacts, and this is the concrete cost of the VM-exit
floor.** Under TCG they measure **901 ns and 803 ns against a 1 000 ns budget — both
inside it.** They breach it under WHPX only because one HPET read costs a VM exit
there, which is ~13.6 µs. So 2 of the 10 "failures" are grading Hyper-V, and a gate
ratcheted at 10 would be pinning two numbers that say nothing about this kernel.

**The other eight are real**, and they are hardware-derived budgets missed on the
closest surface to hardware available here, so they are not emulation excuses. The
`dashboard_api_*` trio at 1.4–5.9× and `vfs_stat_deep` at 4.8× are the largest.

### So the entry's original question has an answer

*Can the budgets refuse?* **Yes, under WHPX, at a ratchet of 8** — with the two
timer-bound benchmarks excluded rather than pinned, because a ratchet including them
would hold a constant that belongs to the hypervisor. Under TCG the answer remains no:
`over_target` swings 13–21 run to run, and the median benchmark moves 9.9%.

What is still needed, and it is small:

1. **A per-benchmark surface field.** Nothing records which benchmarks are
   device/timer-bound. The derivation is now mechanical — a benchmark whose WHPX/TCG
   ratio sits at the VM-exit floor rather than near the 4.2× median is timer-bound —
   and it identifies exactly three today (`hpet_read`, `net_arp_lookup`,
   `net_ns_arp_lookup`).
2. **A decision about the `experiment` tag.** `QEMU_EXTRA` auto-stamps these runs
   (`boot-test.sh:2769`), and `comparable_records` excludes experiments from history
   windows — correctly, since non-default emulator flags are exactly what a reader
   needs warned about. A gate wanting a WHPX baseline has to either pass the
   accelerator some other way or treat `-accel whpx` as a recognised surface rather
   than an experiment. That is the one genuine design choice left, and it is the kind
   that should be asked rather than assumed.
3. **`run_verdict` came back `unknown` on all three runs**, not `clean`, so any gate
   keying on `clean` would skip every WHPX run today. Worth understanding before
   building on it.

Nothing here is built. What changed is that the question stopped being "is this
possible" and became three specific small things, each with a stated reason.

### One of the eight localises: `vfs_stat_deep`'s overrun is procfs, not path resolution

Two of the over-budget benchmarks measure path resolution, and comparing them settles
where the cost is:

| benchmark | path | components | budget | WHPX | vs budget |
|---|---|---|---|---|---|
| `vfs_stat_3comp` | regular filesystem | 3 | 2100 (3 × 700) | 2200 | **1.05×** |
| `vfs_stat_deep` | `/proc/meminfo` | 2 | 1400 (2 × 700) | 6739 | **4.8×** |

A **three**-component path on a regular filesystem lands within 5% of its budget. A
**two**-component path into procfs is 4.8× over. So the component count is not the
variable and general path resolution is not the problem — the benchmark's own doc
comment says what is: resolving `/proc/meminfo` *"traverses the VFS mount table,
descends into the procfs mount, and does a final filename lookup."*

Per-component arithmetic makes the gap concrete: ~733 ns/component on a regular path
against the 700 ns target, versus ~2 923 ns/component once procfs is involved —
4× the cost for the same nominal work.

**So `vfs_stat_deep`'s budget models the wrong thing.** `2 components × 700 ns` is a
correct derivation for a 2-component path and this is not really a 2-component-path
benchmark; it is a mount-crossing-into-a-synthetic-filesystem benchmark that happens
to have two components. The name says so — it is registered as
`vfs_stat_deep_2comp` — and the "deep" framing is what makes the budget look like a
per-component question.

Two things follow, and they want different people:

* **The measurement is sound and the gap is real**, just differently located: procfs
  stat costs about 4× a regular component. Whether that is acceptable for a synthetic
  filesystem that formats its contents on read is a question for whoever owns procfs
  performance, with a number to start from rather than a suspicion.
* **The budget should be restated against what it measures.** Either give it a
  procfs-aware target with the mount crossing priced in, or add a regular-filesystem
  2-component benchmark beside it so the comparison that localised this is in the
  suite rather than in this entry. The second is better: it turns a one-off analysis
  into a standing control, which is the difference between knowing this today and
  knowing it after the next change.

Worth noting how close this came to being recorded as something else. "`vfs_stat_deep`
is 4.8× over budget" invites a reading of path resolution being slow, and
`vfs_stat_3comp` — sitting four lines away in the same scorecard, on target with *more*
components — is the only thing that contradicts it. The two numbers are only useful
together.

### The loop closes: a budget went from failing to passing, and `over_target` moved

`sched::task_count` replaced five `task_list().len()` calls. Measured on the fourth
unperturbed WHPX run (`c1c9352e6`) against the median of the three before it:

| benchmark | before | after | change | budget | then → now |
|---|---|---|---|---|---|
| `dashboard_api_status` | 59 068 | **16 549** | **−72.0%** | 10 000 | 5.91× over → 1.65× over |
| `dashboard_api_health` | 59 135 | **15 561** | **−73.7%** | 15 000 | 3.94× over → 1.04× over |
| `dashboard_api_metrics` | 77 623 | **34 831** | **−55.1%** | 55 000 | 1.41× over → **PASS** |
| `vfs_stat_root` (control) | 893 | 896 | +0.3% | 700 | unchanged |

**`over_target` went 10 → 9.** That is the first time in this entry's history that a
budget has moved from failing to passing because of a deliberate change — the thing
the entry opened by saying the budgets had never done.

Three reasons to believe it rather than just like it: the changes are 10–14× the
measured noise floor (p90 of run-to-run movement is 1.052×, so ~5% is the threshold of
meaning); `vfs_stat_root` sat still at +0.3%, so this is not a global shift in the
run; and all three movers are exactly the three endpoints that called
`task_list().len()`, while the control did not.

### And the prediction made before the change held

The commit that made this change said plainly what it would *not* achieve: that
`dashboard_api_status` could not meet its 10 µs budget under WHPX whatever happened,
because `api_status` reads the HPET once and one HPET read costs ~13.5 µs on that
accelerator — more than the entire budget.

After the change: 16 549 ns measured, of which `hpet_read` is 13 484 ns. **81% of
what remains is the timer read.** The actual work is now about 3 065 ns against a
10 000 ns budget — comfortably inside it — and the benchmark still reports OVER,
because the surface charges 13.5 µs for a clock.

So `dashboard_api_status` has **changed class**. It was compute-bound (6.5× faster
under WHPX than TCG) and is now timer-dominated, which makes WHPX the wrong surface
for it and TCG the right one. That is the per-benchmark-surface point from earlier in
this entry arriving as a concrete instance rather than an argument: a benchmark's
class is not a fixed property, and optimising the code is one of the things that
changes it.

The practical consequence is small and specific: measure the two remaining OVER
endpoints under TCG before concluding anything further about them, since under WHPX
their budgets are now mostly measuring Hyper-V.

### What the cost actually was

Five callers wanted a count and the only available route built a `Vec<TaskInfo>` —
one allocation plus roughly 70 bytes copied per task, under the scheduler lock —
then discarded it. Removing that was worth 55–74% of three endpoints' total cost,
which says the allocation and copy dominated them. It also shortens the window the
scheduler lock is held on every procfs and dashboard read, which no benchmark here
measures and which matters more on a contended machine than the numbers above.

### A trap for whoever picks up the two endpoints still marked OVER

`dashboard_api_status` reports 16 549 ns against a 10 000 ns budget, and **13 484 ns
of that is one HPET read**. The obvious way to make it pass is to stop reading the
HPET: the endpoint only needs `uptime_secs`, `apic::tick_count()` is an atomic load
at 100 Hz, and `uptime_ns` could be derived from it at 10 ms resolution. That change
would remove 81% of the measured cost and turn the verdict green.

**It would also be the wrong change, and the reasoning is worth stating because the
number is so persuasive.**

The 13.5 µs is not what an HPET read costs. It is what a *VM exit* costs under
Hyper-V/WHPX. Under TCG the same read is **448 ns**, because the HPET is emulated
inline there, and on real hardware an MMIO timer read is in that neighbourhood rather
than in microseconds. The 10 000 ns budget was set for hardware. So on the platform
the budget describes, `now_ns()` is already cheap and there is nothing to fix.

Dropping nanosecond uptime to satisfy that benchmark would be **degrading the product
to satisfy the test surface** — trading real reported precision for a number that is
only red on one accelerator. It is the same error as tuning against an emulator's
instruction timings, arriving through a more respectable-looking door: a failing
budget, a named cost, and a clean fix.

The tell that distinguishes this case from a real finding is available and cheap:
**compare the two accelerators.** A cost that is 30× larger under hardware
virtualisation than under emulation is a VM exit, not work —
`comparable_records`' docstring says so, and `hpet_read` measures exactly that
(448 ns TCG, 13 484 ns WHPX, 0.03×). Real work goes the other way: the median
benchmark is 4.2× *faster* under WHPX.

So the correct next step for these two endpoints is the one already recorded —
**measure them under TCG**, where the timer is not a VM exit and the budget is being
compared against something like the quantity it was written for. Not to optimise the
clock away.

*(Noting this rather than acting on it because the change is four lines and looks
like a win from every angle except the one that matters. The allocation removed in
`task_count` was a real defect on every surface — 55–74% on three endpoints, with an
unchanged control — and this is the opposite: an artifact that would cost real
precision to silence.)*

### Classifying the ten by accelerator ratio, and a 42× read/write asymmetry

The cheap test established earlier — a cost larger under hardware virtualisation than
under emulation is the hypervisor, not work — applied to the over-budget set and its
neighbours. Ratio is TCG median ÷ WHPX median, so **> 1 means WHPX is faster**:

| benchmark | WHPX | TCG | ratio | class |
|---|---|---|---|---|
| `io_ring_nop` | 7 | 76 | 10.79× | compute-bound |
| `vfs_throughput_16k_read` | 3 079 | 26 355 | 8.56× | compute-bound |
| `vfs_stat_root` | 894 | 3 769 | 4.21× | compute-bound |
| `vfs_throughput_16k_write` | 128 973 | 486 212 | 3.77× | compute-bound |
| `isr_latency` | 42 264 | 45 197 | **1.07×** | **hypervisor-bound** |
| `hpet_read` | 13 476 | 448 | **0.03×** | **hypervisor-bound** |

**`isr_latency` is a third artifact.** At 1.07× it barely moves between the two
accelerators, which is the signature of a cost the host pays rather than the kernel —
unsurprising for a measurement of real interrupt delivery. Its 3.5×-over-budget
verdict is therefore not a kernel finding on either surface available here, and it
joins `net_arp_lookup` and `net_ns_arp_lookup`. That leaves the "eight real gaps" at
**five**.

**`vfs_throughput_16k_write` is NOT an artifact**, which was my guess and was wrong.
At 3.77× it sits beside the 4.2× median, so it is genuine kernel work and worth
optimising. Recorded because the guess was reasonable — a file write under
virtualisation *could* be dominated by device emulation — and one ratio settled it in
a second.

### The asymmetry, which is the real lead

The same function benchmarks both directions on the same 16 KiB file:

| | WHPX | throughput |
|---|---|---|
| `vfs_throughput_16k_read` | 3 079 ns | ~5.3 GB/s |
| `vfs_throughput_16k_write` | 128 973 ns | ~127 MB/s |

**Writes cost 42× reads for identical data on an identical path.** A read at 5.3 GB/s
is plainly served from memory; a write at 127 MB/s is not. So the write path does
something per call the read path does not — re-allocating or extending the file,
journalling, or not caching at all. `Vfs::write_file` is called with the whole 16 KiB
in one go, so it is not chunking overhead.

That asymmetry is a better starting point than the budget. The budget says 2.6× over
50 000 ns; the asymmetry says the write path costs 42× the read path for the same
bytes, which is the kind of gap that usually has one cause rather than a diffuse
2.6%-here-and-there.

Two incidental defects noticed in the same function and not fixed, since they are
documentation rather than behaviour:

* Its doc says *"Benchmark VFS sequential write throughput (4 KiB chunks)"* and the
  code writes 16 KiB **in a single call**. The "(4 KiB chunks)" describes a benchmark
  this is not.
* It runs as `vfs_write_16k` and scores as `vfs_throughput_16k_write` — another
  instance of the documented name-divergence that `MEASUREMENTS` exists to track.

### Measured on TCG, where these budgets mean something: two of the three now meet them

The recorded next step was to measure the remaining OVER endpoints under TCG, because
under WHPX their budgets are mostly comparing against a 13.5 µs VM exit. Done — one
unperturbed release TCG run at `cf2ec96c1`, against the median of the 32 pre-change
TCG rows:

| benchmark | pre-change | after | change | budget | verdict |
|---|---|---|---|---|---|
| `dashboard_api_status` | 384 112 | **10 551** | **−97.3%** | 10 000 | 1.06× over |
| `dashboard_api_health` | 382 458 | **6 192** | **−98.4%** | 15 000 | **PASS** |
| `dashboard_api_metrics` | 466 312 | **90 709** | **−80.5%** | 55 000 | 1.65× over |

So `sched::task_count` took `dashboard_api_health` under its budget and
`dashboard_api_status` to within 6% of it. The improvement is larger here than the
55–74% measured under WHPX, which makes sense: the cost removed was a heap allocation
plus a per-task copy, and both are more expensive under emulation than under hardware
virtualisation.

**`over_target` on this run: 13**, the lowest of the last six TCG rows
(20, 15, 17, 18, 14, 13). Suggestive rather than conclusive on its own, since TCG's
`over_target` has always swung 13–21; it is worth reading only beside the two
endpoints that demonstrably crossed.

### And the surface matters per benchmark, demonstrated twice over

| benchmark | WHPX verdict | TCG verdict | better surface |
|---|---|---|---|
| `dashboard_api_status` | 1.65× over | **1.06× over** | TCG — WHPX charges 13.5 µs for the clock |
| `vfs_stat_root` | **1.28× over** | 4.19× over | WHPX — TCG inflates compute 4.2× |

The same change, the same budgets, opposite conclusions about which surface to trust —
and the reason is legible in each case rather than a matter of preference.
`dashboard_api_status` reads a timer, so WHPX overstates it; `vfs_stat_root` is pure
compute at a 4.21× accelerator ratio, so TCG overstates it. **A single
"run the benchmarks under X" policy would be wrong for one of these two no matter which
X is chosen**, which is the per-benchmark-surface argument arriving for the third time,
now with a table instead of an assertion.

### What the ten have become

Of the ten over budget when this started:

* **3 are hypervisor artifacts** — `net_arp_lookup`, `net_ns_arp_lookup`, `isr_latency`
  — identified by accelerator ratio and not kernel findings on either surface here.
* **2 now meet their budgets** — `dashboard_api_metrics` under WHPX,
  `dashboard_api_health` under TCG.
* **1 is within 6%** — `dashboard_api_status` on TCG.
* **1 is localised** — `vfs_stat_deep`, where the cost is procfs traversal rather than
  path resolution, proven by `vfs_stat_3comp` meeting its budget with *more*
  components.
* **3 remain open and real**: `vfs_throughput_16k_write` (the 42× write/read
  asymmetry), `vfs_stat_root` (4.19× on TCG, 1.28× on WHPX), `dashboard_api_metrics`
  (1.65× on TCG).

A number this entry opened by calling unfirable has now moved twice under deliberate
change, in both directions on both accelerators, and the remaining list is three
specific things rather than ten.

### Correction: the 42× write/read gap is a cache hit against a write-through, by design

The note above presents `vfs_throughput_16k_write` at 128 973 ns against
`vfs_throughput_16k_read` at 3 079 ns as *"42× reads for identical data on an identical
path"*, and concludes *"the write path does something per call the read path does
not."* **The paths are not identical and the conclusion does not follow.**

`Vfs::read_file_routed` serves a stable-identity regular file **from the shared page
cache** — `design-decisions.md` §38, stated in its own doc comment: *"served from the
page cache, sharing one copy with `mmap` and byte-range `read(2)`."* The benchmark
reads the same file 100 times, so every iteration after the first is a warm cache hit,
which is exactly what 5.3 GB/s means. The write path invalidates that cache and writes
through to the filesystem. Comparing them measures the cache, not the write path.

So there is no asymmetry to explain. A 42× gap between a memory read and a filesystem
write is the architecture working.

**What I actually checked before getting there, and it is worth keeping**, because the
suspicious line is still suspicious-looking to the next reader:
`write_file_resolved` calls `history::try_auto_record(path)` before every write,
documented as *"save the old content before overwriting … record_version reads the
file through VFS internally."* A full read on every write would be a real defect. It
is not one: `try_auto_record` opens with `if !is_auto_version_enabled() { return; }`,
so with auto-versioning off it costs an atomic load.

### What survives as a genuine question

`vfs_throughput_16k_write` is **2.6× over its own budget** — 128 973 ns measured
against 50 000 ns, or 127 MB/s against 327 MB/s. That stands on its own without any
reference to the read side.

And the accelerator ratio says what kind of cost it is: **3.77×**, beside the 4.2×
median, so it scales with CPU emulation and is therefore kernel work rather than device
I/O. If 127 MB/s were the emulated disk's ceiling the ratio would sit near 1.0, the way
`isr_latency`'s does at 1.07×. So the question is a real one about the write path's CPU
cost, and it is not answered by "the virtual disk is slow".

### The lesson, which is the same one as the budgets

I reached for the most striking comparison available — 42× — and it was striking
because it spanned a cache boundary. The ratio that was actually diagnostic was the
dull one already in hand: 3.77× versus 1.07× tells you whether a cost is the kernel's
or the host's, and it would have told me nothing about reads at all.

### `write_file` carries ~43 µs of fixed cost on an in-memory filesystem

`/` is **memfs** — the mount table says so at boot: `[vfs] Mounted memfs filesystem at
'/' (rw)`. So the benchmark's writes never touch a device, and the data copy is a
`Vec::clear()` followed by `extend_from_slice` on a buffer that keeps its capacity.
That makes the measured cost hard to explain, and two benchmarks calling the *identical*
operation separate the fixed part from the per-byte part:

| benchmark | bytes | WHPX | ns/byte |
|---|---|---|---|
| `vfs_write_256` | 256 | 43 744 | 170.9 |
| `vfs_throughput_16k_write` | 16 384 | 128 973 | 7.9 |

Both are `Vfs::write_file(path, &data)` on memfs at root, differing only in size — so
the comparison is sound in the way the read/write one was not. **64× the data costs
2.95× the time**, which decomposes to:

* **fixed cost ≈ 43.7 µs per call**
* marginal ≈ **5.28 µs per KiB** (≈5.3 ns/byte)

### The fixed cost localised against `stat`, which walks the same path

`Vfs::stat` on the same filesystem resolves a path, takes the mount, locks the fs and
reads metadata — for **894 ns** (`vfs_stat_root`, WHPX). `Vfs::write_file` does all of
that and then the write-specific steps, for **43 744 ns**. So roughly **43 µs sits in
the steps `write_file_resolved` adds on top of a resolve**, of which there are six:

    check_path_access(Write) · check_writable · intercept::pre_write
    enforce_quota_write · history::try_auto_record · memfs mutation
    cache_identity + invalidate_identity · quota charge

At ~7 µs apiece if spread evenly, though it is far likelier that one dominates.

**Two candidates eliminated, cheaply, so the next person does not re-check them:**

* **Auto-versioning is not it.** `history::try_auto_record` is documented as reading
  the old contents through the VFS before every write, which would be a genuine
  defect, but it opens with `if !is_auto_version_enabled() { return; }` — an atomic
  load when versioning is off.
* **Page-cache invalidation is not it.** `invalidate_identity` → `invalidate_file`
  uses `cache.range(lo..=hi)` on a `BTreeMap`, so it is O(log n + k) in the pages
  belonging to *that file* — one page for a 16 KiB file — not a scan of the cache. It
  also returns immediately when `ino == 0` or the cache is unpopulated.

### Why the read benchmark cannot localise this

`vfs_read_256` is 2 065 ns against `vfs_write_256`'s 43 744 — a 21× gap on the same
file at the same size — and it is the same cache-boundary artifact corrected above:
reads are served from the shared page cache (design-decisions §38) and so skip the
VFS→memfs path that the write must take. `stat` is the right comparison precisely
because it *does* take that path.

### The next step, stated so it is not guessed at

A phase breakdown, which this suite already has an idiom for —
`bench_vfs_readdir_breakdown` — plus the `#[inline(never)] pub fn bench_*` convention
used by `dashboard::bench_api_status` to expose an internal for measurement. Timing the
six steps individually is the only thing that will name the dominant one, and
43 µs on an in-memory write is worth naming: every file write in the kernel pays it,
not just this benchmark.

### Five candidates eliminated by reading, and the two-clocks fact that explains the dashboard

Narrowing the ~43 µs fixed cost of `Vfs::write_file` by reading rather than measuring.
None of these is the cause, and recording that is most of the value — each one looks
plausible enough to cost someone an hour:

| candidate | why it is not the cost |
|---|---|
| the data copy | 64× the data costs 2.95× the time, so the bulk is fixed, not per-byte |
| `history::try_auto_record` | documented as reading the old file before every write — but opens with `if !is_auto_version_enabled() { return; }` |
| `invalidate_identity` | `BTreeMap::range(lo..=hi)` over *that file's* pages, k=1 here; returns early when `ino == 0` or cache unpopulated |
| `memfs::child_ino` | `children.get(name)` — a map lookup, not the linear scan the name suggests |
| `touch_modified` | reaches the clock, but the **cheap** one — see below |

### The two clocks, which differ by four orders of magnitude under WHPX

This is worth knowing independently of the write path, because the choice is invisible
at the call site:

| call | resolves to | WHPX cost |
|---|---|---|
| `hrtimer::now_ns()` | `hpet::elapsed_ns()` when HPET is available — an **MMIO read**, i.e. a VM exit | **~13 500 ns** |
| `timekeeping::clock_realtime()` → `clock_monotonic()` | `bench::rdtsc()` — a register read | tens of cycles |

`memfs`'s `touch_modified` → `vfs::metadata_now_ns` → `clock_realtime`, so **every file
write stamps its mtime from the TSC and pays nothing for it.** `net::dashboard`'s
`api_status` calls `hrtimer::now_ns()` and pays 13.5 µs — which is the whole of the
finding recorded earlier in this entry, now with its mechanism named: not "the dashboard
reads a clock" but "the dashboard reads the *other* clock".

Nothing here argues for changing either call. `hrtimer::now_ns()` prefers the HPET for
good reasons — it is the calibration-independent source, and the TSC path is a fallback
that needs `bench::calibrate_tsc()` to have run. The useful output is that **"reads a
clock" is not a cost estimate in this kernel**, and a reviewer seeing a timestamp in a
hot path has to look at which one.

### What is left, and why it needs measurement rather than more reading

`intercept::pre_write`, `enforce_quota_write`, the two `check_writable`s (one in
`ipc::namespace`, one in `fs::vfs`), `resolve_write_path`, `walk`, `cache_identity`, and
the quota charge. Reading has taken this as far as it goes: each of those either
early-returns or does a small lookup when read in isolation, and yet together they cost
~43 µs more than `stat` does over the same resolve.

That is exactly the shape a phase breakdown settles and reading does not, and the
suite's idiom for it is `bench_vfs_readdir_breakdown` plus the `#[inline(never)] pub fn
bench_*` convention. **One caution for whoever builds it:** timing a *copy* of
`write_file_resolved`'s body would measure the copy, not the path — the same
two-lists-that-must-agree failure this file records in several other places. Instrument
the real function or expose it, rather than reimplementing it beside a stopwatch.

### The tail, named — and one 256-byte write resolves its path about five times

`bench_vfs_write_breakdown` measured the six tail operations of
`Vfs::write_file_resolved` directly. Of a 43 813 ns write under WHPX:

| operation | ns | share |
|---|---|---|
| **`journal::record`** | **14 206** | **32%** |
| `index::on_file_changed` | 3 997 | 9% |
| `quota::charge_bytes` | 123 | 0.3% |
| `notify::emit_modified` | 34 | 0.1% |
| `audit::log_ok` | 22 | 0.1% |
| named tail | 18 382 | 42% |
| remainder — `invalidate_negative_prefix` + memfs's own write | 25 970 | 59% |
| resolve, access, intercept | 479 | 1.1% |

`journal::record` is **one HPET read** — fixed, see the commit. What the rest shows is
structural rather than a single hot line.

### Roughly five path resolutions, for one write

Following the same 256-byte write through:

1. `Vfs::write_file` → `resolve_follow(path)` — resolution #1.
2. `memfs::write_file` → `resolve_write_path(path)` — follows symlinks again to find the
   write target — #2 — then `walk(&parent_path)` — #3 — then `child_ino`.
3. `cache_identity` → `fs.metadata(relative)` — #4, to learn the inode the write just
   touched.
4. `index::on_file_changed` → `add_entry(path)` → `Vfs::stat(path)` — #5, a *full* VFS
   resolve plus metadata, of the path the caller has been holding all along.

Every one of those is correct in isolation. Together they re-derive the same answer
about the same path up to five times inside one operation, and the information was
available at step 1 — `write_file_resolved` already holds the resolved path, and after
the write it holds the filesystem handle and the relative path too.

So the shape of the remaining cost is not "one slow function" but **metadata being
re-derived instead of passed forward.** The two obvious candidates, in order of
bluntness:

* `cache_identity` and `index::add_entry` both want the inode/metadata of a file the
  write has just finished touching. A `write_file` that returned, or an internal form
  that yielded, the resulting `(ino, size)` would remove #4 and most of #5.
* `index::on_file_changed` takes the `INDEX` lock twice before doing any work —
  `is_live()` locks, returns, then `is_watched()` locks again. One acquisition would do.

### And `invalidate_negative_prefix`, which is the single biggest unmeasured piece

It is not separately measurable — a private method on a private type — so it sits inside
the 25 970 ns remainder together with memfs's write. What is known about it from reading:
it iterates **all `VFS_DCACHE_SIZE` = 1024 entries on every write**, and each entry holds
two `PathBuf`s, so it touches on the order of 80 KB per write regardless of path length
or data size. Its purpose is narrow — drop *negative* entries that claimed this path did
not exist, because a write may have created it — and a scan is a defensible
implementation for a fixed 1024-slot table, but it is the one part of the write path whose
cost grows with the cache rather than with the work.

Worth measuring before being changed, which needs either a `pub(crate)` hook or the
phase instrumentation this breakdown deliberately avoided. Recorded rather than guessed.

### Result: 42× on the journal phase, 29% off every file write

Measured on the seventh unperturbed WHPX run. The evidence is a step change against a
stable baseline rather than a median delta, which matters because the baseline spans two
other changes:

| commit | `vfs_write_256` | journal phase |
|---|---|---|
| `b770d800f` | 43 064 | — |
| `1ec6842eb` | 44 290 | — |
| `d7ebfa9b7` | 43 198 | — |
| `c1c9352e6` | 44 775 | — |
| `179ccdeb8` | 44 274 | — |
| `4f6ca7a1b` | 43 288 | 14 206 |
| **`96356d747`** | **30 867** | **337** |

Six runs inside 43 064–44 775, then one at 30 867. **−29%**, and the journal phase fell
**42×**.

The breakdown accounts for the change exactly:

| | before | after |
|---|---|---|
| full write | 43 813 | **30 183** |
| `journal::record` | 14 206 | **337** |
| `index::on_file_changed` | 3 997 | 4 115 |
| remainder | 25 970 | 25 896 |

The journal saved 13 869 ns; the total fell 13 630 ns. Those agree within 239 ns — inside
the ~700 ns this subtraction can resolve — and the two untouched phases moved by 118 ns
and −74 ns, i.e. not at all. So the change is isolated to the line it touched.

Wider effects, on the same run:

* `vfs_throughput_16k_write`: 129 358 → **115 993**, −10.3%. Still **OVER** its 50 000
  budget, at 2.3× rather than 2.6×. A 16 KiB write pays the same single HPET read as a
  256-byte one, so the absolute saving is the same and the proportional one is smaller —
  which is what a fixed cost looks like.
* `vfs_read_256` +1.9% and `vfs_stat_root` +5.0%: unchanged. Both are inside the p90
  run-to-run movement of 5.2%, and neither goes through `journal::record`.
* `over_target` stays at **9**. The write benchmark was over budget before and remains
  over; nothing crossed. The earlier 10 → 9 was `task_count`, not this.

### A baseline I nearly reported wrongly, for the third time

My first pass compared the new row against the median of *all six* prior WHPX rows, and
produced "`dashboard_api_status` −55.9%" — which the journal fix had nothing to do with.
Three of those six predate `task_count`, so for any benchmark that change affected, the
median is a blend of two populations and the delta is an artifact of the mix.

This is the third appearance of the same trap today: a 0.0% reading from comparing a
baseline against a member of itself, a 42× read/write ratio that spanned a cache
boundary, and now a 56% figure from a baseline straddling an unrelated change. The
defence that worked all three times was the same — **list the rows and look at them**
before computing anything over them. `vfs_write_256`'s own history shows six flat runs
and one step, which no median could have told me as clearly.

### Withdrawing one of my own two suggestions: the double `INDEX` lock is not worth fixing

The note above offered two candidates for `index::on_file_changed`'s 4 115 ns. One was
real: `add_entry` did `Vfs::stat(path)?` **and** `Vfs::metadata(path)`, two full path
resolutions, when `FileMeta` is a superset of what it took from the `DirEntry` — fixed,
and it also removed an `.unwrap_or(0)` that turned a metadata failure into a modified
time of zero.

The other was **not** worth acting on, and the arithmetic says so plainly: `is_live()`
takes the `INDEX` lock, returns, and then `is_watched()` takes it again. An uncontended
mutex acquisition here is tens of nanoseconds, so collapsing the two saves on the order
of **25 ns of 4 115 — 0.6%**. Against that it costs a new predicate, a restructured
shared helper (`is_watched` has three other callers, including `on_file_renamed`, which
locks three times), and a diff that reads like an optimisation.

Recorded rather than silently dropped because I wrote the suggestion down, and a
plausible-sounding one in a known-issues file is an instruction to the next reader. The
general rule it illustrates: **a redundancy is worth removing in proportion to what it
costs, and "two of something where one would do" is not by itself a cost.** The
duplicate *resolve* was ~1–2 µs and worth it; the duplicate *lock* is ~25 ns and is not.
Both look identical when described as "this happens twice".
