## CLOSED 2026-09-15: 89 crates compared, 89 clear

Every collision above is fixed. The tools that changed, and what each one had
been doing:

| tool | the letter | here | upstream |
|---|---|---|---|
| `blkid` | `-n` | `--no-encoding` | `--match-types` |
| `hardlink` | `-x`/`-X` | swapped with each other | `--exclude` / `--respect-xattrs` |
| `hardlink` | `-p`/`-o`/`-t` | `--respect-*`, and the DEFAULTS inverted | `--ignore-*` |
| `dmesg` | `-c -T -f -s -n` | five different meanings | `--read-clear --ctime --facility --buffer-size --console-level` |
| `getty` | `-h` | `--help` | `--flow-control` |
| `getty` | `-o` | `--long-hostname` | `--login-options <opts>` |
| `chpasswd` | `-s` | `--sha256`, a flag | `--sha-rounds <n>` |
| `pstree` | `-g -h -t -N` | four different meanings | `--show-pgids --highlight-all --thread-names --ns-sort` |
| `eject` | `-n` | `--no-unmount` | `--noop` |
| `eject` | `-f` | `--force` | `--floppy` (force is `-F`) |
| `xdg` | `-n` | `--no-open` | `--no-ask` |
| `findmnt` | `-t` | `--type` | `--types` |
| `flock`, `locale`, `logrotate`, `sysctl` | one each | long name only | -- |

**The rule that decided every case.** A short LETTER may not be redefined,
because being wrong about one is SILENT: the request is understood, acted on,
and answered wrongly. A long NAME may be added freely, because being wrong
about one is loud -- an unknown long option fails with a message. So every
extension in the table above kept its long name and gave the letter back.

**The checker was wrong eight times, in eight different ways**, and every one
was caught by verifying a finding against the real tool before acting on it.
Recorded because the pattern is the useful part, not the individual bugs: a
crate that is several programs; a comment quoting a binding; a reference that
binds one letter twice; a personality named only in a `match` arm; a binding
judged against a sibling personality's table; a capture running past its own
match into a subcommand dispatch; an enumeration of ignored options read as a
definition; and an enum variant name mistaken for a program name.

It was also wrong in the OTHER direction once, which cost more: psmisc writes
`-N TYPE, --ns-sort=TYPE`, and the reference parser wanted the comma beside
the letter, so every option in that style went uncompared. `pstree -N` had
been sitting behind that.

**Two classes this cannot find, and neither is hypothetical.** `eject -n`
was `--noop` by NAME and `--no-unmount` in BEHAVIOUR -- the checker compares
spellings, not meanings, and that one was found by reading. And `dmesg -n`
was a bare `"-n" =>` arm with no long partner at all, so there was nothing to
compare; it was found by reading the help text. A clean run from
`scripts/compare-short-options.py` means the SPELLINGS agree, and no more
than that.

**Two near-misses worth keeping.** `unshare` has no `-c` for
`--map-current-user` where util-linux does, and our `blkid -c` has no
`--cache-file` long form. Neither is a collision -- a missing spelling fails
visibly with "unknown option" -- so they are a different and much milder
class than the above.
