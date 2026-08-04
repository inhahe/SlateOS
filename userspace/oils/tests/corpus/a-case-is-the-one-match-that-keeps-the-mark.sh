# A `case` does no quote removal — on either side. Every other construct that
# takes a word takes it through quote removal, which drops the mark a quoted
# empty in an operand left (bash's `CTLNUL`, see
# `a-quoted-empty-in-an-operand-still-makes-a-field.sh`). A `case` expands both
# its subject and its arm patterns with `expand_word_leave_quoted` and matches
# the mark bytes as they stand, so there a mark is a character like any other:
# the byte `\177`. `case abc in ${x:-a''bc})` does not match, and a real
# `$'a\177b'` matches the pattern `${x:-a''b}` because the mark *is* that byte.
#
# The one exception, applied by each side to itself once, is bash's
# `QUOTED_NULL` macro: a word that is *nothing but* a single mark is the empty
# string instead. `case ${x:-''} in ${y:-''})` matches — both sides are `""` —
# while `${y:-''''}` is two marks, so two characters, and matches neither.
#
# Only an *operand* leaves a mark. A quoted empty written in the `case` word
# itself is plain quoting and expands to nothing at all, so `case abc in
# ''a''b''c'')` matches, and so does `case a''bc in abc)`.

p() { printf '  %-22s' "$1"; }
u=; unset n
e=(); f=('')

echo "### an operand's mark is a character in the pattern"
p pat-mid;      case abc in ${nope:-a''bc}) echo match;; *) echo no;; esac
p pat-lead;     case abc in ${nope:-''a*}) echo match;; *) echo no;; esac
p pat-trail;    case abc in ${nope:-a*''}) echo match;; *) echo no;; esac
p pat-both;     case abc in ${nope:-''a*''}) echo match;; *) echo no;; esac
p pat-star-mid; case abc in ${nope:-*''*}) echo match;; *) echo no;; esac
p pat-dq;       case abc in ${nope:-""a*}) echo match;; *) echo no;; esac
p pat-dq-unset; case abc in ${nope:-"$n"a*}) echo match;; *) echo no;; esac
p pat-dq-null;  case abc in ${nope:-"$u"a*}) echo match;; *) echo no;; esac
p pat-tail-lit; case abc in ${nope:-abc''}) echo match;; *) echo no;; esac
p pat-bare;     case abc in ${nope:-a*}) echo match;; *) echo no;; esac
p pat-alt;      s=v; case abc in ${s:+a''bc}) echo match;; *) echo no;; esac

echo "### and in the subject"
p subj-mid;     case ${nope:-a''bc} in abc) echo match;; *) echo no;; esac
p subj-star;    case ${nope:-a''bc} in a*) echo match;; *) echo no;; esac
p subj-q;       case ${nope:-a''} in a?) echo match;; *) echo no;; esac
p subj-q2;      case ${nope:-a''b} in a?b) echo match;; *) echo no;; esac
p subj-star2;   case ${nope:-a''b} in a*b) echo match;; *) echo no;; esac
p subj-ab;      case ${nope:-a''b} in ab) echo match;; *) echo no;; esac
p subj-class;   case ${nope:-a''b} in a[!x]b) echo match;; *) echo no;; esac

echo "### the mark is the byte \\177, and matches one"
p real-byte;    case $'a\177b' in ${nope:-a''b}) echo match;; *) echo no;; esac
p real-pat;     case ${nope:-a''} in a$'\177') echo match;; *) echo no;; esac
p real-subj;    case $'\177' in ${nope:-''}) echo match;; *) echo no;; esac
p real-two;     case $'\177\177' in ${nope:-''''}) echo match;; *) echo no;; esac

echo "### QUOTED_NULL: a word that is one mark and nothing else is empty"
p lone-q;       case ${nope:-''} in ?) echo match;; *) echo no;; esac
p lone-star;    case ${nope:-''} in *) echo match;; *) echo no;; esac
p lone-dq;      case ${nope:-''} in "") echo match;; *) echo no;; esac
p lone-lit;     case '' in ${nope:-''}) echo match;; *) echo no;; esac
p lone-both;    case ${nope:-''} in ${nope:-''}) echo match;; *) echo no;; esac
p two-qq;       case ${nope:-''''} in ??) echo match;; *) echo no;; esac
p two-q;        case ${nope:-''''} in ?) echo match;; *) echo no;; esac
p two-dq;       case ${nope:-''''} in "") echo match;; *) echo no;; esac
p two-pat;      case '' in ${nope:-''''}) echo match;; *) echo no;; esac
p lone-vs-two;  case ${nope:-''} in ${nope:-''''}) echo match;; *) echo no;; esac

echo "### only an operand leaves one — plain quoting in the word does not"
p lit-sq-lead;   case abc in ''abc) echo match;; *) echo no;; esac
p lit-sq-mid;    case abc in a''bc) echo match;; *) echo no;; esac
p lit-dq-lead;   case abc in ""abc) echo match;; *) echo no;; esac
p lit-dq-unset;  case abc in "$n"abc) echo match;; *) echo no;; esac
p lit-dq-cmdsub; case abc in "$(true)"abc) echo match;; *) echo no;; esac
p lit-dollar-sq; case abc in $''abc) echo match;; *) echo no;; esac
p lit-one-empty; case x in "${f[@]}"x) echo match;; *) echo no;; esac
p subj-lit-mid;  case a''bc in abc) echo match;; *) echo no;; esac
p subj-lit-q;    case a''bc in a?bc) echo match;; *) echo no;; esac
p subj-lit-dq;   case a""bc in abc) echo match;; *) echo no;; esac
p subj-via-var;  v=a''bc; case $v in abc) echo match;; *) echo no;; esac

echo "### every other construct still removes it"
[[ abc == ${nope:-''a*''} ]] && echo "  cond      match" || echo "  cond      no"
[[ abc =~ ${nope:-''a''}bc ]] && echo "  regex     match" || echo "  regex     no"
w=abc
echo "  strip     [${w#${nope:-''a''}}]"
echo "  repl      [${w/${nope:-''b''}/X}]"
v=${nope:-''x''}
echo "  assign    [$v] len=${#v}"
printf '  hered     '; cat <<< ${nope:-a''b}
echo done
