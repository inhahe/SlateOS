## 1341. The VP9 encoder takes glibc's logarithm as x86-64 computes it, with FMA

**Date:** 2026-10-04
**Lane:** F
**Decided by:** Claude (autonomous), within §1339's rule that the encoder's
frames are `vpxenc`'s byte for byte.

**In short:** for small pictures (352x288 pixels or fewer) libvpx decides
how to cut each frame into blocks with a tiny neural network, and the
network's inputs are logarithms taken by the C library. A logarithm can
come out one way or the other in its last bit depending on how the library
computes it, and the network's verdict can flip on that bit. The encoder
therefore computes the logarithm exactly as the C library on the machine
that made the reference encodes does: glibc 2.39 on a 64-bit Intel or AMD
processor, which (since about 2013) uses a version built with fused
multiply-add instructions.

**What was decided.** `gui/video/vp9/src/enc/glibcmath.rs` holds glibc's
`logf` -- its table and polynomial -- in the operation order of the FMA
build, read from the library's own machine code: four multiply-adds fused,
`r * r` and `y0 + r` not. A fused multiply-add is done in integer arithmetic
there too, so the result does not depend on how the platform's own `fma`
rounds. `tools/logf_reference.c` hashes glibc's results; the tests match
every float of the features' range (1 to 2^29, 243 million values) and a
million random multiply-adds.

**Why.** The reference encodes are what the encoder is checked against, and
they were made by `vpxenc` linked to glibc on an x86-64 processor with FMA.
On that machine glibc's `logf` is an ifunc that picks the FMA build; its
results differ from the plain C build's in the last bit for some inputs (its
documented error is 0.82 ulp, so it is not correctly rounded either way).
Any other logarithm makes the encoder's choices differ from the reference's
on the blocks where the network's score sits a rounding away from zero.

**Alternatives.**

| | For | Against |
|---|---|---|
| glibc's FMA build (chosen) | the references match; the build every x86-64 machine of the last decade runs | `vpxenc` on a processor without FMA, or with another C library, would decide a few blocks differently -- so would this encoder against it |
| glibc's plain C build | what the C source says | not what runs on the reference machine: some decisions would differ from the references |
| A correctly rounded logarithm | an exact definition, platform-free | matches neither glibc build where they round differently |
| Rust's `f32::ln` | no code to keep | whatever the platform's math library does: on Windows it is MinGW's, unrelated to glibc |

**Revisit when** the references are remade on a machine with another C
library or without FMA: `tools/logf_reference.c` run there gives other
hashes if its `logf` rounds differently, and the tests then name the
difference.
