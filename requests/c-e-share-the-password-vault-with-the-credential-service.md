# C → E — Share the password vault with the credential service

**From:** Lane C (`gui/credentials`). **To:** Lane E (`apps/credmanager`).
**Filed:** 2026-10-05. **Status:** OPEN.

**In short:** the operator decided (`design-decisions.md` §1417, answering
C-Q25) that a program may ask the password manager for a password -- only
with a system-issued key for it, and only after the user agrees, shown which
program is asking. Lane C builds that service (`gui/credentials`), and lane A
has now built the key (`requests/c-a-a-capability-to-ask-the-credential-service-for-a-password.md`,
reaching main with lane A's next green boot). The service needs the
passwords -- and the only vault worth reading is yours: one file,
`<config>/credmanager/vault`, sealed with Argon2id and XChaCha20-Poly1305.
`gui/credentials` today keeps a *second* vault under a home-made cipher that
`known-issues/C-THE-CREDENTIAL-STORE-ENCRYPTS-EVERY-SECRET-WITH-THE-SAME-KEYSTREAM.md`
calls the worst defect in lane C's tree. That vault is to go; the service is
to read yours.

## What is asked

The vault's format, as a library both programs build on:

- `apps/credmanager/src/vaultfile.rs` -- the header, `key_for`,
  `seal_file`, `open_file`, `parse_contents`, `contents_text` -- and the
  entry types its contents are made of, moved into a library crate, so that
  `gui/credentials` reads the file with the very code that writes it, and a
  format change is one change. Where it lives is yours to choose: a library
  in your tree (`apps/credvault`, say), which lane C depends on, is what lane
  C would suggest; a top-level crate works as well.
- One promise about writing: the password manager replaces the vault file
  whole (a new file renamed over the old), so a reader in another process
  never sees half of a save.

## What lane C does with it

The credential service opens the vault only when a program with the key
asks for a password, and only with the master password the user types into
the service's own prompt -- which also says which program is asking and for
what, with Allow once, Allow until locked and Refuse (no "always":
`design-decisions/1464-...`). It keeps the vault open for as long as the
vault's own auto-lock setting says it may sit unused, and reads a login's
`id`, `site`, `url`, `username` and `password`. It reads; it never writes.
The service's own vault, cipher and key derivation are deleted.

What the service needs of the library is small, and is already written as a
trait (`gui/credentials/src/service.rs`, `Vault`): open with a master
password (wrong password told apart from an unreadable file), lock, the
logins, and the auto-lock setting. Lane C writes the adapter; the library
need only expose what `vaultfile.rs` and the entry types already hold.

## Not asked

Any change to how the password manager itself works: it keeps opening and
saving its vault exactly as it does.

## If this is never done

The service cannot answer any program: programs cannot ask for passwords,
which is today's state and safe. It is not built on a second vault meanwhile.
