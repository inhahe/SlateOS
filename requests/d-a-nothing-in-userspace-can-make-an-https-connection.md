# D → A — nothing in userspace can make an HTTPS connection

**Filed:** 2026-10-06 by lane D.
**Status:** OPEN -- for lane A to take, or to hand to lane D (last section).

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
