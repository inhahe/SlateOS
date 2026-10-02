## [B] Two different `sudo` binaries are built from this workspace (2026-08-17)
**Status:** FIXED 2026-08-21 (`f5d95fa2b`) — `su`'s built-in `sudo` personality was deleted, so `userspace/sudo` is the only `sudo`; `check-bin-collisions.py` (pre-push gate 51) now refuses a second one.

**In short:** The build produces two separate programs both called `sudo`,
from two crates that do not know about each other, implementing different
policies. Whichever the installer copies last is the one the machine gets, and
nothing in the tree says which that should be.

| | `userspace/sudo` | `userspace/su` |
|---|---|---|
| Size | ~4400 lines | ~1300 lines |
| Personalities | `sudo`, `sudoedit`, `visudo`, `sudoreplay` | `su`, `sudo` (by `argv[0]`) |
| Policy source | `/etc/sudoers` — full parser, aliases, `NOPASSWD`, host and runas matching | hard-coded: root, or membership of `wheel`/`admin` |
| Timestamps | yes (`-v`, `-k`, `-K`, configurable timeout) | none |
| Session logging | yes (`sudoreplay`) | a line appended to `/var/log/auth.log` on denial |

Both are workspace members (`members = [… "userspace/*" …]`), so both are
built. Both were separately migrated onto `userdb` in this batch, which is
precisely the duplicated-effort tax that having two of them imposes.

### Proper fix

`userspace/sudo` is the real one: it implements the sudoers file, which is the
interface administrators expect and the one `design.txt` implies. `su`'s sudo
personality should be deleted, leaving `su` as `su` alone — its `argv[0]`
dispatch, `SudoOptions`, `parse_sudo_args`, `sudo_authorised`,
`sudo_list_permissions`, `run_sudo` and `log_sudo_failure` all go.

Not done in this batch because deleting a program is a user-visible change on
a different footing from fixing one, and because the two policies genuinely
differ: `su`'s sudo authorises on `wheel`/`admin` membership *without* a
sudoers file, so a machine with no `/etc/sudoers` can currently still
administer itself. Removing it means the installer must ship a default
`/etc/sudoers`, and that is a change to the installed system, not to a crate.
Check what `pkg`/the installer writes before deleting.

**Severity:** high while it lasts — two programs answering the same question
differently is how a machine ends up believing an account is an administrator
in one context and not another, which is the same class of defect as the
`is_admin`/`admin` field split that §330 fixed one level down.
