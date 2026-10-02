## The corrected diagnosis — 2026-08-25

**In short:** the check looked for the library in `debug/deps/`, under a name
built from the package name. Both halves were wrong at once. The folder is
wrong because the current cargo puts libraries somewhere else entirely, and the
name is wrong for `oils`, whose library is called `osh`. Two wrong inputs, one
silent "found nothing", and a check that reported everything fine.

**`deps/` was not deleted; this cargo does not create it.** Established by
measurement, in this order, each step ruling out the previous explanation:

| Step | Result |
|---|---|
| `CARGO_*` env overriding the layout? | none set |
| `~/.cargo/config.toml`, or one in a parent dir? | neither exists |
| Does `deps/` appear *during* a build? (polled every 4 s for 45 s) | never appears |
| `cargo build -v` on the repo | 12 × `Fresh`, 0 × `Compiling` |
| **Clean `cargo build` of a fresh hello-world** | produces `build/ examples/ incremental/ <bin> <bin>.d` — **no `deps/`, no `.fingerprint/`** |
| `cargo build -v` on that hello-world | `--out-dir …/target/debug/build/layoutprobe/<hash>/out` |

The last line is the whole answer. Cargo `1.100.0-nightly (e8cb624d5 2026-08-22)`
emits units to `debug/build/<pkg>/<hash>/out/`, so a library artifact lives at
`debug/build/<pkg>/<hash>/out/lib<libname>-<hash>.rlib`. Confirmed in the shared
target dir:

```
debug/build/coreutils/a051f900598d42ca/out/libcoreutils-a051f900598d42ca.rlib
debug/build/coreutils/49c74fa47cb46b3f/out/libcoreutils-49c74fa47cb46b3f.rlib
debug/build/oils/d51545b5b76655c2/out/libosh-d51545b5b76655c2.rlib
```

It also retires the 2026-08-24 mystery as a mystery: the "no `deps/` at all"
observation that was read as evidence of a deletion carried no information,
because there was never going to be one. *What made cargo call a stale library
fresh that day is still unexplained* — the correction removes a wrong answer, it
does not supply a right one. The freshness check is the response to that, and it
fires on the symptom, which is all anyone gets.

**A near-miss worth recording.** The fix makes "a library was expected and the
build produced none" fatal. Written against the belief that `deps/` had been
deleted, the natural next step was to assume a rebuild would restore it — and
shipping on that assumption would have made all 46 harnesses refuse to run,
since no `deps/*.rlib` was ever going to exist again. What prevented it was
holding the fix back for one decisive experiment (the hello-world build) instead
of acting on a plausible story. A wrong diagnosis that predicts the same symptom
is only distinguishable by an experiment that *could* have come out the other
way.

**Fixed as shipped.** `diff_lib_artifacts` is replaced by three functions in
`scripts/diff-wsl.sh`:

- `diff_manifest` — finds `<pkg>/Cargo.toml` with a *one-level* glob
  (`$root/*/<pkg>/Cargo.toml`). Not a recursive search: `userspace/` holds
  several thousand package directories and walking it costs minutes, far more
  than the check it serves.
- `diff_lib_name` — applies cargo's own two rules, in cargo's order: an explicit
  `[lib] name` wins; failing that a package has a library iff `src/lib.rs`
  exists, named after the package with dashes turned to underscores. Read from
  the manifest rather than taken as a harness knob, because a knob is a second
  copy of a fact the tree already states, and `oils` is the standing proof that
  the copy nobody rereads is the one that goes wrong.
- `diff_lib_artifact` — **searches** beneath `debug/` for `lib<libname>-*.rlib`
  and takes the newest, rather than asserting a directory. Returns 0 with a path
  when found, 0 with no output when the package has no library, 1 when a library
  is expected and absent, and 2 when the package has no manifest at all. The
  last two both route into the existing `cargo clean -p` + rebuild + refuse path,
  which was always correct and merely never reached.

Searching rather than naming is the actual lesson: a layout the next toolchain
invents costs nothing as long as the file keeps its name, and if it ever does
stop being found, the answer is now a refusal rather than a silent pass.

Verified against all four cases before shipping — `coreutils` (implicit lib,
newest of two hashes), `dc` (no library, correctly exempt), `oils` (`libosh`,
the one the old code could never have matched), and a nonexistent package
(rc=2 rather than a silent skip) — then end-to-end: `wc-diff.sh` 116 passed,
0 differed.
