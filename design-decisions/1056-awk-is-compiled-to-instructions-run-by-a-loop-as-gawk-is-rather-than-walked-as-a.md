## 1056. awk is compiled to instructions run by a loop, as gawk is, rather than walked as a tree

**Date:** 2026-10-01
**Lane:** B
**Decided by:** Claude (autonomous)

**In short:** our `awk` used to run a program by walking its parsed tree,
calling itself once for every awk function call. A recursive awk function
3000 calls deep therefore ran the process out of stack and killed it, where
gawk runs 30000 without noticing. It now translates the program into a list
of simple instructions and runs them in a loop that keeps its own call
records in ordinary memory, which is how gawk works -- so recursion is as
deep as memory allows. The same change fixed four other bugs at once, because
each was a consequence of the tree walk's shape.

| Option | For | Against |
|---|---|---|
| **A. Compile to instructions; heap-allocated call frames** (chosen) | Recursion bounded by memory, as gawk's. gawk places a diagnostic on the line of the instruction that raised it and updates that line at every instruction, so an instruction carrying its token's line reproduces gawk's placement exactly, multi-line statements included. An lvalue is resolved once into a target, so `a[i++] += 1` touches one element (the walk evaluated `i++` twice). `exit`, `next` and `nextfile` inside a function are an ordinary return from the loop (the walk could not carry them out through an expression: `exit` printed an empty `awk: ` line). gawk's evaluation order -- a print's redirection before its arguments, a `getline` target before the read -- is the order of the instructions. | A rewrite of the evaluator; 343 harness rows and the probe batches (`target/drafts/loc-probe*.sh`) were the safety net. |
| B. Keep the walk; run it on a thread with a very large stack | Small change | On SlateOS anonymous memory is committed when mapped (`MAP_LAZY` is opt-in, and `posix/src/pthread.rs` maps thread stacks without it), so a 1 GiB stack costs 1 GiB of memory per `awk` up front. Fixes none of the other four bugs. |
| C. Keep the walk; a depth limit checked against the remaining stack | No crash | Still refuses programs gawk runs; fixes none of the other four. |
| D. Keep the walk; grow the stack in segments (`stacker`-style) | Unbounded depth | Stack-switching assembly per target, and `x86_64-slateos` is not one the crates know; fixes none of the other four. |

**What the instructions are.** gawk's, in effect: each made from the token
gawk makes its own from (the `/` of a division, the `~` of a match, the name of
a call), `None` for those gawk makes without a line (jumps, pops, `?:`'s, a
concatenation). See `compile.rs` for the list and its stack effects, and
`interp/run.rs` for the loop.

**Revisit when** profiling shows the loop's dispatch dominating a real
workload -- the next step would be specialising hot instruction pairs, not a
return to the walk.
