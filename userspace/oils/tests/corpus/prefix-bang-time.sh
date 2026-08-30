# `!` and `time` prefix a pipeline in any order and any number, and either may
# stand as a whole command on its own — bash's grammar gives both a
# `<prefix> list_terminator` production, where a list terminator is only `;` or
# a line end. What survives the parse is three flags, not the words: repeated
# `time` is idempotent while each `!` toggles, which is why `declare -f` prints
# them back in a fixed order rather than as they were written.
echo "=== a prefix with nothing after it is a command"
!; echo "rc=$?"
true; !; echo "rc=$?"
false; !; echo "rc=$?"
! !; echo "rc=$?"
! ! !; echo "rc=$?"
! ! ! !; echo "rc=$?"
{ !; }; echo "rc=$?"
f() { !; }; f; echo "rc=$?"
# A redirection alone is a command, so these negate a *real* null command.
! > /dev/null; echo "rc=$?"

echo "=== and it composes with the places a condition appears"
if !; then echo t; else echo f; fi
while !; do echo unreachable; break; done; echo "while=$?"
until !; do echo u; break; done

echo "=== time reports even with nothing to time"
{ time; } 2>&1 | sed 's/0m[0-9.]*s/T/'
{ time -p; } 2>&1 | sed 's/[0-9][0-9]*\.[0-9][0-9]*/T/'
# `-p` selects the POSIX format; so does `--`, which is otherwise just an end
# of `time`'s own options. Both are recognised only here and only unquoted.
{ time -p true; } 2>&1 | sed 's/[0-9][0-9]*\.[0-9][0-9]*/T/'
{ time -- true; } 2>&1 | sed 's/[0-9][0-9]*\.[0-9][0-9]*/T/'
{ time -p -- true; } 2>&1 | sed 's/[0-9][0-9]*\.[0-9][0-9]*/T/'
# Anything else after `time` is the command to run, quoted `-p` included.
{ time --p true; } 2>&1 | sed 's/0m[0-9.]*s/T/'
{ time "-p" true; } 2>&1 | sed 's/0m[0-9.]*s/T/'
x=-p; { time $x true; } 2>&1 | sed 's/0m[0-9.]*s/T/'

echo "=== the two prefixes interleave"
{ time time true; } 2>&1 | sed 's/0m[0-9.]*s/T/'
{ time ! true; } 2>&1 | sed 's/0m[0-9.]*s/T/'
# The status has to be read off the brace group itself: through a pipe it would
# be `sed`'s. `time` leaves it alone, and each `!` still toggles it.
{ time !; } 2>/dev/null; echo "rc=$?"
{ ! time true; } 2>/dev/null; echo "rc=$?"
{ time ! true; } 2>/dev/null; echo "rc=$?"
{ time ! false; } 2>/dev/null; echo "rc=$?"
{ time -p; } 2>/dev/null; echo "rc=$?"

echo "=== and only the flags are printed back"
g() { ! ! true; }; declare -f g
h() { ! time true; }; declare -f h
i() { time --; }; declare -f i
j() { time -p echo hi; }; declare -f j
k() { !; }; declare -f k

echo "=== but nothing else may follow a bare prefix"
( eval '! && echo x' ); echo "rc=$?"
( eval '! | cat' ); echo "rc=$?"
( eval '! & echo x' ); echo "rc=$?"
( eval 'time && echo x' ); echo "rc=$?"
( eval 'time | cat' ); echo "rc=$?"
( eval '( ! )' ); echo "rc=$?"
( eval 'case x in x) ! ;; esac' ); echo "rc=$?"
