## §100 — the shell is compiled for speed, against the workspace's size-optimised userspace default

**Date:** 2026-08-02
**Decided by:** Claude (autonomous)

The workspace builds all userspace at `opt-level = "s"`, and that default has a
real reason behind it: sixty coreutils binaries are embedded in the kernel image
(`requests/coreutils_needs_kernel_embedding.md` puts the total at ~45 MiB), so a
megabyte saved is saved sixty times over.

osh does not fit that shape. It is one binary, and it is the *interpreter* — the
thing standing between every line of every script and the work that line asks
for. Its size is paid once at image build; its speed is paid once per loop
iteration, forever.

**The decision.** `[profile.release.package.oils]` overrides the default with
`opt-level = 3` and `codegen-units = 1`, alongside the override the kernel
already has.

**The measurement.** Twelve constructs, each a 40k–300k-iteration loop, best of
three, against bash 5.2.37 on the same host:

| construct | `-Os` | `-O3` | speedup | bash | `-O3`/bash |
|---|---|---|---|---|---|
| `for ((i=0;i<N;i++)); do :; done` | 2356 ms | 1939 ms | 1.22x | 1187 ms | 1.63x |
| `while ((i<N)); do ((i++)); done` | 1739 ms | 1271 ms | 1.37x | 997 ms | 1.27x |
| `while [ $i -lt N ]; do i=$((i+1)); done` | 4257 ms | 3496 ms | 1.22x | 2475 ms | 1.41x |
| `for (( )); do s=$((s+i)); done` | 2793 ms | 2138 ms | 1.31x | 1795 ms | 1.19x |
| `s+=x` | 3277 ms | 2981 ms | 1.10x | 2180 ms | 1.37x |
| `x=${v#h}` | 1806 ms | 1381 ms | 1.31x | 1304 ms | 1.06x |
| `[[ abc123 =~ [0-9]+ ]]` | 1121 ms | 859 ms | 1.31x | 924 ms | 0.93x |
| function call | 2930 ms | 2292 ms | 1.28x | 1750 ms | 1.31x |
| associative-array fill | 405 ms | 322 ms | 1.26x | 262 ms | 1.23x |
| indexed-array fill | 929 ms | 698 ms | 1.33x | 568 ms | 1.23x |
| `case` | 1332 ms | 1035 ms | 1.29x | 999 ms | 1.04x |
| `printf` | 1646 ms | 1365 ms | 1.21x | 746 ms | 1.83x |

Nothing regressed; the geometric mean is ~1.27x. That is a bigger win than any
single algorithmic fix found in the same hunt, and it moves osh from roughly
1.5–1.8x bash to roughly 1.0–1.4x.

**`codegen-units = 1` is free twice over.** It is not measurably faster than 16
units, but it produces a *smaller* binary — 3.22 MB against 3.68 MB — because
one unit lets LLVM share an inlined body instead of duplicating it per unit. So
taking it costs nothing and gives back half the size `-O3` spent.

**The cost.** +884 KB against `-Os` (2.33 MB -> 3.22 MB) and about 20 s more
build time for the crate. Set against ~45 MiB of embedded coreutils, ~0.9 MB for
the shell is under 2% of the image for the component that decides how fast
everything else *feels*.

**Alternatives considered.**

- *Leave it at `-Os` and hunt the 1.5x in the interpreter instead.* That hunt
  had already run its course: two genuinely pathological cases were found and
  fixed (unclosed-bracket globbing, the linear-scan associative array) and the
  remainder was a flat constant factor across every construct — which is exactly
  the signature of a codegen-level cost, not an algorithmic one. Ablating
  `$BASH_COMMAND`'s per-command `put_var` — the most-suspected remaining waste —
  moved nothing (0–8%, and negative on one case).
- *Raise the whole userspace default to `-O3`.* Sixty binaries times ~40% is
  most of a megabyte each, and almost none of them are hot. The per-package
  override is the precise instrument, and the kernel already establishes the
  pattern.
- *Also enable LTO.* Cargo will not take `lto` in a per-package profile — it is
  workspace-wide only — so this cannot be aimed at the shell alone without
  slowing every other crate's build. Left alone.

**How to reverse.** Delete the `[profile.release.package.oils]` section. Nothing
in the source depends on it.
