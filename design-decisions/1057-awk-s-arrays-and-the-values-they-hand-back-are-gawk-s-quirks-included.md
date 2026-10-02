## 1057. awk's arrays and the values they hand back are gawk's, quirks included

**Date:** 2026-10-01
**Lane:** B
**Decided by:** Claude (autonomous)

**In short:** `for (k in a)` visits an array's elements in an order POSIX
leaves to the implementation, and ours came from Rust's hash map, which is
seeded at random -- so the same program could print its report in a
different order every run. Arrays are now built the way gawk builds them
(its three table layouts, its hash functions, its growth rules), so the order
is gawk's. The catch is that gawk's tables also decide *what kind of value*
`k` is, and gawk's answer is surprising: with indices 9, 10 and 100, the
common idiom `for (k in a) if (k > max) max = k` answers 9, because `k`
starts out a number and turns into a string the first time it is compared.
We reproduce that too, rather than giving the "obvious" 100.

| Option | For | Against |
|---|---|---|
| **A. Port gawk's layouts and its lazily typed values** (chosen) | Output identical to gawk's, order and answers, for every program the harness has: 89 new rows, each measured. gawk is the reference this awk is held to (B-AWK-GAWK-FIDELITY-SWEEP), and a script moved between the two prints the same thing. | Reproduces gawk's surprises on purpose: the `max` idiom's 9; a field compared before it is stored keeps a different kind of index than one that was not (`$1 > 0` first makes `a[$1]`'s index a string). `value.rs` carries two shared state cells to model it. |
| B. Port only the order; hand back indices as strnums | The `max` idiom gives 100, as most people expect. Simpler values. | Disagrees with gawk on programs whose loop compares or tests its index -- most of the 42 harness rows on the index -- with nothing to point at but our own taste. And still not POSIX's answer, which leaves the index's type unsaid. |
| C. Keep a hash map, sorted iteration | Deterministic; easy to explain | Matches no awk at all: neither gawk's order nor its answers. |

**What "gawk's lazily typed values" are.** gawk keeps type flags on a value's
node and settles them in place the first time something asks, on the one node
every copy shares. Two settlements show: an index from an integer array (its
`INTIND` number) becomes a string at its first comparison, truth test or
string use; and input that looks numeric is flagged a string until its first
numeric use, which decides whether a string array keeps it as input or as
text. Which values are one node is gawk's too: a field is shared on the stack
and with a function's parameter, copied into a variable or element
(`UNFIELD`). `value.rs`'s module documentation has the whole model;
`array.rs` the layouts.

**Revisit when** the reference changes: if this awk is ever held to POSIX
alone, or to a different awk, option B becomes the better fit.
