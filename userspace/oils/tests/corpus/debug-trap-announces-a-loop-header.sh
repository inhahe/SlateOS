# `$BASH_COMMAND` is not only a simple command's argv. Before running a `for`,
# a `select` or a `case`, the shell rebuilds a one-line source form of the
# header — `for i in a b`, `case x in ` — records it there and fires the DEBUG
# trap on it, so a debugger stepping through a script stops on the loop itself
# and not only on the commands inside it.
#
# The two loops differ in *when*: a `for` header is announced again before every
# iteration, while a `select` header is announced once, ahead of the menu. A
# `for` over an empty list therefore announces nothing at all. The text is the
# unexpanded source, so `case $(echo z) in ` names the substitution rather than
# what it produced, and a `for name; do` with no list reads as `for name in
# "$@"` — the list the parser wrote in for it.
#
# Under `shopt -s extdebug` a refusal means different things to the two: a `for`
# loses only that iteration (and does not even bind the loop variable), leaving
# the handler's status behind as the loop's own, while a `case` loses the whole
# statement and leaves 0.
e() { sed 's/^.*: line [0-9]*: //'; }
p() { echo "--- $1"; ( eval "$1"; echo "rc=$?" ) 2>&1 | e; }
b() { echo "B:[$BASH_COMMAND]"; }
c() { n=$((n+1)); echo "F$n:[$BASH_COMMAND]" >&2; [ "$n" != "$K" ]; }
r() { if [ "$BASH_COMMAND" = "$T" ]; then return 2; fi; return 0; }

echo "=== a for header is announced once per iteration"
p 'trap b DEBUG; for i in a b; do :; done'
p 'trap b DEBUG; for i in; do :; done'
p 'trap b DEBUG; for i in $(echo); do :; done'
p 'trap b DEBUG; for i in ""; do :; done'
p 'trap b DEBUG; for i in "a b" c; do :; done'
p 'trap b DEBUG; for i in $(echo a); do :; done'
p 'trap b DEBUG; set -- a b; for i; do :; done'
p 'trap b DEBUG; set --; for i; do :; done'
p 'trap b DEBUG; for i in a b; do for j in c; do :; done; done'

echo "=== a case header once, with the trailing space its printer leaves"
p 'trap b DEBUG; case x in x) : ;; esac'
p 'trap b DEBUG; case y in x) : ;; esac'
p 'trap b DEBUG; case $(echo z) in z) : ;; esac'
p 'trap b DEBUG; case x in
x) : ;;
esac'

echo "=== a select header once, ahead of the menu"
printf '1\n2\n' > in.txt
p 'trap b DEBUG; select i in a b; do echo "got $i"; done < in.txt'
# ... and *before* the list is expanded, so an empty one still announces itself
# — where a `for` over an empty list, announcing per iteration, says nothing.
p 'trap b DEBUG; select i in; do :; done < in.txt'
p 'trap b DEBUG; select i in $(echo); do :; done < in.txt'
p 'trap b DEBUG; set --; select i; do :; done < in.txt'
p 'set -x; select i in $(echo); do :; done < in.txt'
p 'set -x; for i in $(echo); do :; done'

echo "=== these are the only compounds that announce themselves"
p 'trap b DEBUG; while false; do :; done'
p 'trap b DEBUG; until true; do :; done'
p 'trap b DEBUG; if true; then :; fi'
p 'trap b DEBUG; { :; }'
p 'trap b DEBUG; ( : )'
p 'trap b DEBUG; f() { :; }'

echo "=== refusing a for header costs one iteration"
p 'shopt -s extdebug; n=0; K=99; trap c DEBUG; for i in a b c; do echo "body $i"; done; echo "r=$?"'
p 'shopt -s extdebug; n=0; K=3; trap c DEBUG; i=Z; for i in a b c; do echo "body $i"; done; echo "r=$? i=$i"'
p 'shopt -s extdebug; n=0; K=5; trap c DEBUG; for i in a b c; do echo "body $i"; done; echo "r=$?"'

echo "=== refusing a case or select header costs the whole statement"
T='case x in '
p 'shopt -s extdebug; n=0; K=1; trap c DEBUG; case x in x) echo arm ;; esac; echo "r=$?"'
p 'shopt -s extdebug; n=0; K=1; trap c DEBUG; select i in a b; do echo "got $i"; done < in.txt; echo "r=$?"'

echo "=== and a 2 leaves the function the loop was written in"
T='for i in a b'
f() { for i in a b; do echo "body $i"; done; echo tail; }
shopt -s extdebug
trap r DEBUG
f
echo "fr=$?"
trap - DEBUG
shopt -u extdebug
echo "=== done"
