## Two process bugs this round, both about trusting a green result

### `cargo test --workspace` is *not* finished when `rustc.exe` hits zero

Recorded earlier this session as a working rule: once the gate's `rustc`
process count reaches 0 it is running test binaries, so source edits can no
longer change its verdict. **That rule is wrong**, and it cost a gate.

Doctests run last, and `rustdoc` compiles them from the **live source** at that
moment — not from a snapshot taken when the crate was built. Editing
`gui/toolkit/src/lib.rs` mid-gate to add `pub use textfmt::{..., fold, ...}`
made the gate fail with `unresolved import textfmt::fold`, against a `textfmt`
rlib built minutes earlier that genuinely had no `fold`. Neither the committed
tree nor the final tree has that problem; the failure existed only inside the
gate's window.

`rustdoc.exe` is also not `rustc.exe`, so polling `ps -W | grep -c rustc.exe`
reports 0 during the entire doctest phase and looks like "tests are running."

The rule is simply: **do not edit the tree while a gate is running.** If there
is nothing else to do, do read-only work.

### A trailing `tail` swallows the exit code the notification reports

The gate was launched as:

```
cmd > /tmp/gate2.log 2>&1; echo "EXIT=$?"; grep -c ...; tail -3 /tmp/gate2.log
```

The background-task completion notification reported **exit code 0**, and it
was `tail`'s exit code. The gate itself had failed. The `echo "EXIT=$?"` line
does capture the real code, but it is buried in the output rather than being
the thing the harness reports, so the notification actively misleads.

Either make the command under test the **last** command in the chain, or chain
with `&&` so a failure propagates. Do not put a diagnostic after it and then
believe the notification.
