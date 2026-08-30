# `$[ … ]` — the deprecated spelling of `$(( … ))` — has an ending of its own
# when no `]` ever arrives, and it is not the one `$((` takes:
#
#     case '[':		/*]*/
#       t_index = zindex + 1;
#       temp = extract_arithmetic_subst (string, &t_index);
#       zindex = t_index;
#       if (temp == 0)
#         { temp = savestring (string);
#           if (expanded_something) *expanded_something = 0;
#           goto return0; }              /* subst.c:10650-10661 */
#
# and `return0:` is `*sindex = zindex;` (subst.c:10772). So the NULL is answered
# by keeping **the whole string being expanded** — all of it, from before the
# construct as well as after — appended to whatever the walk had accumulated,
# after which the walk ends. Hence `X$[9]Y$[ 1 + 2 Z` is `X9Y` (the closed `$[9]`
# expanded) followed by the original string entire.
#
# Reachable only under `no_longjmp_on_fatal_error` — a prompt expansion, a `PS4`,
# a `${x@P}` — since `extract_arithmetic_subst` otherwise reports and jumps
# (subst.c:1493-1506). Written into a script the same text is a parse error,
# `$[` being a row of `parse_matched_pair`'s.
#
# Which scan is reading the text also decides how far an unclosed `$[` reaches.
# `extract_dollar_brace_string` — the expansion-time `${ … }` scan — has rows for
# `` ` ``, `$(`, `<(`, `"`, `'` and a `[` subscript, and **none** for `$[`
# (subst.c:1881-1950). So under `@P` a `${x:-$[ 1 + 2 }` closes its brace at the
# `}`, and the operand `$[ 1 + 2 ` is expanded on its own — where it keeps
# *itself* whole, being the string that walk was handed. Written in a script the
# parser's `$[` row does swallow the `}`, and the quote after it.
#
# Verified against bash 5.2.37.

unset x

h='A$[ 1 + 2 B';            printf '1 [%s]\n' "${h@P}"
i='$[ 1 + 2';               printf '2 [%s]\n' "${i@P}"
j='X$[9]Y$[ 1 + 2 Z';       printf '3 [%s]\n' "${j@P}"
k='$[9]';                   printf '4 [%s]\n' "${k@P}"
l='A$[ 1 + 2 ]B';           printf '5 [%s]\n' "${l@P}"

# The failing construct is not read for anything inside it: a nested `$[ ]`, a
# substitution, a backtick and a double-quoted run all come back as written.
m='A$[ $[3] + 2 B';         printf '6 [%s]\n' "${m@P}"
n='A$[ 1 + 2 B$(echo q)C';  printf '7 [%s]\n' "${n@P}"
o='A$[ 1 + $(echo 2) B';    printf '8 [%s]\n' "${o@P}"
p='A$[ 1 + 2 `echo q` B';   printf '9 [%s]\n' "${p@P}"
q='A$[ "x }" B';            printf '10 [%s]\n' "${q@P}"

# The operand of a `${ … }`, which is a walk of its own.
a='A${x:-$[ 1 + 2 }B';      printf '11 [%s]\n' "${a@P}"
b='A${x:-$[ 1 + 2 ]}B';     printf '12 [%s]\n' "${b@P}"
c='A${x:-p$[ 1 + 2 q}B';    printf '13 [%s]\n' "${c@P}"
d='A${x:-$[ 1 + 2 }B${y}C'; printf '14 [%s]\n' "${d@P}"
e='A$[ 1 + 2 }B';           printf '15 [%s]\n' "${e@P}"
g='${x:-$[ 1 + 2 }';        printf '16 [%s]\n' "${g@P}"

# …and only when the operand is the branch taken.
x=X; printf '17 [%s]\n' "${a@P}"
unset x

# A `PS4` is the same expansion, and reports nothing either.
PS4='A$[ 1 + 2 B'
set -x
: ok
set +x
PS4='A${z:-$[ 1 + 2 }B'
set -x
: ok2
set +x
PS4='+ '

# A here-document body is *not*: nothing sets `no_longjmp_on_fatal_error` there,
# so the extraction reports and discards the body.
cat <<E
A$[ 1 + 2 B
E
echo "after $?"
