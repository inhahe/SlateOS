# The operand of `${x#…}`, `${x/…/…}` and `${x^…}` is a *pattern*, and bash
# does not hand it the quoting the word around it had. `getpattern` swaps the
# bit out:
#
#     l = *value ? expand_string_for_pat (value,
#           (quoted & (Q_HERE_DOCUMENT|Q_DOUBLE_QUOTES)) ? Q_PATQUOTE : quoted,
#           …)                                        /* subst.c:5754-5756 */
#
# and the replacement side clears it outright:
#
#     rep = expand_string_for_patsub (rep,
#             quoted & ~(Q_DOUBLE_QUOTES|Q_HERE_DOCUMENT));  /* subst.c:9183 */
#
# `Q_PATQUOTE` is 0x008, a bit of its own, and the whole of bash tests it in
# three places (subst.c:3020, 10358, 10365) — all about `$*`/`$@`. Everywhere
# else that asks `quoted & (Q_HERE_DOCUMENT|Q_DOUBLE_QUOTES)` reads *false*
# inside a pattern, which is what this file is about. Two of those tests are
# visible with nothing but a `${x:-…}` written in the pattern:
#
#     if ((quoted & (Q_HERE_DOCUMENT|Q_DOUBLE_QUOTES)) && *value)
#       { sindex = 0; temp = string_extract_double_quoted (value, &sindex,
#                                                          SX_STRIPDQ); }
#                                                   /* subst.c:7724-7729 */
#     if (… || (quoted & (Q_DOUBLE_QUOTES|Q_HERE_DOCUMENT)))
#       { … goto add_character; }                    /* subst.c:11190 */
#
# So `"${w#${z:-~}}"` expands the tilde where the very same operand written as
# `"${z:-~}"` does not, and `"${w#${z:-a"\x"}}"` keeps the backslash the same
# operand loses on its own. The enclosing `" … "` is still there — it is only
# that the pattern is no longer expanded *as* a double-quoted string.
#
# A `"` written *inside* the pattern puts the bit back, because that is an
# ordinary quoted run and gets `Q_DOUBLE_QUOTES` like any other.
#
# Verified against bash 5.2.37.

HOME=/hh
unset z
f=('')
b() { printf '  %-14s[%s]\n' "$1" "$2"; }
d() { printf '  %-14s' "$1"; printf '%s' "$2" | od -An -tx1 | tr -d '\n'; printf ' len=%s\n' "${#2}"; }

echo "=== 1. the tilde a pattern's operand still expands"
# `w` is `$HOME`, so a pattern that expanded the tilde strips the lot.
w=/hh
b 'rem #'    "${w#${z:-~}}"
b 'rem ##'   "${w##${z:-~}}"
b 'rem %'    "${w%${z:-~}}"
b 'rem %%'   "${w%%${z:-~}}"
b 'sub pat'  "${w/${z:-~}/Z}"
b 'sub #pat' "${w/#${z:-~}/Z}"
b 'sub all'  "${w//${z:-~}/Z}"
# The same operand written on its own is the plain quoted case, and there the
# third test in `case '~'` does refuse.
b 'plain'    "${z:-~}"
# A bulk operator over an array asks the same question once.
n=(/hh /hh)
b 'arr rem'  "${n[@]#${z:-~}}"

echo "=== 2. and the backslash it still keeps"
# `SX_STRIPDQ` would drop the `\` before the `x` — see
# a-used-brace-operands-embedded-quotes-lose-their-backslashes.sh. In a pattern
# the scan never runs, so the operand keeps it and matches a literal backslash.
v='a\xb'
b 'rem bs'   "${v#${z:-a"\x"}}"
b 'sub bs'   "${v/${z:-a"\x"}/Z}"
b 'plain bs' "${z:-a"\x"}"
# The proof the other way round: against `axb` the *stripped* pattern would
# have matched and this one does not.
v2='axb'
b 'rem bs2'  "${v2#${z:-a"\x"}}"

echo "=== 3. a quote written in the pattern puts the bit back"
b 'rem inq'  "${w#"${z:-~}"}"
b 'bs inq'   "${v#"${z:-a"\x"}"}"
# A single-quoted pattern is not expanded at all.
b 'rem sq'   "${w#'${z:-~}'}"

echo "=== 4. the replacement clears the bit outright"
b 'rep tilde' "${w/hh/${z:-~}}"
b 'rep bs'    "${v/b/${z:-a"\x"}}"
b 'arr rep'   "${n[@]/hh/${z:-~}}"

echo "=== 5. what Q_PATQUOTE does keep of the quoting"
# The three places that do name it are the `$*`/`$@` ones, so a quoted `[@]` in
# the pattern is still a word list and the operand is still not split into
# fields — see a-list-in-an-operand-makes-it-a-word-list.sh for what that does.
# Against `  X` the list pattern is ` X`, which is not a prefix, and the
# istring pattern `  X` would have been.
w2='  X'
d 'strip'    "${w2#${z:-"${f[@]}"  X}}"
d 'strip-q'  "${w2#"${z:-"${f[@]}"  X}"}"
set -- a b c
b 'star'     "${w/hh/$*}"
IFS=:
b 'star ifs' "${w/hh/$*}"
IFS=' '

echo done
