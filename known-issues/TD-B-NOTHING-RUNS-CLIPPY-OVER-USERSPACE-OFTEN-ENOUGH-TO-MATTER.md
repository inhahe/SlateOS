## TD-B-NOTHING-RUNS-CLIPPY-OVER-USERSPACE-OFTEN-ENOUGH-TO-MATTER — OPEN 2026-09-03 (lane B)

**In short:** the project has a lint gate that covers all 2,700-odd userspace
crates, and it works. It is just so expensive that nobody runs it, so a crate
can sit in the tree failing it for days and no one finds out. That is what
happened to `notimpl`: it landed on 2026-09-02, it failed the gate's own
command from the moment it landed, and the failure surfaced on 2026-09-03 only
because a *different* investigation happened to run that command by hand.

**Not the same as `TD-B-USERSPACE-CRATES-DO-NOT-INHERIT-THE-WORKSPACE-LINTS`,
and worth keeping separate.** That entry is about lints that are never
*configured*: 2,730 crates that never opted into the workspace's defensive
set. This one is about a lint that *was* configured and *did* fire.
`notimpl/src/lib.rs` opens with `#![deny(clippy::all)]`; the lint it broke,
`clippy::needless_doctest_main`, is in `clippy::all`; the error was there the
whole time. Fixing every crate's `Cargo.toml` does not help if nothing runs
the compiler afterwards.

**Where the two clippy gates are, and why neither caught it:**

| gate | scope | when it runs | verdict on `notimpl` |
|---|---|---|---|
| `scripts/pre-boot.py` | `cargo clippy -p kernel` | every push (pre-push hook) | never looked — `-p kernel` |
| `scripts/boot-test.sh` → `check_cfg_unix` | `cargo clippy --workspace --target x86_64-unknown-linux-gnu` | before a merge to `main` | would have failed, had it been run |

The second gate is correct and comprehensive. It is also ~1,500 s on a warm
cache and part of a boot test that takes far longer, which is why in practice
it runs at merge time and not at change time. A gate whose cadence is "before
a merge" cannot tell you *which* change broke it, and by then the broken crate
may not be yours.

**Reproduce:**

```
cargo clippy -p notimpl --target x86_64-unknown-linux-gnu --message-format=short
```

against `534e4d63f..570d9375a^`. 45 s, and it is one error.

**The proper fix is a cheap workspace clippy in `pre-boot.py`,** beside the
`-p kernel` one — cheap because the pre-push hook already pays for a workspace
`cargo check` over the same triple, and clippy is that same front end plus
lints. The measured full pass is 1,513 s cold; the incremental pass after a
one-line change is tens of seconds, which is the number that matters for a
per-push gate.

`pre-boot.py` is lane A's file, so this is asked for rather than done:
`requests/b-a-the-cfg-unix-gate-skips-every-test-module.md`, whose last section
carries this finding as the argument for it. That request's main subject is a
different hole in the same gate (`--all-targets`), and the two want the same
edit, so they are filed together rather than as two requests that conflict in
the same three lines.

**If it is never fixed:** nothing degrades on its own, but every merge to
`main` becomes a lottery — `boot-test.sh` goes red on somebody else's crate,
in a lane that may not be the one merging, and the person holding the merge
has to bisect a gate they did not break. The exposure is proportional to how
many crates exist, and that number only goes up.
