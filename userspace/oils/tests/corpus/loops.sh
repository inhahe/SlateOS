# Loop constructs and the control-flow statements that escape them. The corpus
# already exercises the *expansions* a `for` list is built from; this case is
# about the loops themselves — the C-style form, `break`/`continue` with a
# level, redirection applied to a loop body, and the exit status a loop reports.

# `for name in words` with no list at all is not an error — the body simply
# never runs — and `for name` with no `in` iterates the positional parameters.
for x in; do echo "never $x"; done
set -- p q r
for x; do printf '%s ' "$x"; done; echo
set --

# The C-style form evaluates three arithmetic expressions; any of them may be
# empty, and an empty condition is true (so the loop needs an inner break).
for (( i = 0; i < 4; i++ )); do printf '%d ' "$i"; done; echo
for (( i = 0, j = 9; i < j; i += 2, j -= 3 )); do printf '%d:%d ' "$i" "$j"; done; echo
for (( i = 0;; i++ )); do (( i > 2 )) && break; printf 'inf%d ' "$i"; done; echo

# `while` and `until` are exact complements: each runs its body while the
# condition's *status* is zero / nonzero respectively.
n=0
while (( n < 3 )); do printf 'w%d ' "$n"; n=$(( n + 1 ))
done; echo
n=0
until (( n >= 3 )); do printf 'u%d ' "$n"; n=$(( n + 1 ))
done; echo

# A loop's exit status is that of the last command its body ran — not the
# condition that ended it. An empty loop body run zero times reports 0.
while false; do :; done; echo "never-ran=$?"
for x in 1 2 3; do false; done; echo "last-body=$?"
for x in 1 2 3; do true; done; echo "last-body-true=$?"

# `break N` and `continue N` act on the Nth enclosing loop, counting outward
# from 1. A level larger than the nesting depth breaks the outermost loop
# rather than erroring.
for a in 1 2 3; do
    for b in x y z; do
        [ "$b" = y ] && continue 2
        printf '%s%s ' "$a" "$b"
    done
    echo "unreached-$a"
done; echo
for a in 1 2 3; do
    for b in x y z; do
        [ "$a$b" = 2y ] && break 2
        printf '%s%s ' "$a" "$b"
    done
done; echo
for a in 1 2; do
    for b in x y; do
        break 9
    done
done; echo "break-9-status=$?"

# `break`/`continue` with no enclosing loop is a no-op that still reports 0.
break; echo "stray-break=$?"
continue; echo "stray-continue=$?"

# A redirection on the loop keyword applies to the whole body, so a `read` in
# the condition consumes from it across iterations.
printf 'alpha\nbeta\ngamma\n' > lines.txt
while read -r line; do printf '[%s]' "$line"; done < lines.txt; echo
# The same loop redirected on output: every body write goes to the file, and
# the file is opened once (truncated once), not per iteration.
for x in 1 2 3; do echo "line$x"; done > out.txt
wc -l < out.txt

# `read` in a *pipeline* runs in a subshell, so its assignments are lost after
# the loop — the classic gotcha. With a redirect they survive.
count=0
printf 'a\nb\n' | while read -r _; do count=$(( count + 1 )); done
echo "piped-count=$count"
count=0
while read -r _; do count=$(( count + 1 )); done < lines.txt
echo "redir-count=$count"

# A `while read` stops at a final line with no trailing newline unless the
# leftover in $line is handled after the loop.
printf 'one\ntwo\nthree' > partial.txt
while read -r line; do printf '<%s>' "$line"; done < partial.txt
echo "|leftover=$line"

# `case` is not a loop but shares the "status of the matched body" rule, and an
# unmatched `case` reports 0.
case zzz in a) false;; esac; echo "case-nomatch=$?"
case a in a) false;; esac; echo "case-match=$?"

# `select` reads from stdin and writes its menu to stderr. With stdin at EOF it
# exits immediately; REPLY and the loop variable are left unset.
select opt in one two; do echo "picked $opt"; break; done < /dev/null 2>/dev/null
echo "select-status=$? opt=${opt-unset} reply=${REPLY-unset}"

# Loops are commands, so they compose: pipe a loop's output, capture it, and
# run one in the background of a `&&` chain.
for x in c a b; do echo "$x"; done | sort | tr '\n' ' '; echo
captured=$(for x in 1 2; do echo "v$x"; done)
echo "captured=${captured//$'\n'/,}"
true && for x in 1; do echo "chained"; done

# `continue` in the *last* iteration still runs the loop's normal exit path.
for x in 1 2 3; do
    [ "$x" = 3 ] && continue
    printf 'k%s ' "$x"
done
echo "after=$?"

# A loop inside a function returning from the middle: `return` unwinds the
# loops as well as the function.
f() {
    for a in 1 2 3; do
        for b in 1 2 3; do
            [ "$a$b" = 22 ] && return 7
        done
    done
    return 0
}
f; echo "func-return=$?"
