# How `set -x` quotes the words it echoes. bash tries four shapes in order and
# takes the first that applies (`xtrace_print_word_list` in `print_cmd.c`), and
# because it stops at the first match the *order* is observable — a word holding
# both a space and a control character is single-quoted with the control
# character raw, never `$'…'`-escaped.
#
#   1. the empty word                       -> ''
#   2. holds a shell metacharacter          -> '…', embedded quote as '\''
#   3. holds an unprintable character       -> $'…' ANSI-C quoting
#   4. otherwise                            -> verbatim
#
# The one place an assignment differs from a command word: an empty *value*
# stays bare, so `x=` traces as `x=` and not as `x=''`.
#
# Everything here goes through `2>&1` inside a subshell so the trace lands on
# stdout in a fixed order rather than racing the real output.

# --- 4: nothing special, so no quoting at all.
( set -x; true a=b c/d 1+1 -x --long ) 2>&1

# --- 2: the metacharacter set. Each of these words holds exactly one.
( set -x; true 'a b' 'a	b' 'a|b' 'a&b' 'a;b' 'a(b' 'a)b' 'a<b' 'a>b' ) 2>&1
( set -x; true 'a!b' 'a{b' 'a}b' 'a*b' 'a[b' 'a]b' 'a?b' 'a^b' 'a$b' 'a`b' ) 2>&1
( set -x; true "a'b" 'a"b' 'a\b' ) 2>&1

# --- 2, position-sensitive: `#` is a metacharacter only as the first character
# of a word (it starts a comment only there), and `~` only at the start or right
# after a `=` or a `:` (the places tilde expansion looks at).
( set -x; true '#ab' 'a#b' 'ab#' 'a=#b' ) 2>&1
( set -x; true '~ab' 'a~b' 'ab~' 'a~b~c' 'a=~b' 'a:~b' 'a=~b~c' ) 2>&1

# --- 1: the empty word, alone and repeated.
( set -x; true '' ) 2>&1
( set -x; true '' '' x '' ) 2>&1

# --- 1 vs. the assignment exception.
( set -x; x= ) 2>&1
( set -x; x=; y=; true "$x$y" ) 2>&1
( set -x; x= true ) 2>&1
( set -x; x='a b' ) 2>&1
( set -x; x=1; x+=' 2' ) 2>&1

# --- 3: ANSI-C quoting, including bash's `\E` for escape (not `\033`) and
# three-digit octal for everything else unprintable.
( set -x; true $'a\001b' ) 2>&1
( set -x; true $'a\002\003b' ) 2>&1
( set -x; true $'a\033b' ) 2>&1
( set -x; true $'a\177b' ) 2>&1
( set -x; true $'\a\b\f\r\v' ) 2>&1
( set -x; true $'a\010b' ) 2>&1
( set -x; true $'\001' ) 2>&1

# --- 3 vs. 2: the branch order. Both words below hold a control character, but
# they also hold IFS whitespace, so branch 2 wins and the control byte is
# emitted raw between single quotes.
( set -x; true $'a\001 b' ) 2>&1
( set -x; true $'a\t\001b' ) 2>&1
( set -x; true $'a\n\001b' ) 2>&1

# --- an assignment value takes the same three branches.
( set -x; x=$'a\033b' ) 2>&1
( set -x; x=$'a\001 b' ) 2>&1
( set -x; x='#ab' ) 2>&1
( set -x; x='a#b' ) 2>&1

# --- expansions are quoted by their *result*, not their source text.
( set -x; v='a b'; true "$v" ) 2>&1
( set -x; v='a b'; true $v ) 2>&1
( set -x; v=; true "$v" ) 2>&1
( set -x; set -- 'p q' r; true "$@" ) 2>&1
( set -x; set -- 'p q' r; true "$*" ) 2>&1

# --- PS4 is itself expanded, and the trace prefix does not change the quoting.
( PS4='T '; set -x; true 'a b' ) 2>&1
( PS4=''; set -x; true 'a b' ) 2>&1
