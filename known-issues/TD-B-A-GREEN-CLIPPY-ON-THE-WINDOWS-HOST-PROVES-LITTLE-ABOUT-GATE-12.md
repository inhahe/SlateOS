## TD-B-A-GREEN-CLIPPY-ON-THE-WINDOWS-HOST-PROVES-LITTLE-ABOUT-GATE-12 (lane B)

**Status:** OPEN — 2026-09-05

**In short:** Before pushing, we run `cargo clippy` on this machine and take a
clean run as evidence the push will pass. It is not evidence. The clippy that
runs here is **months older** than the one the push gate runs inside WSL, so
the gate rejects code the local run called clean. Nothing is broken by this; it
just means a failed push is the *first* time you learn, and on a crate the gate
has to compile from cold that costs about seventeen minutes per attempt.

**How to see it.** Three clippies, not two — the host has both channels
installed and neither is current:

```
$ cargo clippy --version                  # host default: STABLE
clippy 0.1.95 (59807616e1 2026-04-14)

$ cargo +nightly clippy --version         # host nightly
clippy 0.1.97 (d3cd04068e 2026-05-16)

$ wsl -e sh -c '$HOME/.cargo/bin/cargo +nightly clippy --version'
clippy 0.1.100 (a69a63265c 2026-09-03)    # what gate 12 runs
```

Two separate things are in play and it is worth keeping them apart, because
the obvious remedy only addresses one of them. The host's *default* toolchain
is **stable**, while the gate's is **nightly** — a channel difference. And the
host's whole rustup installation was last updated 2026-05-16, while WSL's was
updated 2026-09-03 — an age difference. So saying `cargo +nightly` locally,
which is the fix for the *other* nightly trap in this file
(`TD-C-A-ZONE-BUILD-FAILS-UNLESS-YOU-KNOW-TO-SAY-NIGHTLY`), closes the channel
gap and leaves three and a half months of the age gap wide open. It narrows
the blind spot; it does not remove it.

**Where it bit.** Pushing the sshd interactive-session work on 2026-09-05. A
local `cargo clippy -p sshd --all-targets --target x86_64-pc-windows-gnu`
exited 0 with zero warnings. Gate 12 then failed the push on
`clippy::needless_late_init` at `userspace/sshd/src/main.rs:1583` — a lint on
**platform-independent** code (`hmac_sha256`), in a function nobody had touched
in that change. The older clippy simply does not raise it. That is the
important half: this is *not* only about `#[cfg(unix)]` arms the host never
compiles, which is the failure mode gate 12 was built for and which everyone
already expects. A lint-version skew rejects code that has no conditional
compilation in it at all, so "I only changed portable code, the local run is
enough" is exactly the wrong inference.

**Why it is this way.** The two rustup installations live in two operating
systems, were updated at different times, and nothing pins them together. Note
what a `rust-toolchain.toml` would and would not buy: it would pin the
*channel*, fixing the stable-vs-nightly half automatically for anyone who
forgets `+nightly` — worth having on its own merits — but a channel is not a
date, so it would leave the three-and-a-half-month age gap exactly where it is.

**What the proper fix is.** Move both installations onto one dated nightly and
thereafter update them together, pinned. **This is deliberately not being done
as part of this task**, for two reasons that are about blast radius rather than
effort. A rustup update invalidates every lane's `target/` cache at once, so
all three lanes pay a full cold rebuild at a moment none of them chose. And
lane A's kernel builds against a custom target JSON via `-Zjson-target-spec` —
an explicitly unstable interface, of exactly the kind a multi-month nightly jump
is entitled to change underneath it. Landing that from a userspace commit,
while two other lanes have uncommitted work in flight, is how one lane's
convenience becomes three lanes' morning. It should be done by whoever can
watch all three trees, deliberately.

**Workaround until then** — the gate tells you this itself, but it is worth
having in one place, because the useful time to read it is *before* the push:

```
bash scripts/coreutils-check.sh --only linux --dir userspace/<crate>
```

runs exactly what gate 12 runs, on the warm WSL target cache, and takes a
fraction of a push. Run it on any crate you changed that the gate will pick up.

**Cost while unfixed:** one wasted push cycle per surprise, paid by whichever
lane touches a crate gate 12 compiles. Nothing reaches `origin` broken — the
gate is doing its job, and doing it is the only reason this was ever noticed.
The cost is purely the length of the feedback loop.

**If never fixed:** the gap widens. Every month the host and the gate diverge
further, so the local run's predictive value keeps falling and the share of
pushes that fail on a lint nobody could have seen locally keeps rising.
