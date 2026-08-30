# A debugger wants the DEBUG, RETURN and ERR traps to reach into the functions
# it is stepping through, so naming `extdebug` writes `functrace` and `errtrace`
# rather than leaving them to be asked for separately. The write goes the one
# way and happens whenever extdebug is *named*, whatever it or they held before:
# `shopt -s extdebug` turns both on, `shopt -u extdebug` turns both off even
# when extdebug was never on and `set -T -E` had just turned them on by hand.
# Nothing goes back the other way — a later `set +T` turns tracing off and
# leaves extdebug exactly where it was. Both are `set -o` options, so `$-` and
# `$SHELLOPTS` follow along.
e() { sed 's/^.*: line [0-9]*: //'; }
p() { echo "--- $1"; ( eval "$1"; echo "rc=$?" ) 2>&1 | e; }
s() { set -o | sed -n 's/^\(errtrace\|functrace\)[ \t]*/\1=/p' | tr '\n' ' '; echo; }

echo "=== extdebug turns functrace and errtrace on"
p 's; shopt -s extdebug; s'
p 'shopt -s extdebug; s; shopt -u extdebug; s'

echo "=== and off again, even when it was never on"
p 'set -T -E; s; shopt -u extdebug; s'
p 'set -T -E; shopt -s extdebug; s; shopt -u extdebug; s'

echo "=== but tracing does not write back"
p 'shopt -s extdebug; set +T +E; s; shopt -p extdebug'
p 'shopt -s extdebug; set +T; shopt -p extdebug; s'

echo "=== and it shows in \$SHELLOPTS and \$-"
p 'shopt -s extdebug; case ":$SHELLOPTS:" in *:functrace:*) echo yes;; *) echo no;; esac'
p 'shopt -s extdebug; case ":$SHELLOPTS:" in *:errtrace:*) echo yes;; *) echo no;; esac'
p 'shopt -s extdebug; echo "[$-]"'

echo "=== what the traces then do"
p 'shopt -s extdebug; trap "echo D:\$BASH_COMMAND" DEBUG; f() { echo in; }; f'
p 'shopt -s extdebug; trap "echo R" RETURN; f() { echo in; }; f'
p 'shopt -s extdebug; trap "echo E" ERR; f() { false; }; f'
p 'shopt -s extdebug; shopt -u extdebug; trap "echo E" ERR; f() { false; }; f'
p 'set -E; shopt -s extdebug; shopt -u extdebug; trap "echo E" ERR; f() { false; }; f'
echo "=== done"
