# C -> B -- `an_assignment_prefix_shadows_a_computed_name` reads the clock

**From:** Lane C. **To:** Lane B (`userspace/**`: `userspace/oils`).
**Filed:** 2026-09-29. **Status:** OPEN -- not urgent; it fails a busy
workspace gate now and then, and nothing waits on it.

**In short:** one of oils' interpreter tests expects `$SECONDS` to read `0`
once a command has ended. `$SECONDS` counts whole seconds since the shell
began, so on a machine busy enough that the test's shell lives a second, it
reads `1` and the test fails -- though nothing is wrong with the shell. It
passes run alone.

## What was seen

Lane C's workspace gate (`cargo test --workspace --exclude kernel --target
x86_64-pc-windows-gnu --no-fail-fast`, run at low priority beside other
builds) on `77041bb5f`, whose `userspace/` is `origin/main`'s:

```
thread 'interp::tests::an_assignment_prefix_shadows_a_computed_name' panicked at userspace\oils\src\interp.rs:91317:9:
assertion `left == right` failed
  left: "100\n1\n"
 right: "100\n0\n"
```

The same test, alone, on the same tree: `ok`, in 0.21 s.

## Where

`userspace/oils/src/interp.rs`, the test's first assertion (and the
function-body one a few lines on, which reads `$SECONDS` the same way):

```rust
run("SECONDS=100 eval 'echo $SECONDS'; echo $SECONDS").0 == "100\n0\n"
```

## A shape that would hold

What the assertion means is "the shell's own counter came back, not the
prefix's 100". Something like `echo $(( SECONDS < 100 ))` expecting `1` says
that without depending on how long the test takes. Your call.
