> aa
> ab
> bb

# `extglob` is not a matching option — it is a *lexing* one. It decides whether
# `?(`, `*(`, `+(`, `@(` and `!(` open an extended-pattern group at all, and
# that decision is taken while the text is being read. It is off by default in a
# non-interactive shell, so in a plain script `(` after any of those characters
# is still a metacharacter.
echo "=== off by default, so the paren is a metacharacter"
shopt extglob
( eval 'echo @(a)' ); echo "rc=$?"
( eval 'echo ?(a)' ); echo "rc=$?"
( eval 'x=@(a)' ); echo "rc=$?"
# A `case` pattern gets no exemption either.
( eval 'case aa in @(aa|x)) echo c;; esac' ); echo "rc=$?"

echo "=== which is why a leading ! is a negated subshell, not a pattern"
!(false); echo "rc=$?"
if !(false); then echo t; fi
!(true); echo "rc=$?"

echo "=== the one exception is the pattern operand of a [[ ]] match"
# bash lexes an extended pattern there whatever the option, so these work in a
# default shell — while the same text in any other [[ ]] position does not.
[[ aa == @(aa|x) ]] && echo m1
[[ aa == !(bb) ]] && echo m2
[[ aa == ?(a)a ]] && echo m3
[[ aab == +(a)b ]] && echo m4
# (stderr dropped: bash's conditional-expression diagnostics name the partial
# word `@(a' where osh names the `(' — see TD-OILS-COND-TOKEN-SPELLING.)
( eval '[[ @(a) == b ]]' ) 2>/dev/null; echo "rc=$?"
( eval '[[ -n @(a) ]]' ) 2>/dev/null; echo "rc=$?"

echo "=== and turning it on only affects text read afterwards"
# bash reads, parses and runs one unit at a time, so the shopt cannot change the
# line it is written on: this is still a syntax error.
( eval 'shopt -s extglob; echo @(aa)' ); echo "rc=$?"
# On the next line it is in force.
shopt -s extglob
shopt extglob
echo @(aa|x)
echo !(bb)
case aa in @(aa|x)) echo c;; esac
x=@(a); echo "$x"
# Nested readings inherit it: a command substitution's body is lexed the same.
echo $(echo @(aa|x))
# And `!(` is a pattern now, not a subshell.
echo !(aa|ab)

echo "=== and turning it back off restores the metacharacter"
shopt -u extglob
( eval 'echo @(aa)' ); echo "rc=$?"
!(false); echo "rc=$?"
