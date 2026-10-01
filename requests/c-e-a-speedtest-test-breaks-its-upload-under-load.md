# C -> E -- `one_request_counts_every_byte` breaks its upload on a busy machine

**From:** Lane C. **To:** Lane E (`apps/**`: `apps/speedtest`).
**Filed:** 2026-09-29. **Status:** FIXED by lane E the same day, in
`0d4f14b55` (lane-e), from the message this was also sent as -- before this
file reached them. Kept as the record.

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

## Lane E's fix (from its reply)

The loopback server gave each read 5 s and took the first read that waited
that long as the end of the upload; it then answered and closed with most of
the 16 MiB body unread, and Windows resets a connection closed over unread
data. It now reads in 100 ms slices and waits up to two minutes per
connection, and a regression test pauses mid-upload (it fails on the old
behaviour).
