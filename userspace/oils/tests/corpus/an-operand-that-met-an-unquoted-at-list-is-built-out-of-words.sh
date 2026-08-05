# bash expands a word along one of two paths. Ordinarily the answer is a string
# and the word around it splits that string on `$IFS`. But an **unquoted** `[@]`
# anywhere in a `${x:-…}` operand moves the whole operand onto the other path,
# where the answer is a list of *words* — and a list of words is glued back
# together with single spaces, not with `$IFS`, and the glue is final.
#
# Two separate things follow from that, and `$IFS` decides which of them apply:
#
#   * a **plain** `[@]` — the array named and nothing more (`$@`, `${a[@]}`, or
#     `${a[@]:-w}` on the branch where it answers with the array) — contributes
#     its elements as words. Their own characters are no longer the operand's to
#     split, and the single space put *between* them is the only separator the
#     list itself adds. A **derived** `[@]` (a slice, the keys, a bulk operator)
#     hands over text bash has already made one string of, so its characters
#     split like any other;
#   * then, if `$IFS` names no space at all, the operand is word-split and the
#     words are glued back with single spaces into **one finished field**, which
#     neither splits again nor globs.
#
# So with `a=(p:q r:s)` there are three regimes, and the ordinary `$IFS` is the
# one that hides all of this:
#
#   IFS=':'   ${x:-${a[@]}}    <p:q r:s>      protected, and joined
#   IFS=': '  ${x:-${a[@]}}    <p:q><r:s>     protected, not joined
#   IFS=' :'  ${x:-${a[@]}}    <p><q><r><s>   neither — `$IFS` leads with a space
#
# The question `$IFS` is asked is not quite the same one twice: joining asks
# whether a space is a separator *at all*, and protection asks whether it is the
# *first* separator named.

show() { printf '<%s>' "$@"; echo; }
a=(p:q r:s); e=(); u=; w=; b=(m:n o:p)

echo '=== the three regimes'
( IFS=:;   show ${u:-${a[@]}} ) 2>&1
( IFS=': '; show ${u:-${a[@]}} ) 2>&1
( IFS=' :'; show ${u:-${a[@]}} ) 2>&1
( IFS=$'\t:'; show ${u:-${a[@]}} ) 2>&1
( show ${u:-${a[@]}} ) 2>&1
( IFS=; show ${u:-${a[@]}} ) 2>&1

echo '=== the join is what makes a separator in the operand come back as a space'
( IFS=:; show ${u:-${a[@]}:Z} ) 2>&1
( IFS=:; show ${u:-Z:${a[@]}} ) 2>&1
( IFS=:; show ${u:-${a[@]}Z} ) 2>&1
( IFS=' :'; show ${u:-${a[@]}:Z} ) 2>&1

echo '=== a derived [@] is text, so its own characters split too'
( IFS=:; show ${u:-${a[@]:0}} ) 2>&1
( IFS=:; show ${u:-${a[@]:0:2}} ) 2>&1
( IFS=:; show ${u:-${a[@]^^}} ) 2>&1
( IFS=:; show ${u:-${a[@]#x}} ) 2>&1
( IFS=:; show ${u:-${!a[@]}} ) 2>&1
( IFS=': '; show ${u:-${a[@]:0}} ) 2>&1

echo '=== a [*] is never this path'
( IFS=:; show ${u:-${a[*]}} ) 2>&1
( IFS=:; show ${u:-${!a[*]}} ) 2>&1
( IFS=:; set -- p:q r:s; show ${u:-$*} ) 2>&1
( IFS=:; set -- p:q r:s; show ${u:-$@} ) 2>&1

echo '=== the elements are words, so their own separators do not split'
( IFS=:; c=(':' 'q'); show ${u:-${c[@]}} ) 2>&1
( IFS=:; c=('' 'q'); show ${u:-${c[@]}} ) 2>&1
( IFS=:; c=('q' ''); show ${u:-${c[@]}} ) 2>&1
( IFS=:; s=('x y' 'z w'); show ${u:-${s[@]}} ) 2>&1
( IFS=:; s=('x y' 'z w'); show ${u:-${s[@]}:Q} ) 2>&1
( IFS=': '; c=(':' 'q'); show ${u:-${c[@]}} ) 2>&1

echo '=== the finished field is a string: it neither splits again nor globs'
mkdir -p g && : > g/aa && : > g/ab
( cd g || exit; IFS=:; g=('a*'); show ${u:-${g[@]}} ) 2>&1
( cd g || exit; IFS=:; g=('a*' 'z'); show ${u:-${g[@]}} ) 2>&1
( cd g || exit; IFS=:; g=('a*'); show ${u:-${g[@]}:z} ) 2>&1
( cd g || exit; IFS=' :'; g=('a*'); show ${u:-${g[@]}} ) 2>&1
( IFS=:; show A${u:-${a[@]}}B ) 2>&1
( IFS=:; show ${u:-A${a[@]}B} ) 2>&1
( IFS=:; show A${u:-${a[@]}}:B ) 2>&1

echo '=== an empty list still leaves the operand its one field'
( IFS=:; show ${u:-${e[@]}} ) 2>&1
( IFS=:; show ${u:-${e[@]}Z} ) 2>&1
( IFS=:; set --; show ${u:-$@} ) 2>&1
( IFS=:; show ${u:-} ) 2>&1

echo '=== a ${a[@]:-w} is plain only on the branch that answers with the array'
( IFS=:; show ${u:-${a[@]:-Z}} ) 2>&1
( IFS=:; show ${u:-${e[@]:-p:q}} ) 2>&1
( IFS=:; show ${u:-${e[@]:-p:q}Z} ) 2>&1
( IFS=:; show ${u:-${e[@]:-${b[@]}}} ) 2>&1
( IFS=:; show ${u:-${e[@]:-${b[@]}}Z} ) 2>&1

echo '=== a nested operand finishes its own word list first'
( IFS=:; show ${u:-${w:-${a[@]}}} ) 2>&1
( IFS=:; show ${u:-${w:-${a[@]}:Y}} ) 2>&1
( IFS=:; show ${u:-${w:-${a[@]}}Z} ) 2>&1

echo '=== command substitution is text, so it is not this path'
( IFS=:; show ${u:-$(echo "${a[@]}")} ) 2>&1
( IFS=:; v=p:q; show ${u:-$v} ) 2>&1
( IFS=:; show ${u:-p:q} ) 2>&1

echo '=== the :+ and := operands ask the same question'
( IFS=:; s=1; show ${s:+${a[@]}} ) 2>&1
( IFS=:; s=1; show ${s:+${a[@]}:Z} ) 2>&1
( IFS=:; q=; show ${q:=${a[@]}}; show "$q" ) 2>&1
( IFS=:; show ${e[@]:-${a[@]}} ) 2>&1

echo done
