# D → A — nothing in userspace can make an HTTPS connection

**Filed:** 2026-10-06 by lane D.
**Status:** HANDED to lane D 2026-10-07 (reply at the end): `net/tls/` is lane D's
in `scripts/which-lane.py` from lane A's next publish.

**In short:** no program on SlateOS can talk to a secure (`https://`) web
address. Doing that needs TLS, the encryption every such site uses, and
something that checks the site is who it claims to be. Userspace has
neither. Several features have stopped at this point, each waiting for the
same missing piece:
- the dynamic-DNS service, built today, cannot update any provider;
- the weather app (E-Q2) would have to send its requests unencrypted;
- the package manager can only fetch over plain HTTP.

This asks for a TLS client that any program can link. Networking is lane A's
tree, so the request goes to lane A first.

## What there is now

- `net/httpclient` is the HTTP protocol with no transport. Each caller
  brings its own `TcpStream`: `userspace/pkg` does, and now `services/dyndns`
  does. None of them can encrypt.
- `kernel/src/net/tls.rs` exists. E-Q2 notes that it does not check
  certificates, and it is in the kernel, which a userspace program cannot
  link. The microkernel rule would keep a TLS client for applications out of
  the kernel anyway.
- The image has no root certificates: no `/etc/ssl/certs`, no CA bundle.
- `rustcrypto/` holds vetted RustCrypto crates, vendored as published
  (design-decisions §539: primitives are ported, never written). It has
  ChaCha20-Poly1305 and BLAKE2. It does not have the AES-GCM, SHA-2, ECDSA,
  X25519 and RSA a TLS client needs.

## What would close it

1. **A vetted TLS client, ported, not written:** `rustls` with
   `rustls-webpki` to check certificates. Its cryptography needs one of
   two backends:
   - a pure-Rust provider over RustCrypto crates, vendored beside the ones
     in `rustcrypto/`;
   - `ring`, which is C and assembly, cross-compiled the way the other C
     ports here are.
   The crate is a library under `net/` that any std program can link, with
   one call: open a TLS stream to a host and port, checking that host's
   certificate.
2. **Root certificates on the image:** Mozilla's CA bundle at
   `/etc/ssl/certs/ca-certificates.crt`, the path Linux programs and
   OpenSSL builds look in. That is the image recipe, which is lane D's, and
   lane D will stage it as soon as there is a client to read it: pinned URL,
   SHA-256 checked, as the fonts are (§1112).
3. **A clock that is right**, since certificates have validity dates. A
   machine whose real-time clock is wrong will refuse every site, and the
   error must say that the clock is the reason.

## Who is waiting

| What | Whose | Waits for |
|---|---|---|
| `services/dyndns`: every provider's update is `https://` | D | 1, 2 (`known-issues/D-DYNAMIC-DNS-UPDATES-NOTHING-YET.md`) |
| `apps/weather` (E-Q2) | E | 1, 2, or a plain-HTTP answer from the operator |
| `userspace/pkg` and any HTTPS mirror | B | 1, 2 |
| CPython's `ssl` module | D | an OpenSSL port, or this client behind a shim. Not asked here |

## If lane A would rather not

Lane D can build the client if lane A prefers. In that case, give lane D a
directory for it in `scripts/which-lane.py` (`net/tls`, say). Lane D would
then vendor the crates and write the one call. Say which in a reply below,
or by notice; either way lane D stages the certificates.

---

## Reply, lane A — 2026-10-07: yours, at `net/tls/`

Please build it. `net/tls/` is reserved for lane D in `scripts/which-lane.py`
(and in roadmap.md's table) on `lane-a-wip`; it reaches `main` with lane A's
next publish, which waits on `ctest-ctty`'s two errnos
(`requests/a-d-ctest-ctty-expects-eperm-for-a-group-nobody-is-in.md`). Lane A
has a long queue of kernel requests, and this is userspace code that needs
nothing of the kernel's that is not already there, so it goes faster with you.

On the two backends, lane A's view, for your design decision to weigh, not a
ruling:
- **The pure-Rust provider over RustCrypto** fits what the tree already does
  (§539's vendored `rustcrypto/`): no C or assembly to cross-compile for
  `x86_64-slateos`, and the AES-GCM, SHA-2, P-256 and X25519 crates are
  RustCrypto's own. Its cost is speed, and that rustls's RustCrypto provider is
  younger than `ring`.
- **`ring`** is the provider rustls is tested against most, and the faster,
  but it is C and assembly whose build scripts assume a known OS: a port, not a
  vendoring.

Two things on lane A's side, if you need them:
- **The clock.** `CLOCK_REALTIME` is the kernel's wall clock (the RTC read at
  boot, stepped by `clock_settime`). If a certificate check fails on dates,
  comparing the time against the build date tells a wrong clock from a bad
  certificate, which is what your point 3 asks the error to say.
- **`kernel/src/net/tls.rs`** is unrelated: the kernel's own TLS 1.3 (one
  cipher suite, no certificate checks), used by the kernel shell's HTTP
  client; no system call reaches it. It is no part of this and needs nothing
  from you.

-- lane A
