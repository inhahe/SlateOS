## B-SUDO-AUTHORISED-WHAT-ITS-RULES-REFUSED -- argument restrictions ignored, negation inverted, `..` through a pattern, sudoedit asked about running files, `-s` authorised the first word (lane B, 2026-10-01)

**Status:** FIXED 2026-10-01

**In short:** `sudo` decides who may run what as whom, from `/etc/sudoers`.
Ours granted things its rules refused, five ways: a rule naming a program
*with* arguments allowed it with any arguments; `!` (refuse this) granted
everything else and did not refuse the thing it named; a pattern like
`/usr/bin/*` let `/usr/bin/../../tmp/evil` through; permission to *run* a file
was taken as permission to *edit* it with `sudoedit`; and `sudo -s CMD` checked
only the first word, then handed the whole line to a shell. Found reading the
crate for the `visudo`/`sudoreplay` split. None of it was live: the kernel has
no set-user-ID path and identity changes need `SET_CREDENTIALS`, so `sudo`
cannot yet lift a caller anywhere -- but it is the code that will decide once
something can, and the staging request puts it on the image.

| What | Rule | Granted | Upstream |
|---|---|---|---|
| arguments ignored | `alice ALL = /usr/bin/systemctl restart nginx` | `sudo systemctl stop sshd` | `command_args_match`: `""` = none, `^…$` = ERE, else `fnmatch` |
| negation inverted | `alice ALL = !/usr/bin/passwd` | every command but passwd | a negated match DENIES |
| negation unreached | `alice ALL = ALL, !/usr/bin/passwd` | passwd (the `!` "did not match", so `ALL` decided) | the last matching spec decides, either way |
| prefix pattern | `alice ALL = /usr/bin/*` | `sudo /usr/bin/../../tmp/evil` | `glob` results + canonical dir + inode; `*` never crosses `/` |
| sudoedit asked "may run" | `alice ALL = /etc/motd` | `sudoedit /etc/motd` | pseudo-command `sudoedit`, files as its arguments (`FNM_PATHNAME`) |
| `-s` first word | `alice ALL = /usr/bin/id` | `sudo -s id '&& reboot'` ran `sh -c "id && reboot"` | the SHELL is authorised; the words reach it backslash-escaped |

**The fix**, against sudo 1.9.15p5's `match.c`, `match_command.c` and
`parse_args.c`: a command spec now answers ALLOW / DENY / UNSPEC
(`cmnd_matches`), aliases recurse with a depth guard, and the first spec that
names the request decides; arguments are compared as upstream compares them
(the matcher is the shared `fnmatch` crate, out of coreutils for this, and
`ere`); the caller's program is resolved on the secure path, made canonical,
and matched -- a pattern with `FNM_PATHNAME`, a `dir/` spec as "directly in
it", any other by canonical path (by name only where a side does not exist) --
and the canonical program is what is executed, with the caller's name as
`argv[0]` (`-sh` for `-i`, as upstream marks a login shell); `-s`/`-i` run
`SHELL -c ESCAPED` and authorise exactly that; `! /cmd` with a space parses as
one negated command. 17 new tests, each failing before the fix.

**Still not upstream's, and why.** This is a reimplementation, not a port;
sudo 1.9.15p5 is ~100,000 lines and a faithful port is the real end state.
Divergences kept, deliberately: an unqualified command in a rule resolves on the
secure path (upstream refuses the rule), which the 2026-09-12 entry above made
safe; no `sha256:` digests, `fdexec` or `CWD`/`CHROOT`; `\,` escapes inside a
rule's arguments are not unescaped (the list is split on every comma); `-s`
uses the target's shell where upstream uses the caller's `$SHELL`. sudoedit's
*execution* and `visudo`'s file handling had their own defects, fixed in the
next entries.

**Where:** `userspace/sudo/src/main.rs` (`check_authorization`, `cmnd_matches`,
`args_match`, `command_matches`, `command_path_matches`, `invocation`,
`shell_escaped_command`); `userspace/fnmatch`.
