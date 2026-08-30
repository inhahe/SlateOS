# `set -H` / `set -o histexpand` is the switch for `!`-style history expansion.
# This case covers the *switch* only — that it has real, observable state. The
# expansion it enables has its own case, histexpand.sh.
#
# The switch is worth pinning separately because it shows up in four places that
# are easy to get inconsistent: the `set -o` listing, `$-`, `SHELLOPTS`, and
# what a subshell inherits. bash keeps all four in agreement, and `histexpand`
# is independent of `history` — either can be on without the other.

echo "=== both default off in a non-interactive shell"
set -o | grep -E '^(history|histexpand)'
echo "  dash=[$-]"

echo "=== set -H turns it on, and shows up as H in \$- and in SHELLOPTS"
set -H
set -o | grep '^histexpand'
echo "  dash=[$-]"
case ":$SHELLOPTS:" in *:histexpand:*) echo "  in SHELLOPTS";; *) echo "  not in SHELLOPTS";; esac

echo "=== it is independent of history, which is still off"
set -o | grep '^history'
case ":$SHELLOPTS:" in *:history:*) echo "  history in SHELLOPTS";; *) echo "  history not in SHELLOPTS";; esac

echo "=== set +H turns it back off"
set +H
set -o | grep '^histexpand'
echo "  dash=[$-]"
case ":$SHELLOPTS:" in *:histexpand:*) echo "  in SHELLOPTS";; *) echo "  not in SHELLOPTS";; esac

echo "=== the long name is the same switch"
set -o histexpand
set -o | grep '^histexpand'
echo "  dash=[$-]"

echo "=== H sits between E and T in the flag string, as in bash's flag table"
set -E -T
echo "  dash=[$-]"
set +E +T

echo "=== set +o replays it re-inputtably"
set +o | grep '^set .o histexpand'

echo "=== a subshell inherits the setting"
( echo "  sub dash=[$-]"; set -o | grep '^histexpand' )

echo "=== turning it on does not create the HIST* sizing variables"
echo "  names=[${!HIST*}]"

echo "=== and it survives alongside history being turned on later"
set -o history
set -o | grep -E '^(history|histexpand)'
echo "  dash=[$-]"
