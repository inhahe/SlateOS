# An expansion error ends the word, and the shell does not look at the rest of
# it. That shows up first as a count — `"$a$b"` under `set -u` names only `a`,
# and `"${a?m1}${b?m2}"` only `m1` — but it is not merely the second message
# being swallowed: the parts after the failure are never expanded, so a command
# substitution among them does not run and its output never appears. Every way
# an expansion can fail behaves alike here, whether the shell is going to exit
# over it (`set -u`, a bad transform) or merely drop the command (`${x!}`,
# `${1=d}`, an indirection through an invalid name). What resets it is the next
# command: the shell reports afresh for each one.
e() { sed 's/^.*: line [0-9]*: //'; }
p() { echo "--- $1"; ( set -u; eval "$1"; echo "rc=$?" ) 2>&1 | e; }

echo "=== one word names one unset variable, not both"
p 'echo "[$a$b]"'
p 'echo "[$a][$b]"'
p 'echo "[$a]" "[$b]"'
p 'echo "[$a$b$c]"'
p 'echo "[${a?m1}${b?m2}]"'
p 'echo "[$a${b?m2}]"'
p 'echo "[${a?m1}$b]"'
p 'echo "[${a:?}${b:?}]"'
p 'echo "[${#a}${#b}]"'
p 'x=(1); echo "[${x[5]}${x[6]}]"'
p 'x=(1); echo "[$a${x[5]}]"'
p 'v="$a$b"; echo done'
p 'a=1; echo "[$a$b]"'

echo "=== and the kinds of failure mix, the first one still winning"
p 'echo "[${a}${!b}]"'
p 'echo "[$a${x!}]"'
p 'echo "[${x!}$a]"'

echo "=== the rest of the word is not expanded, so its side effects do not happen"
p 'echo "[$a$(echo hi >&2; echo H)]"'
p 'echo "[$(echo hi >&2; echo H)$a]"'
p 'echo "[${a?m}$(echo hi >&2)]"'
p 'echo "[$a]" "[$(echo hi >&2)]"'
p 'f() { echo fn >&2; }; echo "[$a$(f)]"'
p 'echo "[${x!}$(echo hi >&2)]"'
p 'ptr=; echo "[${!ptr}$(echo hi >&2)]"'
p 'set -- ; echo "[${1=d}$(echo hi >&2)]"'
p 'a=1; echo "[$a$(echo hi >&2; echo H)]"'
p 'set -- p; echo "[${1=d}$(echo hi >&2)]"'

echo "=== a nested expansion is reached the same way, so it stops too"
p 'x=(1); echo "[$a${x[$(echo sub >&2; echo 0)]}]"'
p 'echo "[$a${b:-$(echo dflt >&2)}]"'
p 'echo "[$a${b/$(echo pat >&2)/y}]"'
p 'echo "[$a$((n=n+1))]"; echo "n=${n-unset}"'
p 'echo "[${a-$(echo dflt >&2; echo D)}$b]"'

echo "=== every context that expands a word answers alike"
p 'for i in "$a$(echo hi >&2)"; do echo "[$i]"; done'
p 'case "$a$(echo hi >&2)" in *) echo m;; esac'
p 'v="$a$(echo hi >&2)"; echo done'

echo "=== but the next command reports afresh"
p 'echo "[$a]"; echo "[$b]"'
p 'echo "$a" | cat; echo "$b" | cat; echo end'
p 'echo "$a" > /dev/null; echo "$b"'
p 'echo "[$a]"; echo "[$(echo hi >&2)]"'
p 'set +u; echo "[${x!}]"; echo "[$(echo after)]"'
p 'set +u; ptr=; echo "[${!ptr}]"; echo "[$(echo after)]"'
p 'set +u; f() { echo "[${x!}]"; echo "[$(echo inner)]"; }; f; echo "[$(echo outer)]"'
echo "=== done"
