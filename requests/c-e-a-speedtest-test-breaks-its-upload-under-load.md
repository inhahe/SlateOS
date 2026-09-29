# C -> E -- `one_request_counts_every_byte` breaks its upload on a busy machine

**From:** Lane C. **To:** Lane E (`apps/**`: `apps/speedtest`).
**Filed:** 2026-09-29. **Status:** OPEN -- not urgent; it fails a busy
workspace gate now and then, and nothing waits on it. (Also sent to Lane E's
session as a message the same day.)

**In short:** one of speedtest's network tests uploads to a server on the same
machine. Once, with the machine busy, the server end cut the connection before
the upload finished, and the test failed. Run alone it passes, so the test --
not the program -- depends on timing.

## What was seen

Lane C's workspace gate (`cargo test --workspace --exclude kernel --target
x86_64-pc-windows-gnu --no-fail-fast`, run at low priority beside other
builds) on `77041bb5f`, whose `apps/` is `origin/main`'s:

```
thread 'net::tests::one_request_counts_every_byte' panicked at apps\speedtest\src\net.rs:682:47:
called `Result::unwrap()` on an `Err` value: "the upload broke: An existing connection was forcibly closed by the remote host. (os error 10054)"
```

The same test, alone, on the same tree: `ok`, in 0.34 s.

## A guess at the cause

Error 10054 is a reset: the loopback server closed its socket -- or dropped out
of the accept loop -- with bytes of the upload still unread, which on Windows
turns the close into a reset the client sees mid-write. A server that reads the
request to its end (or shuts down its write side and drains) before closing
would not do that, however slowly either side is scheduled.
