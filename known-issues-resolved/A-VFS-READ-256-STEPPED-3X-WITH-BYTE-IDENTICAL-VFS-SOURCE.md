## A-VFS-READ-256-STEPPED-3X-WITH-BYTE-IDENTICAL-VFS-SOURCE (lane A, 2026-08-17)

**Status: RESOLVED 2026-08-17 — not a regression. A QEMU/TCG code-layout
artifact, the same mechanism as
`A-CRYPTO-BENCHMARKS-STEPPED-58-PERCENT-WITH-BYTE-IDENTICAL-CRYPTO-SOURCE`
one day earlier. No kernel change is warranted and the range must NOT be
bisected. Not a boot failure; every boot in the investigation was green.**

**In short:** a benchmark that times reading a 256-byte file got three times
slower between two commits, even though not one line of the file-reading code
changed. The cause is where the compiler happened to place that code in
memory: QEMU (which runs the OS in software, not on real hardware) charges a
penalty when a hot loop sits across a 4 KiB address boundary, and four such
loops in one function landed on boundaries this time that had not before.
Nothing about the OS got slower; the measuring instrument did. There is
nothing to fix and nothing to bisect.

**What happened.** `vfs_read_256` sat at ~9000-10000 ns across 40 release runs
spanning dozens of commits, then read 30714 and 32763 on the two runs of
`5570b6b38` (lane A's merge of `origin/main`, carrying the Q24 leaf-lock sweep
`bc6664149`). A 3x step on the VFS read path is exactly the shape of a real
lock regression, which is why it held the merge to `main`.

**The paired A/B that settled it.** Three alternating pairs, one rootfs image,
one host, `--no-build` on both sides so only the kernel ELF was swapped
(`53cb74578` = the parent, `5570b6b38` = HEAD):

| pair | parent binary | HEAD binary | ratio |
|---|---|---|---|
| 2 | 36960 cycles (9950 ns), split 0% | 120710 cycles (32545 ns), split 0% | 3.27x |
| 3 | 63208 cycles (17020 ns), split 7% | 110938 cycles (29878 ns), split 0% | 1.76x |
| 4 | 39748 cycles (10694 ns), split 0% | 122154 cycles (32935 ns), split 0% | 3.07x |

`split 0%` means first-half and second-half medians agreed to the printed
digit — this is deterministic *within* each run, not a tail. Pair 3's parent
run is the one contaminated sample on the parent side (17020 ns is above its
own historical range); the two clean parent runs land on the historical ~10000
ns baseline.

**The mechanism, found in seconds by `scripts/straddle-check.py --compare`:**

```
STRADDLE GAINED (expect these to be SLOWER):
  Vfs::read_file_resolved   2221 B  ffffffff80309533 -> ffffffff8030db73
  Vfs::read_file_resolved   1819 B  ffffffff803095f8 -> ffffffff8030dc38
  Vfs::read_file_resolved   1668 B  ffffffff80309682 -> ffffffff8030dcc2
  Vfs::read_file_resolved   1568 B  ffffffff80309682 -> ffffffff8030dcc2
  ...
STRADDLE LOST (expect these to be FASTER):
  ProcFs::read_file         8 loops
  page_cache::get_or_fill   3 loops
recompiled (code changed, straddle not comparable): 0
only in new: 0    only in old: 0
PAGE CROSSINGS: + Vfs::fill_file_page 0 -> 1,  + Vfs::read_file_resolved 0 -> 1
```

`Vfs::read_file_resolved` is precisely the function `vfs_read_256` measures,
and it gained **four** straddling loops at once.
`B-BENCH-TCP-CHECKSUM-PAIR-BIMODAL-1.7x` measures a single straddle at ~1.7x;
four compounding is consistent with the observed 3.0-3.3x.

**The five evidence lines the crypto precedent asks for, all satisfied:**

1. **Byte-identical source.** `git diff --stat 53cb74578 HEAD -- kernel/src/fs/vfs.rs
   kernel/src/mm/page_cache.rs kernel/src/bench.rs kernel/src/net/ipv6.rs` is
   **empty**. The tool agrees independently and without being told:
   `recompiled: 0`, meaning every function it compared had the identical loop
   signature in both binaries and only its *address* moved.
2. **Exact function-to-benchmark mapping.** The gained straddles are in the
   measured function itself. And the control is built in: `vfs_write_256`
   exercises the same VFS, the same `fs.lock()`, the same converted leaf locks
   — and is **flat** (parent 113172/114579/116799 ns, HEAD
   119795/111638/121059), because the *write* path gained no straddles. A lock
   regression from Q24 could not have missed the write path.
3. **Magnitude matches the known per-straddle cost.** Four loops x ~1.7x each.
4. **Binary-dependent, not host-dependent.** Established by the alternating A/B
   above on one image and one host: the parent binary is fast every time it is
   loaded and the HEAD binary is slow every time, interleaved on the same
   machine within minutes of each other.
5. **Movements in both directions.** Median-of-3 vs median-of-3 over the 86
   benchmarks common to all six runs: **median ratio 1.005**, 5 benchmarks
   above 1.25x and 3 below 0.80x. A real slowdown moves one direction; a layout
   re-roll moves both. Each of the extremes checks out mechanically:

   | benchmark | ratio | straddle-check says |
   |---|---|---|
   | `vfs_read_256` | 3.04x | `Vfs::read_file_resolved` +4 loops |
   | `page_fault` | 1.64x | `handle_page_fault` +2 loops (784 B, 688 B), 0 lost |
   | `vfs_throughput_16k_read` | 1.45x | same read path |
   | `heap_raw_alloc_free_512` | 0.71x | `KernelHeap as GlobalAlloc::alloc` **lost** a 542 B loop |
   | `heap_alloc_free_64` | 0.72x | same |
   | `heap_raw_alloc_free_4096` | 0.73x | same |

**The bench harness's own instrument now agrees.** With the A/B records
labelled by the binary they measured, `mode_structure()` returns
`mode-structured` on `vfs_read_256`: 11 commits have been measured more than
once, **zero of them straddle the split**, 9 sit below it and 2 above (and
those 2 — `5570b6b38` and `f61bc4e71` — are the same binary). Per that
function's own docstring, a split that separates *binaries* is a property of
the compiled image, and "bisecting for a guilty commit is the wrong tool".

**It still passes.** `vfs_read_256`'s target is 200000 ns. The worst clean
measurement in the whole investigation is 32935 ns — 16% of budget. Nothing is
over target.

### A finding that was chased and then WITHDRAWN — `net_ipv6_parse`

Recorded because the withdrawal is the more transferable lesson.

`net_ipv6_parse` had read 80-82 ns across thirteen runs, then **120 ns in both**
of the first two HEAD runs, with `split 1st=448 2nd=448 (0%)` — two runs, two
identical numbers, zero within-run split. That looks airtight and it is not.
Static analysis then refused to support it: `Ipv6Packet::parse` is `0x1b8` =
440 bytes with the *identical* mangled hash `hc6fd05157676c159` in both
binaries, crosses **zero** page boundaries in each, and the measured loop sits
at the same offset in both (`run_all+0x1d4df`, spanning `0x97` bytes), neither
straddling. That left a real +144-cycle cost with no mechanism.

So it was sampled instead of published. Final tally of `min` cycles:

| binary | samples |
|---|---|
| parent `53cb74578` | 304, 294, 300, 310 |
| HEAD `5570b6b38` | 448, 448, 368, **306, 296, 306** |

The last three HEAD samples are indistinguishable from the parent, and at the
ns level the paired A/B reads parent 79/80/83 vs HEAD 82/79/82 — flat. The 448s
belonged to the pre-repack image and transient host state, and a rootfs image
cannot change an in-kernel pure-compute benchmark, so it was host state all
along. **No regression. Do not re-open this on the strength of the two 448s.**

**Lessons, in the order they cost time:**

- **Two agreeing samples are not a replication.** `split 0%` proves a run was
  internally consistent, which is a statement about that run's host conditions,
  not about the binary. `net_ipv6_parse` produced `448/448 (0%)` twice and was
  still noise. When a step has no mechanism, take more samples before writing
  it down.
- **Run the straddle check FIRST.** The crypto entry ends with exactly this
  advice — *"The straddle check should have run **first**: seconds, no boot."*
  — and it was not followed here. It would have named `Vfs::read_file_resolved`
  in under a second, before any of the boots, the scratch worktree, the
  13m49s release rebuild, or the rootfs repack.
- **A/B by swapping the ELF, not by checking out the tree.** `--no-build` with
  the two kernel binaries copied over
  `target/x86_64-unknown-none/release/kernel` in turn holds the rootfs, the
  host and the harness fixed, and takes ~4 minutes per side. Checking out the
  parent worktree instead cost a 15-minute checkout and a 14-minute rebuild,
  and changed the rootfs underneath the comparison.
- **`bench/history.jsonl` records which BINARY was measured.** A swapped-ELF run
  filed under the checked-out tree's commit is a false record: it poisons the
  `level_shifts` baseline (`comparable_records` filters by host, profile and
  deliberate host-load — *not* by contamination or by commit), and it makes
  `mode_structure` report `run-noise` because one commit's repeats then straddle
  the split. The eight A/B runs here carry a `note` field saying so, and the
  four control runs carry `commit: 53cb74578`. Do the same for any future
  swapped-ELF comparison; `boot-history.jsonl` has a `label` field
  (`BOOT_LABEL`) for it.

**Also disproved along the way**, so nobody re-derives them: an atime write on
the read path (only procfs/kshell/self_test call it); memfs root-directory
growth (`vfs_readdir` moved 1.5%); the lockdep class-table scan
(`lock_uncontended` 282/278 vs a 259-306 historical range); the heap and frame
allocators (all normal); and a suspected fourth VFS-under-spinlock bug in
`kernel/src/net/httpd.rs`. That last one is a **confirmed false positive** and
must not be "fixed": line 1536 is `let doc_root = *DOC_ROOT.lock();`, which
copies the `&str` out and drops the temporary guard at the semicolon, so no
lock is held across the `Vfs::stat` calls at 1541/1548.
