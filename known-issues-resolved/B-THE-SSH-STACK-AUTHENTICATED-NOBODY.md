## B-THE-SSH-STACK-AUTHENTICATED-NOBODY — 2026-08-21 — FIXED

**In short:** SlateOS's SSH server, SSH client and FTP server all performed
authentication that could not fail. The SSH server let anyone log in who knew
a user's *public* key — which is public, travels in the clear, and sits
world-readable in `authorized_keys`. The SSH client never checked that the
server it reached was the server it asked for. The FTP server did not read the
password at all. Each of these is a full remote authentication bypass on its
own, and all three shipped together, so a user of this tree who ran `sshd` on
a network was running an open door with a lock painted on it.

Nine defects across three programs, found in one pass — seven in the SSH key
exchange and public-key path, two in password checking. They are listed
together because they are one failure, not nine: at no point did anything in
the SSH stack do the arithmetic that authentication *is*. Every place a
signature or a hash was supposed to be checked, something cheaper was checked
instead, and a comment explained that this was a simplification.

### The seven in the key exchange and public-key path

| # | Where | What it did | Effect |
|---|---|---|---|
| 1 | `sshd` `handle_pubkey_auth` | Matched the offered public key against `authorized_keys` and returned success **without reading the signature** | **Remote auth bypass.** Anyone who had seen the user connect, or could read `~/.ssh/id_ed25519.pub`, could log in as them |
| 2 | `sshd` `HostKey::sign` | Returned an HMAC-SHA256 zero-padded to 64 bytes and labelled `ssh-ed25519` | No real client could ever connect; the label was a lie about the maths |
| 3 | `sshd` `generate_dh_private` | `sha256(pid ‖ uptime_ms)` | **Session keys recoverable.** ~2^32 of search space, and both inputs are partly observable |
| 4 | `sshd` DH | Never range-checked the client's `e` | `e = 0` or `e = 1` pins the shared secret to a value the *client* chose |
| 5 | `sshd` host key file | Unparseable file → `sha256(first_line)` as the seed; missing file → a pid-derived key that changed at every restart | Every restart was a new host key, so `known_hosts` warned constantly and users learned to ignore it |
| 6 | `ssh` client | `let _ = sig_blob; // Acknowledge we received it.` — the host key signature was read and discarded, and the `known_hosts` prompt ran *before* the exchange hash existed | **The entire host key mechanism was decorative.** Any machine answering on port 22 could name itself with someone else's key and be permanently trusted under it |
| 7 | `ssh` client | The DH private exponent was `sha256` of two hard-coded 64-bit constants | Not weak — **constant**. Every connection by every copy of the binary used one exponent, so anyone with the binary could decrypt any session it made |

And two more in password handling, filed under the same head because they are
the same shape:

| # | Where | What it did | Effect |
|---|---|---|---|
| 8 | `ftpd` `validate_password` | Took the password as `_password`, never read it, and returned `lookup_user(username).is_some()` | **Remote auth bypass.** Any password logged in any account in the world-readable `/etc/passwd`, which also served as the target list |
| 9 | `sshd` `verify_password` | A fourth hasher disagreeing with the three in §329: `$5$`/`$6$` were checked as `sha256(password ‖ salt)` in hex, and anything unrecognised fell through to a **plaintext** comparison | A password set with `passwd` could never be used over ssh, and the failure looked exactly like a typo |

### The pattern, which is the actual finding

Every one of the nine was *documented*. The code said
`(signature verification skipped in simplified implementation)`, and
`In a real implementation this would use a CSPRNG`, and
`simplified: accept any non-empty password for now since our OS does not yet
have a full shadow mechanism`. Nothing was hidden. What made them dangerous is
that each one **still returned the answer a working implementation returns** —
`true`, `Ok(())`, a 64-byte blob of the right shape — so every test passed,
every connection succeeded, and nothing anywhere reported that the security
property was absent.

Two lessons worth keeping:

1. **A placeholder that returns success is not a placeholder, it is a bug that
   has been written down.** Where the real check cannot be implemented yet, the
   stub must return the *failing* answer, so the gap announces itself the first
   time anyone relies on it. `unimplemented!()` in a daemon is better than
   `true`.
2. **"We cannot verify this one" must resolve to no.** Defect 9's plaintext
   fallback and the old pubkey path's willingness to accept an algorithm it
   did not implement are the same mistake: an unknown case routed to the
   permissive branch. Both now refuse.

Also worth noting: the shadow-hash problem had already been found once,
between `passwd`, `login` and `chpasswd`, and fixed by centralising into
`posix::crypt` and then `authlib` (`design-decisions.md` §329, §341). It
recurred in `sshd` anyway, because centralising does nothing for a program
that never looks. Two of the three daemons had their own copy at the moment
the "one verifier" crate was being written.

### The fix

- **`posix::ed25519`** — a real RFC 8032 Ed25519, ~900 lines, with all four
  §7.1 test vectors and curve constants derived rather than transcribed. It
  went in `posix` rather than a new crate because it is a libc primitive and
  because the top-level `sha2/` crate is lane A's.
- **`posix::random::fill`** — the safe-Rust entry point to the existing
  ChaCha20 CSPRNG, so a Rust binary needing key material does not have to
  reach for a raw pointer, which is what pushed both daemons toward hashing
  the clock in the first place.
- **`sshd`** signs the exchange hash for real, loads and persists an OpenSSH-
  format host key (refusing an unparseable one rather than inventing a seed
  from it), verifies the RFC 4252 §7 publickey signed blob — binding session
  id, user and service — answers the query phase with `SSH_MSG_USERAUTH_PK_OK`
  instead of a failure, range-checks `e`, and takes its DH exponent from the
  CSPRNG.
- **`ssh`** verifies the host key signature *before* consulting `known_hosts`,
  refuses an algorithm it cannot check rather than accepting it, requires the
  key and signature blobs to name the same algorithm, sends a random KEXINIT
  cookie, range-checks `f`, and takes its exponent from the CSPRNG.
- **`sshd` and `ftpd`** hand passwords to `authlib::Authenticator`, one per
  daemon so the per-user failure tally outlives a connection. `sshd` passes
  the raw bytes rather than a lossy UTF-8 conversion, which was silently
  rewriting any password containing an invalid byte.

### Verification

`posix` 36 new tests (RFC 8032 vectors, a 512-bit-flip rejection sweep),
`sshd` 139, `ssh` 23 (11 new; it had 11 total before), `ftpd` 111. The
security tests are written as the attack rather than as the API: an attacker
holding the victim's public key and signing with its own is rejected; a
signature from another session, another user, another service is rejected; a
real host key replayed by a party without the private half is rejected; an
existing account with the wrong password is rejected. Clippy and rustfmt clean
on the host and for `x86_64-slateos`.

### Still open

`ftpd` sends the password in the clear — a property of FTP, not of this
implementation, and now stated in its module documentation so that the
authentication fix is not read as making the daemon safe to expose. The real
answer is `sftp` over the now-working `sshd`, which is not yet built — and is
blocked on the same missing piece as
`TD-B-SSHD-EXEC-DOES-NOT-STREAM-AND-SSH-DASH-T-IS-REFUSED`, since an sftp
subsystem is a pipe-backed session with no terminal.
