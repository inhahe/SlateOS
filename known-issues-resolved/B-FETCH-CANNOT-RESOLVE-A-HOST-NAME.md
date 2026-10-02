### B-FETCH-CANNOT-RESOLVE-A-HOST-NAME. `fetch` could reach a literal IP address and nothing else — 2026-08-30 — FIXED 2026-08-30 (lane B)

**Where:** `net::Connection::connect`, `userspace/coreutils/src/bin/fetch.rs`.
One call site, in `fetch_url`.

**Symptom.** Every URL naming a host — which is every URL anyone would type —
failed before a packet was sent:

```
$ fetch -I http://example.com/
fetch: connection to example.com:80 failed: bad address: invalid socket address syntax
$ echo $?
2
```

The utility's entire stated purpose is to fetch a URL, so this was not a
corner: `fetch` worked only when handed a numeric address, and no test or
document mentioned that restriction.

**Cause.** The connect path never resolved anything. It read:

```rust
let addr: SocketAddr = format!("{host}:{port}")
    .parse()
    .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "bad address"))?;
```

`str::parse::<SocketAddr>` is *not* a resolver — it parses the textual form of
an address that is already numeric. `"93.184.215.14:80"` parses;
`"example.com:80"` does not, and the `InvalidInput` it returns was then
reported as the network having a bad address rather than as the program never
having asked the network anything.

**Why it lasted.** Every test in the file exercised URL *parsing*
(`ParsedUrl::parse`), header parsing, chunk decoding and redirect resolution —
all of which are pure functions over bytes and all of which were correct. The
one step that is not pure, and so is not unit-testable without a network, was
also the one step that was wrong. Nothing in the crate ever ran the binary
against a server.

**Fix.** `to_socket_addrs()`, which is the resolver, and then *every* returned
address is tried rather than the first, because a dual-stack name commonly
resolves to an AAAA record this machine has no route to. Resolution failure and
connection failure are now separate arms of a `ConnectError`, because they want
different diagnostics: a name that does not resolve gets one wording on all
platforms (`fetch: could not resolve host: <host>`), since the host's own text
for it is not an errno and is not portable — Windows says `No such host is
known.` where glibc says `failed to lookup address information: Name or service
not known`, and `coreutils::errmsg::strerror` can normalise neither. A refused
or unreachable *address* is a real errno and is reported exactly.

Measured after the fix, against the live network:

| command | result |
|---|---|
| `fetch -I http://example.com/` | `HTTP 200 OK`, headers, `rc=0` |
| `fetch -L -I http://google.com/` | follows to `http://www.google.com/`, `rc=0` |
| `fetch http://no-such-host-xyzzy.invalid/` | `fetch: could not resolve host: …`, `rc=2` |
| `fetch http://127.0.0.1:9/` | `fetch: connection to 127.0.0.1:9 failed: Connection refused`, `rc=2` |
| `fetch -q http://httpbin.org/stream/3` | 3 lines (chunked decoding) |
| `fetch -L -q http://httpbin.org/redirect/12` | `fetch: too many redirects (max 10)`, `rc=1` |

**Known limitation, deliberate:** `--timeout` bounds each `connect` and each
later read and write, but **not** the name lookup — `to_socket_addrs` is the
platform resolver and offers no deadline. A resolver that hangs hangs us. The
fix is a resolver of our own, which this OS will need anyway once `net/` serves
userspace; it is not worth a thread-per-lookup workaround in one utility.
