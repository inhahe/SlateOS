## TD-A-A-A-A-MEASURED-AUTO-VERSIONING-IS-HALF-A-SMALL-FILE-WRITE-AND-THE-TWO-ROUTES-DISAGREE (lane A, 2026-09-12) — **the A/B landed; read the accelerator caveat before using the numbers**

**In short:** writing a small file costs about twice what it should, and roughly half of
that is the automatic version history — reading the old contents back and checksumming
them before the write. Measured, not inferred. But the measurement ran under emulation,
and the cost structure differs enough between emulation and hardware virtualisation that
the same change can look decisive under one and invisible under the other.

### The numbers (run b6mifed3b, commit 2de15d6f6, **QEMU TCG**, release profile)

| series | min ns | share of the write |
|---|---|---|
| `vfs_write_breakdown_full` (root, versioned) | 95,942 | — |
| `vfs_write_breakdown_unversioned` (`/tmp`) | 47,892 | — |
| **differential: versioning** | **48,050** | **50.1%** |
| `vfs_write_breakdown_history` (direct) | 26,194 | 27.3% |
| `vfs_write_breakdown_index` | 16,059 | 16.7% |
| `vfs_write_breakdown_journal` | 616 | 0.6% |
| `ns` / `access` / `intercept` / `quota` | 19 / 54 / 323 / 279 | under 1% combined |

`vfs_write_256` in the same run: 95,356 ns. `vfs_read_256`: 9,223 ns — so a write costs
**10.3× a read** of the same size.

### The two routes disagree by 1.83×, and that was the designed outcome

The benchmark measures versioning two ways on purpose, and its comment said a
disagreement would itself be the finding. It disagrees: 48,050 differential against
26,194 direct.

The differential is an **upper** bound, because the `/tmp` arm differs by more than
versioning: it is a separate `memfs` instance whose root directory holds a handful of
entries, where `/` holds the whole staged OS tree. `child_ino` does a map lookup in the
parent's children, so the root arm pays a deeper and colder lookup on every iteration.
The direct phase is a **lower** bound for the opposite reason: it calls
`try_auto_record` 200 times on identical content, so the content-addressed store dedupes
and the version list sits at its 16-entry cap, exercising the eviction path rather than
the insert path. True cost is between them.

### The accelerator caveat, which is the part most likely to mislead

**This run is TCG. Do not compare it to the WHPX figures in this file** — the same commit
measures ~43,000 ns under WHPX and ~108,000 under TCG, so a cross-accelerator delta is
meaningless. That is the straddling error this session produced three times.

More subtly: the HPET finding recorded earlier — that `record_version` calls
`hpet::elapsed_ns()`, the MMIO read whose removal took the journal phase from 14,206 ns
to 337 — **cannot be confirmed or denied by this run.** `hpet_read`'s accelerator ratio
is 0.03×: it is ~30× *slower* under WHPX, because there the MMIO access traps. Under TCG
that read costs a few hundred nanoseconds, so it is a negligible part of this 26,194 and
the figure is genuinely the read-back plus the SHA-256 plus the CAS.

So both readings are true of different machines: under **WHPX** the clock read alone is
~13,900 ns and dominates; under **TCG** it disappears and the hashing dominates. A fix
that looks decisive on one accelerator can be invisible on the other, and neither number
is the "real" one — real hardware is a third case nobody here has measured.

### What is actionable

1. `record_version`'s `hpet::elapsed_ns()` → `clock_monotonic()`. Same contract, same fix
   as the journal. Worth ~13,900 ns under WHPX, ~nothing under TCG, and it is one line.
2. The `index` phase at 16,059 ns is the second-largest component and is live **only
   because a self-test left it live** — see the indexer entry. Whatever is decided there
   changes this number by a sixth.
### MEASURED UNDER WHPX, 2026-09-12 (run bbartu7os, commit 6a0f8d89e) — the fixes land

The first WHPX run since the cold restart, and the first that could see the
`record_version` HPET substitution at all. Compared against the last WHPX/release run,
`96356d747`, so accelerator and profile both match:

| series | before | after | delta |
|---|---|---|---|
| `vfs_write_256` | 30,867 | **12,326** | **−60.1%** |
| `vfs_write_breakdown_full` | 30,183 | 12,584 | −58.3% |
| `vfs_write_breakdown_index` | 4,115 | 2,979 | −27.6% |
| `vfs_write_breakdown_journal` | 337 | 300 | stable, already fixed |

**A 256-byte write costs 12.3 µs where it cost 30.9 µs.** 18,541 ns of that is gone; the
HPET read alone was predicted at ~13,500–13,900 ns from the journal's own measured delta,
which is roughly three quarters of it, and `index::add_entry`'s second path resolution
accounts for another 1,136.

**Attribution is inferred, not isolated, and that is worth saying plainly.** The window
spans several commits including other lanes' work, so −60.1% is not all mine and no
controlled experiment separates the parts. What is solid is the direction, the magnitude,
and that the predicted HPET saving fits inside the observed one rather than exceeding it.

**The two A/B routes now agree to 1.08×**, where under TCG they differed by 1.83×:
differential 5,497 ns (43.7% of the write) against a direct `history` phase of 5,078 ns.
That agreement is itself a result. The TCG disagreement was attributed to the `/tmp` arm
differing by more than versioning; under WHPX that gap largely closes, which suggests the
spread was accelerator structure rather than a confounder in the experiment's design.

**Versioning is still 43.7% of the write** after the clock read was removed from it. The
cost that remains is the read-back, the SHA-256 and the CAS insert — real work rather than
a trapped instruction — so A-Q10's question stands and its figure is now firmer, not
smaller.

### Replicated, 2026-09-12 (run bssh17cpy, commit 24f11ef45, TCG/release)

A second independent run, and the ratio holds while the absolutes do not:

| | run 1 (2de15d6f6) | run 2 (24f11ef45) |
|---|---|---|
| `full` (root, versioned) | 95,942 | 107,025 |
| `unversioned` (`/tmp`) | 47,892 | 56,607 |
| **versioning share** | **50.1%** | **47.1%** |
| direct `history` phase | 26,194 | 25,221 |

Every absolute moved 6–18% between runs — including `unversioned`, which neither commit
between them touches — so that drift is TCG run-to-run noise, not regression. The measured
floor for this harness is a median of 1.099× pairwise under TCG with p90 1.750× and a third
of observations exceeding 25%, so a 10% shift is well inside it. **Reading it as a
regression would be the error this file documents repeatedly**, and the temptation was
real: `vfs_write_256` rose 10.1% in the run that shipped a change intended to make writes
faster.

The *ratio* is the robust quantity, and for a structural reason rather than luck: both arms
sit in the same run and move together, so noise largely divides out of a within-run
comparison and does not divide out of a between-run one. That is the argument for having
built this as an A/B in the first place.

**And the indexer finding is now confirmed empirically, not just by reading.** The new
scorecard line says `indexer live=true (initialized=true, rebuilds=1, entries=333)`.
Exactly one rebuild — which is `index::self_test`'s, never reset — so the benchmark has
indeed been measuring an indexed write by accident.

The HPET substitution in `record_version` landed between these two runs and is invisible
in them, as predicted: both are TCG, where that read costs ~450 ns rather than the ~13.5 µs
it costs under WHPX. Confirming it needs a WHPX run, and the accelerator is not selectable
— it is read from the guest's CPUID, by design.

3. Whether writes should carry version history at all is in `deferred-questions.md`. Its
   promotion trigger was "a cost figure". This is that figure: **half the write**.
