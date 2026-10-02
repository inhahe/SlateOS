## TD-B-THE-UNIX-HALF-OF-COREUTILS-IS-NEITHER-LINTED-NOR-TESTED-BY-DEFAULT (lane B, 2026-09-03) -- RESOLVED

**RESOLVED 2026-09-04.** All four steps landed. The checker exists (step 1),
pre-push gate 12 runs it (step 2), and its scope is no longer one crate but is
**computed from the push** (step 4): every crate this lane owns that a push
changes and that contains a `cfg(unix)`/`cfg(windows)` arm is compiled and
tested for `x86_64-unknown-linux-gnu` before the push is allowed.

Step 4 is worth reading even though it is closed, because **its own premise was
wrong and a measurement is what showed it.** It named `posix` and
`userspace/shell` as the crates to widen to; both have zero `cfg(unix)` arms,
so compiling them for Linux would have added 8m41s a push to type-check source
the host build had just type-checked — while 32 crates that *do* have such arms
were on nobody's list. The fix was to stop listing.

What remains unautomated is deliberate and named in step 4: crates outside this
lane's globs, and the *dependencies* of a checked crate (a dependency whose API
changes breaks the host build too, which everything else already catches).

**In short:** the coreutils crates are built, linted and tested against the
*Windows* host target, because that is the toolchain the machine has. Every
piece of code marked "only on unix" is invisible to that build — the compiler
never sees it, clippy never lints it, and the tests inside it never run. Since
unix is the half that actually resembles SlateOS, the half that ships is the
half nobody checks.

**Found by accident, twice in one change.** The `rm`/`dirfd` work
(`TD-B-RM-WALKS-BY-PATH-SO-A-SYMLINK-SWAP-CAN-REDIRECT-A-REMOVAL`) passed
`cargo test -p coreutils --target x86_64-pc-windows-gnu` (361 + 53 tests) and a
full 89-minute `cargo clippy --all-targets` on the same target with **zero**
warnings. Then `scripts/rm-diff.sh`, which builds for
`x86_64-unknown-linux-gnu` inside WSL as a side effect of what it is really
for, printed two warnings the host build could not have produced:

  * `unused import: os_from_bytes` — used only in the `#[cfg(not(unix))]` half,
    so it is genuinely unused on unix and genuinely used on the host;
  * `unused doc comment` on the `#[cfg(unix)] unsafe extern "C"` block, which
    the host build does not compile at all.

And when the same suite was run for real on the Linux target, it reported
**405** lib tests and **59** `rm` tests rather than 361 and 53. Forty-four lib
tests and six `rm` tests exist that the host run silently skips — including all
three of `dirfd`'s deliberate directory-swap refusals, which are the tests that
certify the security property the module was written for. They pass; the point
is that nothing in the normal workflow would have told anyone if they did not.

**Why the host target is used at all.** It is fast, it needs no WSL round trip,
and most of coreutils is portable. That is a real benefit and the answer is not
to abandon it.

**Proper fix**, roughly in order of value:

1. ~~A `scripts/coreutils-check.sh` that runs `cargo clippy` and `cargo test`
   for **both** `x86_64-pc-windows-gnu` and `x86_64-unknown-linux-gnu`.~~
   **LANDED 2026-09-03.** `scripts/coreutils-check.sh` does exactly that, into
   the harnesses' `~/.cache/slateos-diff-target`, so it costs no extra disk.
   `--only host|linux`, `--no-clippy`, `--no-test`, `-p PKG` and pass-through
   cargo arguments after `--`. It exits **2**, never 0, when a requested half
   could not run — the failure being guarded against here is precisely a check
   that did not happen reading as one that passed, so "no WSL on this host" is
   a decline and the summary names which halves actually ran.

   Two findings from building it, both worth knowing before running it:

   * The Linux half must run from the **workspace root**, not from
     `userspace/`. That zone's config sets `build-std = [… "panic_abort"]` for
     the SlateOS target, and asking for a host target underneath it dies with
     ``the crate `panic_abort` does not have the panic strategy `abort` `` — a
     message about a mismatch nobody asked for. The root config sets no
     `build-std`, which is why the zone was given its own.
   * WSL already had a Rust toolchain (the `*-diff.sh` harnesses need one), but
     it is invisible to `command -v cargo` even under `bash -lc`, because
     `~/.cargo/env` is sourced from `.bashrc` and a non-interactive login shell
     does not read it. Name `$HOME/.cargo/bin/cargo` explicitly, as the
     harnesses and this script do, rather than concluding there is no cargo.
2. ~~Wire the Linux side into the pre-push gates, or at least into the boot
   test's pre-build tooling suite, so a `#[cfg(unix)]` regression cannot be
   pushed.~~ **LANDED 2026-09-04** as pre-push **gate 12**, scoped to pushes
   that touch `userspace/coreutils` or the checker itself.

   **The cost this step warned about was wrong, and the correction is the
   reason it could land at all.** The "89 minutes" above is real but is a
   *host* `clippy --all-targets` figure, which builds the zone's integration
   tests and dev-dependencies. `coreutils-check.sh` scopes itself to
   `--lib --bins`, and `--only linux` measured **2m16s** against a warm shared
   target directory (clippy 1m12s, test 1m04s) — nearer 6 minutes against a
   cold or contended one. A guessed cost had blocked a gate for a day; measure
   before deferring on price.

   Two things about gate 12 that are not obvious from the others:

   * **It reads the working tree, and every other gate reads the commits being
     pushed.** It has no choice — cargo compiles files, not a revision, so
     there is no `--head` to pass. What makes that honest is that it first
     *establishes* that the working tree is the push (one ref, its sha equal to
     `HEAD`, no modified tracked file, no untracked file) and declines by name
     if it is not. Answering about the wrong tree is exactly the defect gate 7
     shipped; declining is the answer gate 7 should have given.
   * **It is `--may-skip`**, so a host without WSL declines loudly rather than
     refusing every push. That is why `coreutils-check.sh` moved its usage
     errors to exit **64**: sharing exit 2 with a genuine decline would let a
     future renamed flag read as "no WSL on this host" and skip forever.

   Behavioural coverage is `scripts/test-pre-push-unixhalf-gate.py`, which
   drives real pushes through the real hook against a stub checker and asserts
   on whether the checker was *invoked* — because a declined gate and a passing
   gate both allow the push, and only the invocation count tells them apart.
3. ~~Step 2 has landed, but the gate is scoped to `userspace/coreutils` and only
   fires on a push.~~ **Largely obsolete since step 4 (2026-09-04):** the gate
   now covers *any* crate in this lane that a push changes and that has a
   platform-conditional arm, so the manual run is no longer the only thing
   standing between an unchecked `cfg(unix)` arm and origin. It remains the way
   to iterate — a push is a slow edit-compile loop — so:

   ```sh
   scripts/coreutils-check.sh --only linux            # both halves by default
   scripts/coreutils-check.sh --only linux --no-clippy -- dirfd   # while iterating
   ```

   First measured run, 2026-09-03: the whole `coreutils` lib reports **416**
   passing tests on the Linux target, `dirfd` alone **24** — against 0 of
   either on the host, since `dirfd`'s unix arm does not compile there at all.
4. **Widen it past `coreutils`.** ~~Gate 12 compiles one crate because that is
   where the defect was found and because the price is per-crate; the argument
   for it applies unchanged to `posix`, `userspace/shell` and the support
   crates, which are equally host-only-checked today. What is missing is a
   measurement per crate rather than a design: the gate already takes `-p`, and
   widening it is a matter of deciding how many minutes a push may cost.~~ Do
   that with numbers, not with the instinct that blocked step 2 for a day.

   **DONE 2026-09-04, and the numbers refuted the step as written.** It asked
   for `posix` and `userspace/shell` by name. Neither belongs:

   | crate | `cfg(unix)`/`cfg(windows)` arms | linux half costs | tests there |
   |---|---:|---:|---:|
   | `userspace/coreutils` | 719 | 2m16s warm, ~6m cold | 405 (host: 361) |
   | `oils` + `cpio` + `stat` | 108 + 20 + 18 | 5m03s together | 4 binaries, clean |
   | `posix` | **0** | 6m16s | 20,652 |
   | `userspace/shell` | **0** | 2m25s | **0** |

   `posix`'s conditionals are real but they are `target_os = "none"` — 1,845 of
   them — which is false on the Windows host and false on Linux alike, and is
   already compiled by the default `x86_64-unknown-none` build. `userspace/shell`
   is a 159-line toolchain-validation stub, not a shell. Compiling either for
   Linux type-checks precisely the source the host build already type-checked,
   for 8m41s a push. The step's premise — "the argument applies unchanged" —
   was false; the argument is *entirely* about `cfg(unix)` arms, and those two
   crates have none.

   Meanwhile the same census found **32 other crates in this lane that do have
   such arms and were on nobody's list**: `oils` 108, `cpio` 20, `stat` 18,
   `htop` 9, then `wall`/`vi`/`tput`/`tar`/`stty`/`pkg`/`nano`/`mktemp` 6 each,
   `ssh-keygen`/`man`/`getopt`/`authlib` 5, `sftp`/`service`/`rsync`/`quoting`/
   `less` 4, `sshd`/`chown`/`backup` 3, `scp`/`notimpl`/`lsof`/`logind`/
   `localtime`/`firejail`/`du` 2, `file` 1.

   So the answer was not a longer list — a list is wrong the day it is written
   and rots from there. **Gate 12's scope is now computed** from the crates a
   push changed, minus those with no platform-conditional arm at all, the
   filter being a `git grep` against the pushed commit. It costs nothing on a
   push that touches no such crate, cannot go stale, and reached `stat` and
   `oils` without anyone naming them. See `design-decisions.md` §767 and the
   five new cases in `scripts/test-pre-push-unixhalf-gate.py`.

**If it is never fixed:** warnings and dead code accumulate in the unix half
where nobody sees them, and — much worse — a unix-only test can rot into a
permanent silent skip. A test that never runs is indistinguishable from a test
that passes, which is the same failure shape as
`TD-B-PRE-PUSH-GATES-2-6-8-11-JUDGE-THE-WORKING-TREE-NOT-THE-PUSH`: a green
report produced by a check that was not performed.
