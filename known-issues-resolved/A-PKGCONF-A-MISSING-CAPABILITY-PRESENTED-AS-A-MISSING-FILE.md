## A-PKGCONF-A-MISSING-CAPABILITY-PRESENTED-AS-A-MISSING-FILE (lane A, 2026-08-16)

**Status:** FIXED (the grant, and a `--debug` re-run so the next one is read
rather than guessed). Recorded because the *class* will recur with every
further C port, and nothing in the tree warns about it.

### What happened

The new `self_test_pkgconf_on_slateos_libc` rung failed on its first real run,
on all five assertions, with pkgconf's own upstream diagnostic:

```
Package slateos-simple was not found in the pkg-config search path.
Perhaps you should add the directory containing `slateos-simple.pc'
to the PKG_CONFIG_PATH environment variable
```

Every obvious reading of that sentence is wrong. The `.pc` fixtures were staged
correctly, at the right path, and `PKG_CONFIG_LIBDIR` was delivered to the child
(`Stored 3 argv, 3 envp entries` in the same log).

The cause is one line in `libpkgconf/path.c` (pkgconf 2.3.0), in
`prepare_path_node` — which `pkgconf_path_add(…, filter=true)` calls for every
directory in the search path:

```c
if (lstat(path, &st) == -1)
        return NULL;     /* silently drop this search directory */
```

The `lstat` is not a permission check — pkgconf keys its search-path dedup cache
on `st_ino`/`st_dev`, so it needs the `struct stat`. (The block is compiled in
whenever `<sys/stat.h>` exists and the host is not Windows, which is our
`--host=x86_64-linux-musl` build.) But `SYS_FS_LSTAT` (and `SYS_FS_STAT`)
gate on `(File, METADATA)`, and the rung spawned pkgconf with
`(File, READ | WRITE)`. So every `lstat` returned `-1`, every search directory
was dropped, the search path came out **empty**, and pkgconf reported the only
thing an empty search path can report.

### Why this class is worth naming

**A program that treats a failed metadata probe as "absent" converts a missing
*capability* into a missing *file*.** The failure message then names the file —
which is present and correct — and says nothing about the capability, which is
the actual fault. Three separate wrong hypotheses (staging failed, `getenv` is
broken, the `.pc` path is wrong) all survive the evidence, and each costs a
boot cycle to test.

This is specific to a capability system. Under ambient authority `stat` on a
world-readable directory does not fail, so upstream's "cannot stat ⇒ not a
usable directory" is sound there and is nonsense here. It will therefore repeat:
any ported program that probes with `stat`/`access`/`lstat` and treats failure
as absence behaves the same way, and every one of them will report the symptom
in the vocabulary of files rather than rights.

Not a systemic risk for *real* userspace: `init` is spawned with
`(File, Rights::ALL)` (`kernel/src/main.rs`) and children inherit subsets, so
this bites hand-rolled self-test fixtures with deliberately narrow capability
lists — which is most of `spawn.rs`.

### The fix, and the general guard

1. The grant: `pkgconf_invoke` now passes `READ | WRITE | METADATA`, with the
   `lstat` mechanism written out beside it, because a reader who does not know
   it would see `METADATA` on a program that only reads files and delete it.
2. The guard: `pkgconf_diagnose` re-runs a failed case with `--debug`, which
   turns on pkgconf's own trace handler — it prints the search-path list it
   built and every `.pc` path it tried, to stderr, which the rung already maps
   to the console. It runs only after an assertion has failed, so it costs a
   green boot nothing, and it would have answered this question from the first
   failing log instead of the third.
3. The precondition, recorded: the rung now logs each staged `.pc` with its byte
   count and `lmetadata`s the search directory — the kernel side of the exact
   call pkgconf makes — so "the directory exists and stats as a directory" is
   evidence in the log rather than an assumption in the reader's head.

### What to do when you see it again

If a ported program reports something *absent* that you can see is present, the
first thing to check is not the path — it is whether the program stats before it
opens, and whether the child holds `(File, METADATA)`. `grep -n "Rights::READ | Rights::WRITE"`
over `kernel/src/proc/spawn.rs` lists the fixtures that cannot stat.
