## [B] FIXED — `sudo` never checked the password, and admitted everyone when the user database was absent (2026-08-17, `65dca4eba`)

**In short:** `sudo` is the program that decides who is allowed to become the
machine's administrator. It asked for a password, and then threw it away
without looking at it. Anyone with an account could type anything and get root.
If the file listing the machine's accounts did not exist yet, it gave root to
*any* name typed at the prompt, account or not. Found while migrating the
readers of `/etc/users.yaml` onto the shared `userdb` crate; fixed in the same
commit that found it.

### What the code did

`userspace/sudo/src/main.rs`, `fn authenticate(username, _password)` — note the
underscore, which is Rust for "this argument is deliberately unused":

1. If `/etc/users.yaml` does not exist, `return Ok(())`. The comment read
   "If no user database, allow (development/single-user mode)". This is
   unconditional root for anyone on a machine that has not yet created an
   account.
2. Otherwise, return `Ok(())` if the file's *text* contains the substring
   `name: <username>`. Never a hash comparison; the password was not read.
3. The substring is a substring of `username: "alice"`, which is why it
   appeared to work at all. It is equally a substring of a `display_name`, a
   `home_dir`, or a comment — so a string that was never an account could
   authenticate as one.

### Why 191 tests missed it

The decision was welded to a hard-coded filesystem path. No test could
construct a database to decide against, so no test decided anything. The fix
splits the decision into `authenticate_against(db, username, password)`, which
is pure, and seven tests now cover it — including "an empty database admits
nobody" and "a username is not matched by substring".

**This is the shape to look for elsewhere:** a security decision whose inputs
can only come from a path only root can write is a decision that will never be
tested, and the untestedness is not an accident of effort — it is a
consequence of the signature.

### Two more dead comparisons in the same crate, also fixed

`get_user_groups` and `get_user_info` compared each line to `name: <user>` with
`==` rather than a substring test, so unlike `authenticate` they matched
nothing at all. Consequences: every user was reported as belonging only to
their own group, so **every sudoers rule written against a group silently
denied**; and `get_user_info` special-cased root's home and shell ahead of the
database, so an administrator who set root's shell had it ignored.
