## TD-B-SSHD-ACCEPTS-ONLY-ONE-AUTHORIZEDKEYSFILE-PATH-WHERE-OPENSSH-ACCEPTS-A-LIST (lane B, 2026-09-05) -- FIXED 2026-09-16

`AuthorizedKeysFile` now takes a list. `authorized_keys_paths` splits the
setting on whitespace and resolves each pattern independently;
`pubkey_auth_for_account` reads every named file and a key in any of them
authorises, which is what OpenSSH does. 235 -> 240 tests, clippy clean.

**Two decisions inside it that are not obvious.**

*The discard-on-error rule moved from the setting to each file.* An unreadable
file authorising nothing -- indistinguishable from an empty one, so an
unauthenticated peer cannot probe which accounts exist -- was right and stays.
Applied to the whole SETTING it would have made a missing first file hide a
present second one, which is precisely the `authorized_keys` +
`authorized_keys2` case this entry is about.

*The contents are joined with a newline, not concatenated.* A file whose last
line lacks a trailing newline would otherwise be spliced onto the first line of
the next, destroying one key from each and producing a single corrupt entry out
of two valid ones -- a failure nobody would trace back to a missing byte.

**Fail-closed on an empty setting:** zero patterns resolve to zero paths, so
nothing is authorised. Resolving an empty setting to the home directory would
have the daemon read a directory as a key list on every login.

**Probed by restoring the defect** -- `vec![authorized_keys_path(setting, user)]`,
the whole setting as one filename. Four of the five new tests fail; the fifth,
`a_single_pattern_is_unchanged_by_list_support`, correctly still passes, because
it pins the behaviour that was always right. A control that failed under the
sabotage would have meant it was testing the bug rather than the invariant.

**Not implemented, and named so nobody assumes it:** `AuthorizedKeysFile none`,
which OpenSSH accepts to disable publickey auth entirely. Today `none` resolves
as a relative filename (`~/none`), which almost certainly does not exist, so the
effect is the same by accident. Worth doing properly if anyone relies on it.

The description below is kept in the tense it was written in.

**The original entry, 2026-09-05, follows.**

**In short:** `sshd_config` has a setting, `AuthorizedKeysFile`, naming the file
that lists which keys may log into an account. Real OpenSSH lets an
administrator write *several* files there, separated by spaces, and tries each
in turn. We read the whole setting as one file name. An administrator who
carries over a config using the list form gets a daemon looking for a single
file whose name contains spaces, which will not exist — so publickey
authentication silently stops working for every account, and the clients are
told only that their keys were not accepted.

**Where it lives:** `authorized_keys_path` in `userspace/sshd/src/lib.rs`
resolves exactly one pattern, and `pubkey_auth_for_account` reads exactly one
file. The config parser (`"authorizedkeysfile"` in `SshdConfig::parse`) stores
the rest of the line verbatim, spaces included.

**Reproduce:** put `AuthorizedKeysFile .ssh/authorized_keys .ssh/authorized_keys2`
in `sshd_config` and try to log in with a key listed in the first file. It is
refused. The daemon looked for a file literally named
`.ssh/authorized_keys .ssh/authorized_keys2`.

**Why the list form is not obscure:** it is how `.ssh/authorized_keys2` — the
second file every OpenSSH has read since the SSH-1/SSH-2 split — is still
configured, and it is how an administrator adds a system-wide file
(`/etc/ssh/authorized_keys/%u`) *alongside* the user's own rather than instead
of it. That combination is the standard recipe for "the admin can grant access
and the user can too", so the configs most likely to use it are the ones written
by someone being careful.

**Severity:** a config-compatibility gap rather than a security hole. The
failure is closed — an unresolvable path authorises nothing — so it refuses
logins that should work and never admits one that should not. Nothing in this
tree writes the list form today, so it bites only an operator bringing a config
from a real OpenSSH.

**The proper fix:** split the setting on whitespace at *parse* time, keeping
`SshdConfig::authorized_keys_file` a `Vec<String>`, and have
`pubkey_auth_for_account` resolve and read each in order, concatenating what it
finds. Resolution stays per-path so `%h`/`%u`/`%U` expand in each. A file that
is absent must remain not-an-error, exactly as the single-file case already
treats it — with a list, most accounts will have only the first.

The one thing to get right is that a *missing* file and an *unreadable* one stay
indistinguishable to the peer, as they are now: reporting which is which across
several paths would let an unauthenticated client map the filesystem. And the
concatenation must not stop at the first file that exists — OpenSSH reads all of
them, and stopping early would silently ignore keys an administrator had granted.

Note that quoting is *not* part of this: OpenSSH splits on whitespace with no
quoting mechanism, so a path containing a space cannot be configured there
either. Matching that exactly is better than inventing quoting we would then
have to keep agreeing with.
