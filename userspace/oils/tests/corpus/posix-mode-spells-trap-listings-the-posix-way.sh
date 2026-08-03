# Posix mode changes `trap`'s listing in two ways that are *not* gated alike.
#
# Signal names lose their `SIG` prefix in any listing, since POSIX spells a
# signal without it. But a signal with *no* trap being shown as `- NAME` rather
# than omitted happens only under an explicit `-p`; a bare `trap` still shows
# just what is set. And the reset form goes away entirely: POSIX gives `trap` an
# action or nothing, so `trap EXIT` is a usage error there rather than a way to
# take the EXIT trap away.
#
# (The no-operand `trap -p` listing — every signal the shell knows — is left to
# the lib test: the signal *set* is the host's, so it cannot read the same on
# every machine.)

echo "=== the SIG prefix goes, in a bare listing and under -p alike"
trap 'echo U' USR1
trap 'echo E' EXIT
echo "--- outside:"; trap; trap -p USR1
set -o posix
echo "--- inside:"; trap; trap -p USR1
set +o posix
echo "--- and back:"; trap -p USR1

echo "=== the pseudo-signals never had a prefix to drop"
trap 'echo D' DEBUG
trap 'echo R' ERR
trap 'echo T' RETURN
set -o posix
trap -p EXIT DEBUG ERR RETURN
set +o posix
trap -p EXIT DEBUG ERR RETURN
trap - DEBUG ERR RETURN

echo "=== an untrapped signal is listed as reset — but only under -p"
set -o posix
echo "--- trap -p QUIT:"; trap -p QUIT
echo "--- trap -p USR1 QUIT INT:"; trap -p USR1 QUIT INT
echo "--- bare trap (only what is set):"; trap
set +o posix
echo "--- outside, trap -p QUIT prints nothing:"; trap -p QUIT; echo "  rc=$?"

echo "=== an ignored trap is still the empty action, not a reset"
trap '' INT
set -o posix
trap -p INT QUIT
set +o posix
trap - INT

echo "=== a bad name is still an error, and still interleaves"
set -o posix
trap -p USR1 BOGUS QUIT; echo "  rc=$?"
set +o posix

echo "=== the reset form is a usage error in posix mode"
( set -o posix; trap EXIT ); echo "  trap EXIT -> rc=$?"
( set -o posix; trap USR1 ); echo "  trap USR1 -> rc=$?"
( trap 'echo E' EXIT; set -o posix; trap EXIT; echo "  reached" )
echo "--- while the two-operand forms still work"
( set -o posix; trap 'echo E2' EXIT; trap -p EXIT )
( set -o posix; trap - USR1; trap -p USR1 )
trap - USR1 EXIT
