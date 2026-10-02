## A-A-4x-CRYPTO-"REGRESSION"-BISECTS-TO-A-COMMIT-THAT-ONLY-EDITS-audio_mixer.rs (lane A, 2026-08-18)

**Status: the machine code did not change. Do NOT revert the commit this
bisects to.** What is wrong is our confidence in the benchmark, not the kernel.

### What was seen

`bench/history.jsonl` shows `crypto_sha256_64B` sitting in a 1930-2560 ns band
for 20 consecutive runs and then stepping to 8087/8083 ns — a clean step, not
drift, and the second reading has a perfectly clean contamination canary
(spread 0%, whole-suite drift -0.1%), so it is not host load.

Rebuilding the last known-good commit `5666d38cb` from scratch in a fresh
worktree on the same host, with the same staged service binaries and
bootloader, reproduces the split:

| benchmark | `5666d38cb` | HEAD `b2180939e` | ratio |
|---|---|---|---|
| `crypto_sha256_64B` | 7426 cy | 30048 cy | 4.05x |
| `crypto_sha256_1KiB` | 55274 cy | 249860 cy | 4.52x |
| `crypto_hmac_sha256` | 20058 cy | 77056 cy | 3.84x |

### What it bisects to

`git bisect run` over the 145-commit range, restricted with `-- kernel/` to the
21 commits that touch guest code, threshold 15000 cycles (chosen in the empty
band between the ~7-10k and ~28-30k populations — no measurement has ever
landed near it):

```
GOOD 91a52df12: 9490    BAD  665fbb27b: 28184
GOOD 80a1e70c1: 7180    GOOD 398d57d1c: 7324
GOOD 0e54368e9: 7364
665fbb27bd5773438c629798dd5d2e39df49c731 is the first bad commit
```

**`665fbb27b` is "kernel: take `mix_output`'s 12 KiB of scratch off the stack".
It changes `kernel/src/audio_mixer.rs` and nothing else** — 77 insertions, 25
deletions, one file. There is no causal path from the audio mixer to SHA-256.

### Why it is not a code change

`llvm-nm` on a pre-regression kernel and on HEAD gives, for every SHA-256
symbol, **the same size and the same mangled hash** — `compress` is `0x26e`
bytes with hash `h8234a763022d2833` in both, likewise `Sha256::update`
(`0x14f`), `sha256` (`0xb6`) and `hmac_sha256` (`0x411`). Identical machine
code, identical 64-byte input, ~4x slower, deterministic.

The only thing that differs is the **address**:

| symbol | good build | HEAD |
|---|---|---|
| `crypto::compress` | `…80b039f0` | `…80afce00` |
| `crypto::sha256` | `…80afd1f0` | `…80af6600` |
| `net::tcp::tcp_checksum_ip` | `…808b5be0` | `…808ae8c0` |

a uniform shift of `0x6BF0` (27632 bytes).

### The shape of the damage, across all 98 benchmarks

Comparing the GOOD step (`398d57d1c`) against the BAD one (`665fbb27b`) — same
host, same suite, adjacent commits:

| ratio | benchmark | good -> bad |
|---|---|---|
| **12.48x** | `net_tcp_checksum_v6_1460b` | 5946 -> 74224 |
| 3.85x | `crypto_sha256_64B` | 7324 -> 28184 |
| 3.63x | `crypto_sha256_1KiB` | 54606 -> 198208 |
| 3.18x | `crypto_hmac_sha256` | 19712 -> 62654 |
| 2.46x | `vfs_write_16k` | 1434244 -> 3521676 |
| … | … | … |
| 0.78x | `crypto_poly1305_1KiB` | 24274 -> 18868 |
| 0.75x | `ipc_channel_roundtrip` | 2714 -> 2028 |

**Median ratio across all 98: 0.995.** Five benchmarks above 2x, none below
0.5x, and several genuinely *faster*. This is not a machine that got slower —
it is a handful of specific hot loops falling off a cliff while everything else
is unmoved. The two worst are both tight streaming loops over a byte buffer,
which is the signature of an address-indexed structure in the emulator
(translation-block jump cache and/or the softmmu TLB, both direct-mapped on
address bits) aliasing for those particular loops at those particular
addresses.

### Why it still matters even though it is not our bug

- **Every crypto and checksum number in `bench/history.jsonl` is only
  comparable within a build.** A 4x swing can be introduced by an unrelated
  one-file commit, so the 10%-regression rule in `CLAUDE.md` cannot be applied
  to these benchmarks as they stand: it will fire on noise and miss real
  regressions hidden under a favourable relayout.
- **It will keep flapping.** Nothing about `665fbb27b` is special; the next
  commit that shifts `.text` can move it back or somewhere worse.
- The effect is almost certainly **invisible on real hardware**, so it must not
  be "fixed" in the kernel by contorting the crypto code.

### Open: the mechanism, and what to do about the suite

Being run down now with a `QEMU_EXTRA` knob added to `boot-test.sh` (identical
binary, varied emulator) — see the next entry when it lands. The fix for the
suite is likely one of: pin the hot benchmarks' buffers to a fixed alignment,
report crypto throughput relative to an in-run reference loop rather than in
absolute cycles, or mark these benchmarks as placement-sensitive and compare
them only against same-build baselines.

### RESOLVED as to cause (2026-08-18): placement, proven by a control that killed the first answer

Three builds of the **same source**, differing only in how symbols are named
and therefore in what order the linker gathers `.text`:

| build | `crypto::compress` at | `crypto_sha256_64B` |
|---|---|---|
| HEAD, ordinary (legacy mangling) | `…80afce00` | **30048 cy** |
| HEAD + `#[rustc_align(4096)]` on `compress` | `…80365000` | 7196 cy |
| **HEAD, pristine, `RUSTC_BOOTSTRAP=1` only** | `…80364980` | **7188 cy** |

The third row is the control, and it is the important one. The alignment
attribute **did nothing**: pristine source built the same way is just as fast.
What actually moved `compress` was `RUSTC_BOOTSTRAP=1` switching Rust's symbol
mangling from legacy to v0 — mangled names become section names
(`.text._RNv…`), section names drive link order, and `compress` landed
somewhere else.

**The near-miss is worth recording.** The aligned run restored all three
benchmarks to baseline (7196/55676/19962 against 7426/55274/20058) and looked
like a clean, causal fix. Reporting it would have shipped "align `compress` to
4 KiB" as a remedy for something alignment has no part in. The only reason it
was caught is that the mangling change was visible in `llvm-nm` output and was
treated as a confound rather than a curiosity.

### What is established

- **No kernel code is at fault.** SHA-256's machine code is byte-identical
  across every one of these builds; only `compress`'s address changes.
- **The effect is one function's address.** `crypto_sha256_64B`, `_1KiB` and
  `hmac_sha256` all route through `compress` and all three move together.
  `crypto_sha512_64B` — near-identical code, same file, *different function* —
  went 0.59x (faster) across the same boundary, and `poly1305`, `chacha20`,
  `crc32`, `crc32c`, `ed25519` and `x25519` are all ~1.0x or faster.
- **Any move is enough.** `…80365000` and `…80364980` are both fine; they are
  not related by alignment, only by not being `…80afce00`.
- **It is emulator-side.** `-accel tcg,tb-size=512` with a bit-identical binary
  changed nothing (8053 ns vs 8087/8083), which rules out translation-buffer
  flush thrash — the one hypothesis in this family a larger buffer would fix.
  The remaining candidates are QEMU's address-indexed structures (the
  direct-mapped TB jump cache, the softmmu TLB). Which one it is has **not**
  been pinned down, and pinning it down needs QEMU source we do not have
  checked out. It does not change what we should do.

### What NOT to do

- **Do not revert `665fbb27b`.** It is a good commit and is not the cause; it
  merely shifted `.text` by 0x6BF0.
- **Do not align, pad or reorder kernel code to chase this.** Any such fix is
  luck, holds only until the next commit shifts `.text` again, and is
  meaningless on real hardware where the effect almost certainly does not
  exist.
- **Do not switch the kernel to v0 mangling to "fix" it.** That is the same
  luck wearing a respectable hat: it reshuffles every symbol and will land some
  other hot function on the bad address eventually.

### What WAS done about it (2026-08-18, commits df0403e6a and 60e75a32a)

Nothing in the kernel — there is nothing there to fix. Both changes are to the
measuring apparatus, and both target the same failure: this cost a multi-hour
bisect because the history recorded *what* the numbers were and nothing about
*where the code was*.

1. **`hot_symbols` on every benchmark record.** `bench-history.py` now reads
   the load address of `crypto::compress`, `crypto::sha512_compress` and
   `net::tcp::tcp_checksum_ip` out of the measured ELF and stores them beside
   the timings. A repeat of this becomes a one-line observation — the crypto
   benchmarks jumped *and* `compress` moved in the same run — instead of a
   bisect. The ELF is parsed directly rather than via `nm`/`objdump`, neither
   of which is on PATH here by default; a diagnostic that silently records
   nothing on a machine missing an optional tool is worse than none, because
   its absence reads as "the addresses did not move". For the same reason the
   field is *absent* when no ELF was offered but `{}` when one was offered and
   yielded nothing.

2. **`--experiment` / `BENCH_EXPERIMENT`, so probes stop poisoning baselines.**
   The five runs above measure kernels no checkout reproduces — three at
   ~8085 ns for `crypto_sha256_64B`, two at ~1936 for *identical source* under
   a different mangling scheme, one under non-default QEMU flags. All five went
   into the history unlabelled, and all five would have entered one 8-run
   reference window, stretching that benchmark's outlier fence past 4x and
   blinding the level-shift detector for it for the next eight runs. They are
   now labelled with what each actually was; labelled records are kept in full
   but excluded from `comparable_records`. `QEMU_EXTRA` implies the label
   automatically, since that was the case that slipped through; a hand-modified
   *guest* cannot be detected from the harness and must still be declared.

**What is still open.** Which QEMU structure produces the effect (TB jump cache
vs. softmmu TLB) is unknown and needs QEMU source we do not have checked out.
It is recorded as unknown rather than guessed because the guess would be
untestable here — and knowing the answer would not change either action above.

### The apparatus was used in anger, and it worked (2026-08-18, `49f8e2b52`)

**In short:** the very next commit to touch `crypto.rs` moved SHA-256 63-68%
*faster*, and `hot_symbols` — added above for exactly this — answered why in one
line instead of a bisect. It is placement again, not a speed-up.

`ec93008ad`/`49f8e2b52` replaced the kernel's private SHA-256 with the shared
`sha2` crate (a re-export, so the machine code is the same implementation).
Lane C's request predicted a 22% gain from that crate's compression function;
the reply to it (`requests/a-c-sha2-kernel-will-adopt-but-your-22pct-does-not-carry.md`)
predicted **no gain**, and committed to checking the addresses before the code if
the numbers moved. They moved. Here is the check.

`compress` is a **crate-root** item in `sha2`, so adopting it moved the function
from the middle of the kernel's own `.text` to a completely different part of the
image — `…80afce00` → `…8119bb30`, **+0x69ED30 (6.6 MiB)**. That is the largest
displacement this benchmark has ever seen, and the SHA-256 family landed back in
its fast band:

| commit | exp | sha256_64B | sha256_1KiB | **sha512_64B** | hmac_sha256 | chacha20_1KiB | poly1305_1KiB |
|---|---|---|---|---|---|---|---|
| `5666d38cb` | — | 2043 | 21170 | 3246 | 5542 | 12452 | 5280 |
| `b2180939e` | tb-size control | 8087 | 67350 | 2664 | 20781 | 12068 | 5094 |
| `b2180939e` | tb-size control (repeat) | 8083 | 67219 | 1908 | 20730 | 12056 | 5104 |
| `780ab7be2` | tb-size=512 test arm | 8053 | 67084 | 1903 | 20695 | 12128 | 5079 |
| `20b45a860` | `rustc_align(4096)` (confounded) | 1935 | 14971 | 1952 | 5367 | 12216 | 5209 |
| `20b45a860` | mangling control | 1937 | 14995 | 1953 | 5406 | 12124 | 5162 |
| `60e75a32a` | — | 7930 | 65754 | 1908 | 20768 | 12085 | 5128 |
| **`49f8e2b52`** | **— (sha2 adopted)** | **2920** | **20865** | **1929** | **7601** | 12192 | 4985 |

(`entries` — the per-iteration cycle counts, the quantity the bisect above used.)

**SHA-256 is bimodal; the controls are not.** Across those eight runs
`crypto_sha256_64B` is either ~1935-2920 or ~7930-8087, with nothing between,
and `_1KiB` and `hmac_sha256` step in lockstep with it — all three route through
`compress`. Meanwhile **`crypto_sha512_64B`, which was *not* migrated, sits at
1903-1953 across every one of the same boundaries** (the 3246/2664 in the first
two rows are the contaminated runs), and `chacha20`/`poly1305` are flat to ±3%.
The `sha512_compress` address barely moved either (`…80af5580` → `…80af3570`).
An unmigrated near-identical function in the same file, unmoved and unchanged, is
as clean a control as this bench can offer.

**So there is no 22%.** The crate's `compress` is not measurably faster than the
one it replaced; the function merely landed on a better address. Had
`hot_symbols` not been in the record, a −63% on three crypto benchmarks the same
day a crypto crate was adopted is exactly the coincidence someone would have
written up as a win.

**One number is not claimed.** 2920 sits above the previous fast band's
1935-2043, so it is tempting to read a residual cost. It is not read that way
here: `crypto_sha256_64B` was flagged in this run's dispersion, and a single
sample inside a bimodal, placement-sensitive benchmark cannot support a 1.4x
claim in either direction. Likewise, a first pass that split all 70 records at a
flat `<4500` cycles produced a headline "60.38x between the bands" — that figure
is an artifact (SHA-512's own contaminated runs exceed 4500 and fall in the
"slow" side), and it is recorded here only so nobody re-derives it and believes
it. The defensible evidence is the eight-run table and the SHA-512 control,
nothing wider.

**Nothing was done to the kernel in response, per the rules above** — no
alignment, no padding, no reordering. The numbers are in the history as measured,
with the addresses beside them, which is the whole point.
