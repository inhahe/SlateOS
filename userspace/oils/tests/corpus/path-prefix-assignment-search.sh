# A `PATH=` written as a command's *assignment prefix* is not merely exported to
# the child: it is the `$PATH` the shell itself searches to find that child.
# Measured against bash 5.2.
#
#   * bash reads every variable through the temporary environment first, and the
#     command search is no exception — so `PATH=dir cmd` looks in `dir`, not in
#     the shell's own `$PATH`;
#   * a hit found that way is *not* remembered: the `hash` table is neither
#     consulted (a name already hashed elsewhere is overridden) nor added to;
#   * binding `PATH` empties the table, and a prefix binds it — so `PATH=dir cmd`
#     leaves the table empty even though the run itself hashed nothing. That
#     holds however the prefixed command turns out to be dispatched: a builtin
#     and a function flush it just as an external does;
#   * but only in *this* shell. A pipeline stage and a `&` job are forked before
#     the lookup, so their prefix's flush dies with the fork and the parent's
#     table survives intact;
#   * a real assignment does hash, and `BASH_CMDS` — the same table seen as an
#     associative array — follows the flush rather than going stale;
#   * `$_` in the child names the file the search found, spelled with `/`;
#   * `command -v`/`type` answer against the prefix too, and the prefix's `$PATH`
#     is what decides whether `command_not_found_handle` is reached at all.
#
# The scratch directories are prepended to the inherited `$PATH` rather than
# replacing it, because `sed` and `cat` below have to go on being findable —
# and the *separator* to join them with is the host's and not the shell's, so it
# is read off the `$PATH` in hand (a Windows one carries `;` and, in its drive
# letters, `:` as well, hence testing for `;` first). The directories themselves
# come from `$PWD` for the same reason: a literal path spelled for one shell
# need not split for the other.
#
# The scripts have no `#!` line on purpose — a shebangless text file is run by
# the shell itself, so no host interpreter has to exist for this to measure.
# `type -a` is deliberately absent: it applies a stricter executable test that an
# MSYS `chmod +x` on a scratch file does not satisfy (TD-OILS-MSYS-CHMOD-TYPE-A).
#
# Diagnostics name the shell — `$0`, the path it was invoked as
# (TD-OILS-DOLLAR-ZERO-ARGV0) — and the scratch directory differs per run, so
# both are folded away. The shell-name pattern must not be `[^:]*`: a Windows
# path carries a drive-letter colon of its own. Every `sed` and `cat` here is a
# pipeline stage, so the setup's `hash -r` is the last word on the table:
# nothing but `w.sh` can ever enter it.
D=$PWD
sq() { sed -e "s|$D|DIR|g" -e 's/^.*: line [0-9]*: /SH: /'; }

mkdir da db
printf 'echo DA\n' > da/w.sh
printf 'echo DB\n' > db/w.sh
printf 'echo "U=$_"\n' > db/u.sh
chmod +x da/w.sh db/w.sh db/u.sh
case $PATH in *';'*) S=';' ;; *) S=':' ;; esac
PATH="$D/da$S$PATH"

# Hash `w.sh` from the shell's own `$PATH`, so every case below starts with a
# table that a prefix could wrongly consult.
h() { hash -r; w.sh >/dev/null; }

echo "=== the prefix outranks the shell's own \$PATH, and the hash table"
h; w.sh; PATH="$D/db" w.sh; hash

echo "=== and the hit it finds is not itself remembered"
hash -r; PATH="$D/db" w.sh; hash

echo "=== a real assignment does remember"
( hash -r; PATH="$D/db"; w.sh; hash; type w.sh ) | sq

echo "=== a prefix that is not PATH leaves the table alone"
h; X=1 w.sh; hash | sq

echo "=== the flush does not care what the command turned out to be"
f() { echo in-f; }
h; PATH="$D/db" echo hi; hash
h; PATH="$D/db" f; hash
h; PATH="$D/db" :; hash

echo "=== BASH_CMDS follows the table rather than going stale"
( hash -r; PATH="$D/db"; w.sh >/dev/null; declare -p BASH_CMDS; PATH="$D/da"; declare -p BASH_CMDS ) | sq

echo "=== a forked command's prefix cannot reach this shell's table"
h; PATH="$D/db" w.sh | cat; hash | sq
h; PATH="$D/db" w.sh & wait; hash | sq

echo "=== the child is told which file was found"
PATH="$D/db" u.sh | sq

echo "=== the builtins that report a command see the prefix too"
PATH="$D/db" command -v w.sh | sq
PATH="$D/db" type w.sh | sq
PATH="$D/db" command -V w.sh | sq

echo "=== a name this \$PATH cannot find is never handed to the OS to find"
{ PATH="$D/db" sed -e '' </dev/null; echo "rc=$?"; } 2>&1 | sq
( PATH="$D/db"; sed -e '' </dev/null; echo "rc=$?" ) 2>&1 | sq

echo "=== and the prefix decides whether the handler is reached"
( command_not_found_handle() { echo "handler:$1"; }
  hash -r; PATH="$D/db" w.sh; PATH="$D/db" nope-x ) | sq
