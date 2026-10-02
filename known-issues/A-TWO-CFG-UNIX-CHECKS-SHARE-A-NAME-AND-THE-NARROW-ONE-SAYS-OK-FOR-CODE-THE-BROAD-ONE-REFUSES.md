## A-TWO-CFG-UNIX-CHECKS-SHARE-A-NAME-AND-THE-NARROW-ONE-SAYS-OK-FOR-CODE-THE-BROAD-ONE-REFUSES (lane A, 2026-09-13)

**In short:** there are two checks called *cfg-unix*, with different scopes, and
the cheap one passing is easily read as evidence about the expensive one. It is
not. Today it told me a blocked tree was clear.

| | scope |
|---|---|
| `scripts/check-cfg-unix.py` | the **60** crates that contain `cfg(unix)` code |
| `boot-test.sh` `check_cfg_unix()` | `cargo clippy --workspace --exclude kernel --all-targets` -- **all 415** |

**How it misleads, concretely.** `apps/launcher` contains *zero* `cfg(unix)`, so
it is outside the script's population entirely. It nonetheless fails the boot's
check: a deny-level `field_reassign_with_default` inside its `#[cfg(test)]`
module, which `--all-targets` compiles. So:

```
python scripts/check-cfg-unix.py           -> OK (60 of 415 ...)
cargo clippy -p launcher --all-targets     -> error, rc 101
```

Both are correct about their own question. The script does not claim to cover
the workspace. But it is named for the gate, prints a reassuring total that
mentions 415, and is the obvious thing to run when the boot dies at
`=== Checking that every #[cfg(unix)] arm compiles and lints ===`.

**Nearly acted on it.** After merging 33 commits I re-ran the script, got OK,
and had begun concluding the blockage had cleared -- which would have meant
starting a run that dies 1895 s in at the same line. What stopped it was the
code being visibly unchanged: a check that stops complaining while the code it
complained about is byte-identical has changed its own scope, not the tree.

**Partly known already.** `bd542aed2` is titled *pre-push: the narrow cfg-unix
gate stops borrowing the broad one's name*, so the collision was recognised for
the hook. The boot-side function is still `check_cfg_unix` and the script is
still `check-cfg-unix.py`, so the confusion survives where it costs the most.

**The fix is a rename, and it is not lane A's to make unilaterally** --
`scripts/` ownership is A-Q11, still open. Recorded rather than done. Whoever
takes it: the narrow one wants a name about `cfg(unix)` *arms*, and the broad
one is really a whole-workspace unix-target lint, which is a different claim.
