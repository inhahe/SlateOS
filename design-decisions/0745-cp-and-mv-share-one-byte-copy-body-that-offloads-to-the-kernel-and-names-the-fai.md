## 745. `cp` and `mv` share one byte-copy body that offloads to the kernel *and* names the failing end, because GNU has a third error sentence — superseding §741

**Date:** 2026-09-01
**Decided by:** Claude (autonomous)
**Lane:** B

**In short:** Copying a file's bytes can be done two ways. You can ask the
kernel to do it (`copy_file_range`), which is fast and preserves *holes* — the
unwritten regions of a "sparse" file, which is how a 4 GB disk image can occupy
200 MB. Or you can loop in the program: read a block, write a block. The loop is
slower and writes every hole out as literal zeroes, but when something fails it
knows which of the two files failed, because it made the two calls itself. This
tree had `mv` doing the first and `cp` doing the second, and §741 wrote that
down as a considered trade. It was not one — it was a false choice, and both
programs now run the same body, which offloads *and* reports the failing end.

### Why §741 was wrong

§741 assumed the diagnostic has to name one file. It does not. GNU's
`sparse_copy` (`copy.c:307`) has **three** sentences, not two:

| Sentence | Site | Emitted by |
|---|---|---|
| `error copying SRC to DST` | `copy.c:376` | the offload loop |
| `error reading SRC` | `copy.c:402` | the fallback loop |
| `error writing DST` | `copy.c:435` | the fallback loop |

The first names *both* files and lets the errno carry the rest. That is the
piece §741 did not have: it is not a guess about which end failed, and it is not
a refusal to say anything — it is an honest statement of what the kernel
actually reported. So the offload keeps its speed and its holes, the fallback
keeps its precision, and nothing is traded. §741's "writing both, as GNU does"
rejection — *"to recover a sentence in a case nothing in the harness can
reach"* — priced only the second and third sentences and never noticed the
first, which costs nothing at all.

§741's judgement about *which* loss matters is still right, and is why this is a
supersession rather than a reversal: the sparse-file loss is silent and
permanent, the diagnostic loss is loud and recoverable. That is exactly why the
resolution is "keep the offload and say something true about it" rather than
"drop the offload to get a better sentence".

### What made it visible

Merging the two bodies. Neither program could see the defect alone — `cp`'s
loop was locally correct and `mv`'s `io::copy` was locally correct, each simply
missing what the other had. It was only when they had to become *one* body that
the question "which of these two is right?" had to be asked, and the answer
turned out to be "neither, and upstream has known that since it wrote the third
sentence."

This is the same lesson the module header already records twice, in the two
fixes that landed on one program and not the other. Duplication does not only
double bugs; it hides the fact that both copies are wrong, because each one
looks fine next to its own tests.

### The fallback's precondition, which is correctness and not tuning

The read/write loop is entered only while **nothing has been copied yet**
(upstream's `*total_n_read == 0`, `copy.c:366`). This is not an optimisation.
The offload advances both file positions, so the fallback — which resumes from
those positions — would silently produce a truncated destination if it started
after a partial offload. The errno half of the same guard is upstream's
`is_CLONENOTSUP` (`copy.c:298`) plus a special case for the `ENOENT` seen across
CIFS shares.

Upstream's reason for listing `EPERM` there is worth keeping: it can mean
`copy_file_range` is filtered out by seccomp, in which case the plain copy
works, or that the file is immutable, in which case the plain copy fails too and
reports the more accurate error. Either way, falling back is right.

A zero return with nothing copied also falls back, because `copy_file_range`
wrongly returned 0 when reading from procfs on Linux through at least 5.6.19
(`copy.c:345`). A zero *after* real progress is an honest EOF.

### What was rejected

**Leaving the two bodies alone and only fixing `mv`'s sentence.** Would have
closed the logged issue and left the duplication — which is the thing that
produced the issue.

**§741's post-hoc probe** (a zero-length read on the source after the failure,
to learn which end died). It is ingenious and it is unnecessary once the third
sentence exists: it spends a syscall and a paragraph of explanation to
manufacture a distinction that upstream simply declines to claim.

**Sizing the buffer from the destination's `st_blksize`**, as upstream does.
Correct, and deliberately not done here — it would move throughput numbers in a
stage whose certification rule is that they must not move. Separate change.

### Reversal

`copy::copy_bytes` is the whole of it, and both call sites are one statement
each. The three sentences are constructed in that one function, so a change of
wording is a change in one place — which is the property §741's arrangement did
not have, and the reason the two programs' wording had drifted apart in the
first place.
