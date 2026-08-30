# A `case` pattern's `)` has no opening mate, so a scan that finds the end of a
# `$( … )` by counting parentheses reaches zero in the middle of the `case` and
# ends the substitution there. `x=$(case $y in a) … esac)` is one of the
# commonest things anyone writes inside a substitution, so the extent scan has to
# know enough grammar to tell a pattern's `)` from a group's.
#
# "Enough grammar" means knowing where the reserved words `case`, `in` and `esac`
# are reserved — which is command position, and only there: `printf %s case in f`
# is three arguments and its `)` still closes the substitution. Quoting matters
# too: `"esac"` is a plain word. Each case below pins one of those.

echo "=== the shape that motivates all this ==="
x=$(case b in a) echo A;; b) echo B;; esac)
echo "x=[$x]"

echo "=== a pattern's optional open paren has no mate of its own ==="
x=$(case b in (a) echo A;; (b) echo B;; esac)
echo "x=[$x]"

echo "=== alternatives, and a catch-all ==="
x=$(case b in a|b) echo AB;; esac)
echo "x=[$x]"
x=$(case zz in a) echo A;; *) echo star;; esac)
echo "x=[$x]"

echo "=== the clause body ends at ;; / ;& / ;;& — or at esac ==="
# A bare `;` does not end a body, so `esac` after one is still the reserved word.
x=$(case b in b) echo B; esac)
echo "semi x=[$x]"
x=$(case b in b) echo B ;; esac)
echo "spaced x=[$x]"
x=$(case b in b) echo B;& c) echo C;; esac)
echo "fallthrough x=[$x]"
x=$(case b in b) echo B;;& b*) echo B2;; esac)
echo "retest x=[$x]"

echo "=== an empty case ends at once ==="
x=$(case b in esac)
echo "x=[$x]"

echo "=== nesting: two cases can share a depth ==="
x=$(case b in b) case c in c) echo deep;; esac;; esac)
echo "x=[$x]"
x=$(echo "$(case b in b) echo deep;; esac)")
echo "nested-subst x=[$x]"

echo "=== a group inside a body is still a group ==="
x=$(case b in b) (echo sub);; esac)
echo "subshell x=[$x]"
x=$(case b in b) echo "$(echo inner)";; esac)
echo "cmdsub x=[$x]"
x=$(case $(echo b) in $(echo b)) echo P;; esac)
echo "in-pattern x=[$x]"

echo "=== the parens that are not the grammar's ==="
# A quoted one is text; an extglob one is part of the pattern and closes itself.
x=$(case ")" in ")") echo paren;; esac)
echo "quoted x=[$x]"
shopt -s extglob
x=$(case b in @(a|b)) echo X;; esac)
echo "extglob x=[$x]"
shopt -u extglob

echo "=== command position is what makes a word reserved ==="
x=$(printf '%s-' case in file)
echo "argument x=[$x]"
x=$(echo a case b in c)
echo "later-argument x=[$x]"
x=$(echo $((1 + 1)) case)
echo "after-arith x=[$x]"
x=$(case b in b) echo "esac";; esac)
echo "quoted-esac x=[$x]"

echo "=== and every place a command position arises ==="
x=$(true | case b in b) echo piped;; esac)
echo "pipeline x=[$x]"
x=$(for i in 1 2; do case $i in 1) echo one;; 2) echo two;; esac; done)
echo "for-do x=[$x]"
x=$(f() { case b in b) echo fn;; esac; }; f)
echo "function x=[$x]"
x=$(echo `case b in b) echo bt;; esac`)
echo "backtick x=[$x]"

echo "=== the layout can be spread out ==="
x=$(case b in
b)
  echo B
  ;;
esac
)
echo "multiline x=[$x]"
x=$(case b
in
b) echo B;;
esac)
echo "in-next-line x=[$x]"
x=$(case b in # a comment
b) echo B;; esac)
echo "comment x=[$x]"

echo "=== a here-document in a clause body ==="
x=$(case b in b) cat <<EOF
hd
EOF
;; esac)
echo "heredoc x=[$x]"

echo "=== process substitution reads its body the same way ==="
cat <(case b in b) echo B;; esac)
echo "procsub rc=$?"

echo "=== a case the close paren interrupts ==="
# The `)` is where bash's parser wanted `;;` or `esac`, so it names that token —
# and the failure belongs to the substitution, which exits 1, not to the input,
# which would exit 2. Needs an input of its own because it never ends.
( eval 'x=$(case b in b) echo B); echo "x=[$x]"' ) 2>&1
echo "open rc=$?"
# `esac` is reserved wherever a pattern could start, so `case esac in` is a
# syntax error rather than a match against the word `esac`.
( eval 'x=$(case esac in esac) echo E;; esac); echo "x=[$x]"' ) 2>&1
echo "esac-subject rc=$?"

echo done
