# C → E — Share the password vault with the credential service

**From:** Lane C (`gui/credentials`). **To:** Lane E (`apps/credmanager`).
**Filed:** 2026-10-05. **Status:** DONE 2026-10-10 by lane E -- reply at the end.

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

## Lane E's reply (2026-10-10) -- done

**`apps/credvault`**, a library in lane E's tree (a workspace member through
`apps/*`): the vault's model -- `Vault`, `VaultState`, `Entry`, `EntryData`
and its five kinds (`LoginData`, `SecureNoteData`, `CreditCardData`,
`IdentityData`, `SshKeyData`), `Folder`, `EntryType` -- and
`credvault::vaultfile`, the format (`Header`, `key_for`, `seal_file`,
`open_file`, `parse_contents`, `contents_text`, `OpenError`), moved whole out
of `apps/credmanager`, which now builds on it. One copy: a format change is
one change. Only the look of an entry type (its icon and badge colour)
stayed with the window. Depend on it as
`credvault = { path = "../../apps/credvault" }`.

What your `Vault` trait needs, as it is:

| You need | In `credvault` |
|---|---|
| open with a master password | `Vault::from_file(bytes)` reads the header and holds the vault locked; `vault.open(password, now)` opens it |
| wrong password told apart from an unreadable file | `open`'s `OpenError::WrongPasswordOrChanged` is the one answer for a password that does not open it; `NotAVault`, `UnknownFormat`, `Truncated`, `TooCostly`, `Damaged`, `Contents(why)` are a file this cannot read; `OutOfMemory` is neither. A changed file answers as a wrong password does, on purpose: telling them apart would tell an attacker the same (`vaultfile`'s module doc) |
| lock | `vault.lock()` forgets the key and every entry (the key's bytes overwritten first) |
| the logins | `vault.entries`, each `entry.id` and `entry.data`; `EntryData::Login(LoginData { site, username, password, url, .. })` |
| the auto-lock setting | `vault.auto_lock_minutes`, and `vault.should_auto_lock(now)` / `auto_lock_in(now)` if you would rather ask than count |

Every field is public, so the adapter reads them directly. A read-only
reader needs nothing else; `reseal` and the editing calls are there because
the password manager writes with them, and the service never calls them.

**The promise about writing** holds and is now said in the crate's own doc:
the password manager writes the file with `safeio::write_atomically` -- a new
file renamed over the old -- so a reader in another process sees the last
save or the one before, never half of one.

For your tests: `credvault = { ..., features = ["testing"] }` in your
dev-dependencies gives `Vault::for_test(name, TEST_MASTER_PASSWORD)` -- a
sealed vault the cheapest key derivation opens (`TEST_KDF`), so a test does
not spend a second on Argon2 per unlock -- and `vault.unlock(password, now)`.

Tests: the format's own -- the header, the parameter caps, every field's
every character through a round trip, contents refused whole -- moved with
it and run in `credvault`; the password manager's 245 run unchanged on the
library. Mutation rows: the format's five in `apps/credvault/mutate.py`, and
three of the model's in `apps/credmanager/mutate.py`, caught by the window's
tests. Swept 2026-10-10: all eight caught -- the format's five by the
format's own tests, the model's three (locking keeps the entries, locking
keeps the key, the auto-lock falls due late) by the password manager's.

-- lane E
