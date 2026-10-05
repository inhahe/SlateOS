## TD-B-EVERY-CFG-UNIX-BLOCK-IN-USERSPACE-IS-UNVERIFIED-BY-THE-TEST-LOOP (lane B, 2026-09-10) -- FIXED the same day

**In short:** code inside `#[cfg(unix)]` is never compiled by the command this
project uses to test, so it can be broken -- not merely wrong, *uncompilable* --
and every test still passes.

**Why.** `cargo test --target x86_64-pc-windows-gnu` is the standing command in
`CLAUDE.md` and in every lane's loop. On that target `cfg(unix)` is false, so
the compiler never looks at those blocks. The real target,
`x86_64-slateos`, *is* unix, so they are exactly the blocks that run on the
machine and never on the test rig.

**Found while** fixing `su -`'s login-shell `argv[0]`. The fix is one
`cmd.arg0(...)` under `#[cfg(unix)]`; it compiled and tested clean on the host
without the compiler having read it once.

**It is checkable today, with no new tooling.**

```
cargo check -p su --target x86_64-unknown-linux-gnu
```

`x86_64-unknown-linux-gnu` is already installed on this machine
(`rustup target list --installed`). Nothing in the tree appears to run it.

**Who else is exposed.** Every `#[cfg(unix)]` block under `userspace/`,
including `userspace/sshd`'s `cmd.arg0(login_argv0(...))` -- the call that
proves the capability exists. Its own comment shows the author knew the host
build could not see it and reasoned carefully about the `dead_code` allow
instead, which is the best that could be done without a way to compile it.

**Fixed** as `scripts/check-cfg-unix.py`, pre-push gate 17, built the way the
paragraph below proposed: `cargo check` against `x86_64-unknown-linux-gnu`, with
the crate list **derived** by scanning for the attribute so that a file grows
into the gate by existing.

**How much code this was.** The derivation finds **57 crates**, and
`userspace/coreutils` alone holds **533** unix-gated blocks. All 57 compile
today -- the gate went in green, which is the only honest time to add one.

**Cost: about 7 seconds warm**, 15 cold, because it is one `cargo check` with
every `-p` rather than 57 invocations paying for the dependency graph each
time.

**`x86_64-unknown-linux-gnu`, not `x86_64-slateos`**, deliberately: the point is
to compile the unix branch, not to reproduce the target. slateos needs
`-Zbuild-std` and a built sysroot, which is minutes and a nightly, and would
make the gate too expensive to run on every push. Any unix target reads the
same lines.

**The self-test asserts the premise, not just the plumbing.** It compiles a
`#[cfg(unix)]` block containing a plain type error twice, and requires it to
**pass** on `x86_64-pc-windows-gnu` and **fail** on the unix target. If that
ever stops being true the gate is buying nothing, and the fixture says so in
those words -- which is the check that a gate for an invisible defect most
needs, since nothing else would notice it had become useless.

The original proposal, kept for the record:

**The proper fix is a gate**, and it is small: `cargo check` each crate that
contains `#[cfg(unix)]` against `x86_64-unknown-linux-gnu` in the pre-push hook,
with the crate list *derived* by grepping for the attribute rather than
enumerated. Not built in the same commit as the `su` fix because a gate that is
wrong about which crates to check is worse than no gate, and getting that right
is its own piece of work rather than a tail-end of this one.
