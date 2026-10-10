## D-SPIKE-LINK-ONCE-NAMED-A-ZIG-CACHE-OBJECT-THAT-WAS-GONE — a spike's configure test failed once because ld.lld was handed an object in zig's shared cache that no longer existed; not reproduced (lane D, 2026-10-07)
**Status:** OPEN — seen once, not reproduced; watch for a recurrence

**In short:** on 2026-10-07 the GDB spike (`scripts/gdb-spike/run.sh`), run
in lane-d's work tree, failed in zlib's `configure`. Its "suffix of
executables" test runs the link wrapper as `cc -o conftest -g -O2
conftest.c`, and that call failed with `ld.lld: error: cannot open
/home/inhahe/.cache/zig/o/d2785f3b3c09e6c2a15087eac691082b/conftest.o: No
such file or directory`. The same tree had built cleanly in the scratch
worktree an hour before. Rerun at once, unchanged, it built cleanly again.

**Why it is a puzzle and not just a flake.** The link wrapper
(`slate_make_link_wrappers` in `scripts/lib/worktree.sh`) never hands
`ld.lld` a path in zig's cache for a call like this one. It compiles each
source with `zig cc -c` into its own `mktemp -d` directory, and links
those objects. A path of the form `~/.cache/zig/o/<hash>/conftest.o` on a
link line is what zig's own driver produces when it compiles and links in
one call. Either the call reached zig's driver whole, which the wrapper's
argument loop should not allow for these arguments, or zig's `-c`
compile placed something unexpected in the wrapper's directory.

**What was different that time:**
- the spike had been stopped with SIGSTOP for eight minutes while another
  lane's QEMU ran (`run-spike-paused.sh`, a runner kept outside the tree)
  and resumed;
- Mono's native class-library build was running beside it (gcc, not zig);
- zig's global cache, `~/.cache/zig` (1.6 GB, 100,887 entries), is shared
  by every lane's WSL builds, so another lane's zig may have been running.

No script in the tree prunes that cache.

**Not reproduced by:** 72 identical `cc -o conftest -g -O2 conftest.c`
calls through the same wrapper, 24 at a time in three rounds: none
failed.

**If it recurs:** keep the failing tree as it is. Record which other zig
processes were running (`ps -ef | grep zig` in WSL) and what the
wrapper's scratch directory held. Run the failing call with `bash -x` on
the wrapper. If two lanes' builds turn out to share cache entries
unsafely, the fix is a cache per lane: `ZIG_GLOBAL_CACHE_DIR` set in
`slate_make_zig_wrappers`, keyed by `$SLATE_LANE`. Every spike's first
build would then be cold.
