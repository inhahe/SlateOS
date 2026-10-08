## TD-B-SORT-SORTS-ON-ONE-THREAD (lane B, 2026-10-08)

**Status:** OPEN -- performance, not behaviour.

**In short:** GNU `sort` sorts each buffer with up to eight threads
(`--parallel`, default the processing units this process may use, at most
eight). Ours sorts on one. The output is the same either way -- a stable
sort is one answer however many threads find it -- so nothing a user sees
differs but the time a large sort takes on a machine with several cores.

**Where:** `userspace/coreutils/src/bin/sort/external.rs`, `sort()`: each
buffer's lines are ordered by one `sort_by`. `--parallel` is parsed and
checked as upstream checks it (`limits::specify_nthreads`), and the thread
count already does the one thing that is observable -- it sizes the buffer
(`bytes_per_line`), which decides when a temporary file is made -- so that
part is upstream's and must stay so.

**The fix:** sort a buffer's line indices on `threads(cfg)` threads -- split
into that many runs, each sorted on its own scoped thread, then merged with
ties to the earlier run (which keeps the result stable, so `-s` and `-u`
are unchanged) -- and measure it against upstream on a few hundred MiB
before keeping it. Upstream's merge tree (`sortlines`, `merge_tree_init`)
also writes the output while the last levels merge; that pipelining is the
part worth copying if a plain split-and-merge does not close the gap.
