# An alias is expanded only where the word being read is the *command word* of
# a simple command. bash decides that from the token it read last — its
# `command_token_position` — and the answer is not simply "after a separator":
#
#   * A **reserved word** puts a command after it (`if`, `then`, `do`, `{`, `!`,
#     …), but only when it was itself read where a reserved word was acceptable.
#     `echo if c` is `echo` with two arguments, so `c` stays `c`.
#   * `time` counts, and so do the `-p`/`--` that belong to it.
#   * A `)` ends a `case` arm's pattern, so the arm's *body* begins there — but
#     `;;`, `;&` and `;;&` end an arm, and what follows one of those is the next
#     arm's **pattern**, which is never expanded.
#   * **Assignment words** precede the command word, so `x=1 c` expands `c` —
#     but only where an assignment was acceptable in the first place, so
#     `echo x=1 c` does not, and `1bad=1 c` does not either.
#   * A **leading redirection** does too, for as long as the command has been
#     nothing but redirections: `>f c` expands, and so does `2>&1 c`, `<f c`,
#     `{v}>f c` and a leading here-document — but `x=1 >f c` does not, because
#     reading the assignment word ended the run.
#
# Every check prints a marker the alias would replace, so a line reading `HIT`
# means it expanded and a line reading `c` means it did not.
#
# (`shopt -s expand_aliases` is required throughout: a non-interactive shell
# does not expand aliases at all without it.)

shopt -s expand_aliases
alias c='echo HIT'
p() { printf '%-26s' "$1"; }

echo "=== the separators"
p "start";        c
p "semicolon";    :; c
p "pipe";         : | c
p "and";          : && c
p "or";           false || c
p "subshell";     ( c )
p "background";   { : & c ; } 2>/dev/null; wait

echo "=== the reserved words"
p "then";         if :; then c; fi
p "else";         if false; then :; else c; fi
p "elif";         if false; then :; elif :; then c; fi
p "do-while";     while :; do c; break; done
p "do-for";       for i in 1; do c; done
p "until";        until c; do break; done
p "brace";        { c; }
p "bang";         ! c
p "func-body";    f() { c; }; f

echo "=== but only where a reserved word was acceptable"
p "echo if";      echo if c
p "echo then";    echo then c
p "echo do";      echo do c
p "echo while";   echo while c

echo "=== time, and time's own options"
p "time";         { time c ; } 2>/dev/null
p "time -p";      { time -p c ; } 2>/dev/null
p "time --";      { time -- c ; } 2>/dev/null
p "! time";       { ! time c ; } 2>/dev/null

echo "=== a case arm's body begins at the )"
p "first arm";    case x in x) c;; esac
p "second arm";   case y in x) :;; y) c;; esac
p "(pattern)";    case x in (x) c;; esac
p "after ;&";     case x in x) :;& *) c;; esac
p "after ;;&";    case x in x) :;;& *) c;; esac
p "in a loop";    for i in 1; do case $i in 1) c;; esac; done

echo "=== but a pattern is not a command"
alias pat='zz'
case zz in a) echo "  wrong arm";; pat) echo "  expanded";; *) echo "  literal";; esac
case zz in pat) echo "  expanded";; *) echo "  literal";; esac

echo "=== assignment words come before the command word"
p "x=1";          x=1 c
p "x=1 y=2";      x=1 y=2 c
p "x+=1";         x2+=1 c
p "x[0]=1";       x3[0]=1 c 2>/dev/null
p "echo x=1";     echo x=1 c
{ 1bad=1 c ; } 2>/dev/null; echo "  1bad=1                    rc=$?"

echo "=== a leading redirection, for as long as it is the only thing read"
{ >/dev/null c ; } 2>&1;            echo "  >f c              (silent = it ran)"
{ >/dev/null >/dev/null c ; } 2>&1; echo "  >f >f c           (silent = it ran)"
p ">f then <f";   </dev/null c
p "2>&1";         2>&1 c
p "{v}>f";        {v}>/dev/null c
p "heredoc";      <<EOD c
body
EOD
{ x=1 >/dev/null c ; } 2>&1;          echo "  x=1 >f c"
{ >/dev/null x=1 c ; } 2>&1;          echo "  >f x=1 c          (silent = it ran)"
{ >/dev/null x=1 >/dev/null c ; } 2>&1; echo "  >f x=1 >f c"

echo "=== and the trailing-blank rule still chains past all of it"
alias pre='echo PRE ' post='POST'
p "chained";      pre post
p "after x=1";    x=1 pre post
p "after >f";     2>/dev/null pre post
echo
