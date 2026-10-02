### TOOL-DIFF-WSL-LIB-FRESHNESS-CHECK-IS-CURRENTLY-INERT. `diff_lib_artifacts` cannot tell "this package has no library" from "this package's library is gone", and today it answers the second with the first — 2026-08-25 — FIXED

> **Correction, same day, before the fix landed.** This entry was filed with the
> right symptom and the wrong cause. It blamed the missing `deps/` directory on
> "the second occurrence" of an unexplained 2026-08-24 deletion. There was no
> deletion: **cargo 1.100.0-nightly (e8cb624d5 2026-08-22) does not create
> `deps/`**, and the original text is kept below with the correction marked, so
> that the reasoning error stays visible rather than being quietly overwritten.
> The corrected diagnosis, the evidence, and the fix as shipped are at the end.

**In short:** the coreutils harnesses build their subject and then check that the
build really happened. Part of that check — the part added specifically because
a stale library once made a harness report sixteen differences against a fix
that was correct and in the tree — is silently doing nothing at the moment. It
looks for a file inside a directory that does not exist, finds nothing, and
reads "nothing" as "nothing to worry about."

**Where:** `scripts/diff-wsl.sh`, `diff_lib_artifacts` (~line 297) and its caller
`diff_first_stale` (~line 336).

```sh
diff_lib_artifacts() {
  for diff_p in $DIFF_PKG; do
    ls -t "$target_dir/x86_64-unknown-linux-gnu/debug/deps/lib${diff_p}-"*.rlib \
      2>/dev/null | head -1
  done
}
```

**Two independent faults, same shape.**

*Fault 1 — the name is guessed from the package, not read from the target.*
`lib${diff_p}` assumes a package's library artifact is named after the package.
That holds only when `Cargo.toml` has no `[lib] name`:

| package | `[lib] name` | actual artifact | guessed | matches |
|---|---|---|---|---|
| `coreutils` | absent | `libcoreutils-*.rlib` | `libcoreutils-*` | yes |
| `dc` | no library at all | — | `libdc-*` | vacuously |
| `oils` | `osh` | `libosh-*.rlib` | `liboils-*` | **no** |

`oils` is the first package a harness has pointed `DIFF_PKG` at whose library is
renamed, and it arrived today with `scripts/osh-diff.sh`. Left alone, that
harness would carry a freshness check that can never fire.

*Fault 2 — a missing artifact is indistinguishable from an absent one.* `ls …
2>/dev/null` prints nothing both when the package legitimately has no library
(`dc`) and when the library the build was supposed to produce is not there. Only
the second is a problem, and it is exactly the problem the function was written
for. Measured 2026-08-25, twice, once with a `cargo build` actively running
against the directory:

```
$ ls -d $HOME/.cache/slateos-diff-target/x86_64-unknown-linux-gnu/debug/*/
build/  examples/  incremental/
deps exists? NO      fingerprint exists? NO
```

So `deps/` is gone — and with it every `*.rlib` the check reads — while `debug/`
still holds 40-odd finished binaries (they survive as hardlinks when the
`deps/` names are removed). That is verbatim the state `diff-wsl.sh`'s own
comment records from 2026-08-24: *"a `debug/` full of finished binaries and no
`deps/` at all, so something had removed the intermediates and left the
fingerprints."* This is its second occurrence, and the cause is still not known.
It is not disk space: the WSL root had 892 GiB free.

> ~~**CORRECTED.**~~ Everything in the paragraph above after the first sentence
> is wrong. `deps/` was never "gone"; this toolchain never makes one. See
> **The corrected diagnosis** below.

**Why it matters that the detector is the thing that broke.** The check exists
because a green harness is the only evidence anyone reads, so a check that
cannot fail is worse than no check — it is a green light wired to nothing.
Both faults produce silence, and silence is this function's success signal.

**What the proper fix looks like.** Do not add a knob for the harness author to
set, because a knob is a second copy of a fact the tree already states, and a
second copy is the one that drifts. Read it instead:

1. For each package in `DIFF_PKG`, take the library target's name from
   `userspace/<pkg>/Cargo.toml` — the `name` under `[lib]`, falling back to the
   package name — so `oils` resolves to `osh` without anyone saying so.
2. Decide whether a library is *expected* from whether `userspace/<pkg>/src/lib.rs`
   exists. `dc` has none, so nothing is expected of it; `coreutils` and `oils`
   both do.
3. When one is expected and no artifact is found, report it as stale with the
   existing `<the build left nothing here>` wording, which already routes into
   `cargo clean -p` + rebuild + refuse. That path is correct; it is only never
   reached.

**Consolation, and the reason this is not urgent.** With `.fingerprint/` gone,
cargo cannot judge anything fresh, so every harness in a sweep is currently
doing a full from-scratch rebuild. Results gathered in this state are the
*freshest* possible, not the stalest — the bug's cost is a check that would not
warn if the directory returned to a half-populated state, not a wrong answer
today. That is also why the sweep takes about a minute per harness.

> ~~**CORRECTED.**~~ Also wrong, and wrong in the comforting direction, which is
> why it is worth keeping. Cargo judges *everything* fresh: `cargo build -v`
> reports all 12 crates `Fresh` and 0 `Compiling`. Nothing is being rebuilt from
> scratch, so the "results are the freshest possible" consolation was unearned —
> had the library genuinely been stale, this entry would have talked the reader
> out of worrying about it. The minute per harness is 9p/DrvFs stat traffic
> against `/mnt/d`, not compilation.

**Risk of leaving it.** The 2026-08-24 incident cost a day of chasing sixteen
imaginary differences in `interleave-diff.sh`. The check that was written so it
could not happen twice is, as of today, not running.
