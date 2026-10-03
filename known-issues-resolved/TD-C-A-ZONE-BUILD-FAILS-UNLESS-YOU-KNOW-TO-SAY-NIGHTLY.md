## TD-C-A-ZONE-BUILD-FAILS-UNLESS-YOU-KNOW-TO-SAY-NIGHTLY (lane C, 2026-08-22) -- CLOSED 2026-09-13 for every zone lane C owns

**In short:** Every zone's `.cargo/config.toml` tells you to build by running
`cargo build` from inside the zone directory. Do exactly that and you get
`error: `.json` target specs require -Zjson-target-spec` — a message naming a
flag that, per `CLAUDE.md`, you are specifically told *not* to pass. The missing
word is `+nightly`. Nothing is misconfigured; the instructions are incomplete.

**Reproduce** (any zone — checked in `apps/`, `gui/` and `userspace/`):

```
cd gui/window && cargo build -p oswindow
error: `.json` target specs require -Zjson-target-spec

cd gui/window && cargo +nightly build -p oswindow
    Finished `dev` profile ... in 5.38s
```

**Why.** The workspace-root `.cargo/config.toml` sets
`[unstable] json-target-spec = true` (`:54`), which is what makes
`build.target = "../toolchain/x86_64-slateos.json"` in each zone config legal.
But **cargo on a stable toolchain ignores the entire `[unstable]` table**, without
warning. The default toolchain in this checkout is
`stable-x86_64-pc-windows-gnu`, so `json-target-spec` never takes effect and the
`.json` spec is rejected. `build-std` in the same table is silently dropped for
the same reason — the `.json` error just happens to fire first.

**Why the error message is actively misleading.** It says the spec "requires
-Zjson-target-spec", so the obvious next move is to pass `-Zjson-target-spec` on
the command line. `CLAUDE.md` line 41 says in as many words that this is *not*
how it is set ("NOT via env var or CLI flag"). Both statements are true and
neither mentions the toolchain, so the reader is left with a contradiction and no
route out. The route out is one word.

**What was fixed.** The header comments of `apps/.cargo/config.toml` and
`gui/.cargo/config.toml` — the two zones lane C owns — now state the `+nightly`
requirement, quote the exact error, and say plainly that the fix is not a `-Z`
flag. `gui/`'s header also said *"Zone config for apps/"*, copied verbatim from
`apps/`, and now names its own zone.

**Update 2026-09-13 (lane C) — `net/` is done, and it was the last zone this
lane owns.** Its config carried the copied header twice over: the first line
said *"Zone config for apps/ — userspace GUI/CLI applications"* and the merge
paragraph said *"from inside apps/<name>/"*. Both now name `net/`, and the
`+nightly` paragraph is there.

**Checked in this zone rather than assumed from the sibling configs**, which is
the whole lesson of the entry above — the two other zones were fixed by copying
a paragraph, and a paragraph copied once is a paragraph that can be copied
wrong. From `net/`: `cargo build` gives exactly

```
error: `.json` target specs require -Zjson-target-spec
```

and `cargo +nightly build` finishes in 3 m 59 s, rebuilding std. Both numbers
are in the header now.

**What is left is not lane C's and is not worth a request file:**

- `CLAUDE.md` line 41 should say "and only on a nightly toolchain". That file
  may be edited only on an explicit operator instruction, so it waits for one.
  It is deliberately *not* in `open-questions.md`: that queue is for decisions,
  this is a five-word correction with an obvious answer, and padding the queue
  with items that need no thought is how a reader learns to skim it.
- The workspace-root `.cargo/config.toml:62` `build-slateos` alias comment
  already *shows* `cargo +nightly build-slateos` in its example without saying
  why the `+nightly` is load-bearing. Minor, recorded here rather than filed:
  the example is correct as it stands, and a cross-lane request for one
  clarifying clause costs both lanes more than the clause is worth.

**Still open, and why lane C did not do it:**

- `userspace/.cargo/config.toml`, `net/.cargo/config.toml` and
  `init/.cargo/config.toml` carry the identical incomplete header. Those are
  lanes A and B's trees.

  **Lane B, 2026-08-22: `userspace/` and `init/` are done** — both now carry
  the `+nightly` paragraph, and both had *also* inherited lane C's original
  first line verbatim ("Zone config for apps/ — userspace GUI/CLI
  applications"), which now names the right zone. `net/` is untouched; per the
  lane table in `CLAUDE.md` it is lane C's, not lane A's.

  Lane B lost exactly the ten minutes this entry predicts, converting `mv`.
  The cost is worse in `userspace/` than elsewhere and the fixed header now
  says so: the target's family is `unix` and the development host is Windows,
  so `cargo test` on the host never compiles the `#[cfg(unix)]` arm of
  anything in that tree. A zone build is not a convenience there — it is the
  only thing that type-checks half the source, and it is the half that runs in
  production. An agent who does not know the `+nightly` word therefore does not
  merely lose ten minutes; it concludes the target build is unavailable and
  commits code whose `#[cfg(unix)]` branches have never been compiled.
- `CLAUDE.md` line 41 should gain "and only on a nightly toolchain", and the
  `build-slateos` alias comment at `.cargo/config.toml:62` already shows
  `cargo +nightly build-slateos` in its example but does not say why the
  `+nightly` is load-bearing. `CLAUDE.md` may only be edited on an explicit
  operator instruction, and the workspace-root `.cargo/` is not lane C's.

**Alternative fix not taken:** adding `json-target-spec = true` to each zone
config would not help — it is the *toolchain*, not the config location, that
ignores it. A `rust-toolchain.toml` pinning nightly at the workspace root
**would** fix it properly and for every zone at once, and is the right answer if
the operator wants one; it is a workspace-root file and a decision with
consequences for every lane, so it is not lane C's to make unilaterally.

**Severity.** Low as a defect — nothing is broken and the workaround is one
word — but it costs every newcomer (human or session) the same ten minutes, and
it cost this one, which is why it is written down rather than remembered.
