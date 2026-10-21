## D-SPIKES-SLATELINKS-REPEAT-THE-LINK-CHECKS-AND-STAGING-AND-HAVE-DRIFTED (lane D, 2026-10-21) — OPEN

**Status:** OPEN

**In short:** ten scripts re-link the ported programs against our C library,
and each carries its own copy of the same steps: count the missing and the
duplicate symbols, check the unwind-table segment and the SlateOS ABI note,
strip, and put the result where the image build picks it up. The copies have
drifted, so a fix made in one is missing from the others, and two of the
drifts are latent bugs.

**Where, and what has drifted** (found by the 2026-10-21 review of
`scripts/cmake-spike/slatelink.sh`, which was itself made by copying Oils'):

| script | drift |
|---|---|
| `scripts/llvm-spike/slatelink.sh:84`, `scripts/coreutils-spike/run.sh:231` (lane B's) | `readelf -n … \| grep -q SlateOS` under `set -o pipefail`: if `grep` exits before `readelf` has written everything, `readelf` dies of SIGPIPE and the check reports a missing note that is there. LLVM's then deletes the staged tool and fails the image. Oils' and CMake's use `grep >/dev/null` for this reason. |
| `scripts/gdb-spike/slatelink.sh:115`, `scripts/mono-spike/slatelink.sh:111`, `scripts/binutils-spike/slatelink.sh:109` | `strip -o` straight onto the staged path: a strip that fails half-way leaves half a binary where the image build looks. Oils' and CMake's strip to `.new` and `mv`. |
| `scripts/llvm-spike/slatelink.sh` | a failed strip carries on (`STRIP_FAILED`) instead of failing. |
| `scripts/espeak-spike/slatelink.sh` | checks the link's exit status; the others infer success from the output file and the two counts. |
| all but CMake's | no `-Wl,--error-limit=0`: `ld.lld` stops at 20 errors, which caps both counts and can leave missing symbols uncounted behind duplicates. |

**The proper fix:** one helper — link, count both kinds of failure without the
error cap, run the ELF checks, strip to a temporary file and move it into
place — that every `slatelink.sh` calls with its output name, staged path and
link inputs. It belongs beside `slate_make_link_wrappers` in
`scripts/lib/worktree.sh`, which is lane A's, so it needs a request; or in a
lane-D-owned `scripts/lib`-style file the ports source. Until then each copy's
drift is a bug to fix where it is.
