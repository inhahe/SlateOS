## §921 — The 70ms per-file-open was not the antivirus; re-measure on E:

**Date:** 2026-09-07. **Decided by:** Operator (factual correction). **Lane:** A.

**In short:** every build and check was paying ~70ms per file opened. The
entry hypothesised Defender real-time scanning. The operator reported that
`D:\visual studio projects` was already excluded from scanning, so the
antivirus was not the cause. More likely explanations: CPU saturation from
concurrent work, filesystem I/O contention from four continuous backup jobs
(two local, two cloud), and HDD latency (the tree has since moved to SSD).
The question is effectively closed — re-measure on E: if the latency
persists, but the leading hypothesis is invalidated.
