## §420 — The HarfBuzz sweep reports mixed-script strings apart from its verdict

**Date:** 2026-08-14
**Lane:** C
**Decided by:** Claude (autonomous)

`gui/font/tools/harfbuzz_sweep.py` shapes a 23-string corpus with both this
crate and HarfBuzz over every face on the host, and its value is entirely in
the *set* of disagreements staying the one you expect. Two of the 23 strings
mix scripts:

    hello <hebrew> world
    abc <arabic> xyz

The two halves of the sweep cannot be asked the same question about those.
`hb_buffer_guess_segment_properties` takes the first strong character and
picks **one** script and one direction for the whole buffer; both strings
start with Latin, so HarfBuzz shapes the Hebrew and the Arabic as Latin. This
crate itemizes into script runs and shapes each on its own. The resulting
differences are real and are the itemizer's, not the shaper's:

* FrankRuehlCLM-Bold put `vav` 10 units left of HarfBuzz, because our `hebr`
  run reaches a kern that HarfBuzz's `latn` buffer never selects. The Hebrew
  word shaped **on its own** agrees exactly, -10 and all.
* 43 Arabic faces returned isolated forms from HarfBuzz where we returned
  joined ones, for the same reason.

Two options.

**Itemize inside the sweep** — split the string into script runs in Python and
shape each with its own HarfBuzz buffer, so both halves answer the same
question. Correct in principle, and it would turn 49 dead lines into real
comparisons. But `uharfbuzz` exposes no script-property lookup, so it means
reimplementing the itemizer in the oracle: the Unicode script property, the
Common/Inherited absorption rule, and the bidi run splitting. An oracle that
reimplements the thing under test tells you the two implementations agree with
*each other*, and every rule mirrored slightly wrong becomes a false alarm
that costs a diagnosis to dismiss.

**Report them apart** — keep shaping the whole buffer, count the mismatches,
and print them under their own heading instead of mixing them into
`misplaced`/`differ`. Chosen. The cost is that a genuine mixed-script
regression would land in a bucket already known to be non-empty, which is a
real loss. It is outweighed by what the buckets were doing before: 49 of the
report's lines were noise, and each of the two had already cost a full
diagnosis session to trace back to `guess_segment_properties`. With them held
out, `misplaced` fell from 19 to 13 and every remaining entry is one string —
Devanagari, waiting on the Indic shaper — so the next real regression will be
visible the moment it appears.

Revisit if the crate ever exposes its run boundaries to `shape_dump`: the
sweep could then feed *our* itemization to HarfBuzz, which is the honest
version of the first option and costs no reimplementation.

**Where.** `gui/font/tools/harfbuzz_sweep.py` — the `MIXED` set, the branch on
it in `main`, and the `mixed` line in the report.
