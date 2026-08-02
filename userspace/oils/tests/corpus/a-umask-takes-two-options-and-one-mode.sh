# `umask` has exactly two option letters, `p` and `S`, and takes exactly one
# operand. The letters are scanned left to right, across words and within
# bundles, so `-pS` and `-Sp` are both "print re-inputtably, symbolically"; the
# first letter that is neither is `umask: -X: invalid option` plus the usage
# line at status 2, whatever it is bundled with. That makes every other
# `-`-word a usage error rather than a mode — `umask -w` does not set anything
# — and `--` exists to introduce a mode that begins with `-`. A word that is a
# lone `-` is not an option at all: the scan stops there and it becomes the
# mode, where it parses as a clause with no `who` and no permissions and so
# changes nothing. Only the *first* operand is the mode; the rest are ignored.
#
# Printing has two shapes. With no operand `umask` reports the current mask,
# octal or (`-S`) symbolic, prefixed by a re-inputtable `umask ` under `-p`.
# With an operand it sets, and then reports only under `-S` — the bare
# symbolic body, with no prefix even when `-p` also asked for one.
#
# A symbolic mode is a comma-separated list of clauses, and an *empty* clause
# is not skipped: it is a clause whose operator is missing. Which character the
# diagnostic names is the one standing where the operator should be, so
# `u=rwx,,g=rx` faults on the second comma while `u=rwx,` and `''` fault on the
# string terminator — which bash quotes as the NUL byte it is, so this case's
# expected output contains a literal NUL.
#
# Deliberately absent:
#
#   * any check that the mask reaches a created file. It does not, on either
#     shell as run here: this host has no POSIX modes, and osh does not consume
#     `umask_val` when opening at all — see known-issues TD-OILS15.
#   * `umask -S` with a *symbolic* operand naming who-copies (`u=g`) or the
#     `s`/`t` bits, which the mode parser does not accept.
#
# Every probe runs in a subshell so a mask change cannot reach the next one.
# Stderr is collected and replayed at the end so it can be compared in a fixed
# place; nothing here prints a pid, so it is replayed unfiltered.
exec 4>&2 2>err
umask 022

echo "=== the two options, alone and bundled"
( umask; echo "  plain    rc=$?" )
( umask -S; echo "  -S       rc=$?" )
( umask -p; echo "  -p       rc=$?" )
( umask -pS; echo "  -pS      rc=$?" )
( umask -Sp; echo "  -Sp      rc=$?" )
( umask -p -S; echo "  -p -S    rc=$?" )
( umask -S -p; echo "  -S -p    rc=$?" )

echo "=== -- ends the options"
( umask --; echo "  alone    rc=$?" )
( umask -S --; echo "  -S --    rc=$?" )
( umask -- 077; umask; echo "  mode     rc=$?" )
( umask -- -w; umask; echo "  -w       rc=$?" )
( umask -- --; echo "  --  --   rc=$?" )

echo "=== every other letter is an invalid option"
( umask -w; echo "  -w       rc=$?" )
( umask -x; echo "  -x       rc=$?" )
( umask -r; echo "  -r       rc=$?" )
( umask -rwx; echo "  -rwx     rc=$?" )
( umask -Sw; echo "  -Sw      rc=$?" )
( umask -wS; echo "  -wS      rc=$?" )
( umask -pq; echo "  -pq      rc=$?" )
( umask -; echo "  -        rc=$?" )
( umask -S -w; echo "  -S -w    rc=$?" )

echo "=== setting also prints, or does not"
( umask -S 077; umask; echo "  -S 077   rc=$?" )
( umask -p 077; umask; echo "  -p 077   rc=$?" )
( umask -pS 077; umask; echo "  -pS 077  rc=$?" )
( umask 077; umask -S; echo "  after    rc=$?" )

echo "=== operands"
( umask 022 077; umask; echo "  two      rc=$?" )
( umask u=rwx,g=,o=; umask; echo "  symbolic rc=$?" )
( umask 8; echo "  bad      rc=$?" )

echo "=== an empty clause is a clause with no operator"
p() { ( umask 022; umask "$1" >/dev/null 2>&1; r=$?; m=$(umask); echo "  [$1] rc=$r mask=$m" ); }
p ''
p ','
p ',,'
p 'u=rwx,'
p 'u=rwx,,g=rx'
p ',u=rwx'
p '-'
p '+'
p '='
p 'u'
p 'a-'
p 'u=rwx,g'

exec 2>&4 4>&-
echo "=== what went to stderr"
cat err
