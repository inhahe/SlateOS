# A used `${x-w}` / `${x=w}` / `${x+w}` operand is read once more before it is
# expanded — see a-used-brace-operand-is-scanned-once-more-before-it-expands.sh
# for the reports that read makes. This file pins what it *builds*, which read 3
# then expands instead of the original:
#
#     if ((quoted & (Q_HERE_DOCUMENT|Q_DOUBLE_QUOTES)) && *value)
#       { sindex = 0;
#         temp = string_extract_double_quoted (value, &sindex, SX_STRIPDQ); }
#     else
#       temp = value;                              /* subst.c:7724-7732 */
#
# `SX_STRIPDQ` takes the `"`s out, and with them it rewrites the backslashes
# between them:
#
#     if ((stripdq == 0 && c != '"') ||
#         (stripdq && ((dquote && (sh_syntaxtab[c] & CBSDQUOTE)) || dquote == 0)))
#       temp[j++] = '\\';                          /* subst.c:906-911 */
#
# — with `stripdq` set the backslash survives only inside an embedded quote
# before a `CBSDQUOTE` character (`$`, backquote, `"`, `\`, newline), or outside
# one, where `dquote == 0`. So a backslash before an ordinary character is
# dropped, but only inside the operand's own `" … "`.
#
# `dquote` starts at 0 and only a `"` *in the operand* raises it, so the
# operand's own top level counts as outside however the enclosing word was
# quoted. And the whole thing is gated on the expansion being quoted, which is
# the one place bash's own `temp = value` shows through.
#
# None of this needs a prompt expansion or a failed read: it is what an ordinary
# `echo "${z:-…}"` does.
#
# Verified against bash 5.2.37.

f() { printf '%s [%s]\n' "$1" "$2"; }
unset z w

echo "=== 1. inside the operand's own quotes the backslash goes"
f 'one'        "${z:-p"\x"r}"
f 'two'        "${z:-p"\x\y"r}"
# `dquote` toggles, so each run is judged on its own.
f 'runs'       "${z:-p"a"b"\x"c}"
# …and stays before the five characters it keeps its meaning for.
f 'cbs $'      "${z:-p"\$"r}"
f 'cbs bs'     "${z:-p"\\"r}"
f 'cbs dq'     "${z:-p"\""r}"
f 'cbs nl'     "${z:-p"\
"r}"
# The escape is taken once: the `\` that `\\` swallowed is not read again as the
# start of another one, so the `x` keeps nothing in front of it but that `\`.
f 'bs then bs' "${z:-p"\\x"r}"

echo "=== 2. outside them it stays, however the enclosing word is quoted"
f 'outside'    "${z:-p\xr}"
f 'mixed'      "${z:-p\x"\y"r}"
f 'empty q'    "${z:-p""\xr}"

echo "=== 3. and only when the expansion is quoted"
# Unquoted there is no scan at all — `temp = value` — so the operand goes on as
# it stands and the `"` does its ordinary work at expansion time.
f 'unquoted'   ${z:-p"\x"r}
f 'unq bs'     ${z:-p\xr}

echo "=== 4. the same six operators as the read, and no others"
unset z; f ':-' "${z:-p"\x"r}"
unset z; f '- ' "${z-p"\x"r}"
unset z; f ':=' "${z:=p"\x"r}"
unset z; f '= ' "${z=p"\x"r}"
w=WW;    f ':+' "${w:+p"\x"r}"
w=WW;    f '+ ' "${w+p"\x"r}"
# A pattern operator reads its operand through a reader of its own, which does
# no such rewrite — the `\x` is still a `\x` when the match is tried.
y=YY
f '# '   "${y#p"\x"r}"
f '% '   "${y%p"\x"r}"
f '/ '   "${y/p"\x"r/Q}"
f '/rp'  "${y/Y/p"\x"r}"
f ':of'  "${y:p"\x"r}"

echo "=== 5. every quoted context, not just a prompt"
unset z
v="A${z:-p"\x"r}B"; f 'assign' "$v"
cat <<EOF
heredoc [${z:-p"\x"r}]
EOF
u='A${z:-p"\x"r}B'; f 'at-P' "${u@P}"
printf 'printf [%s]\n' "${z:-p"\x"r}"

echo "=== 6. what the scan copies through by extent it does not touch"
# A nested `${ … }` is copied whole, and gets the rule from its own
# `parameter_brace_expand_rhs` rather than from this one.
f 'nested brace' "${z:-${w2:-p"\x"r}}"
# A `$( … )` is copied by its extent, so what is inside is the command's to
# read and no backslash in it is rewritten here.
f 'in comsub'    "${z:-p$(printf '%s' "a\xb")r}"
# `string_extract_double_quoted` has no single-quote row at all: a `'` is just a
# byte it copies, and the `\x` beside it is at `dquote == 0`.
f 'single q'     "${z:-p'\x'r}"
f 'sq then dq'   "${z:-p'\x'"\y"r}"

echo done
