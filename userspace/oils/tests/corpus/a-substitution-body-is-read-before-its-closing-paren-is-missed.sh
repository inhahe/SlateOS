# A `$( … )` body is parsed *as it is read*, so an error in it is reported and
# the missing `)` never is.
#
# bash does not scan for that `)` at all. `parse_comsub` (parse.y:4083) runs a
# whole nested `yyparse` over the rest of the input with `shell_eof_token = ')'`:
#
#     token_to_read = DOLPAREN;     /* let's trick the parser */
#     r = yyparse ();
#     …
#     if (EOF_Reached)
#       {
#         parser_state |= PST_NOERROR;
#         return (&matched_pair_error);
#       }
#
# Two consequences, and they are the whole of this case:
#
#   * an error *in* the body comes out first — `$(fi` is `near \`fi'`, not
#     `unexpected EOF while looking for matching \`)'`;
#   * the `)` message is the `EOF_Reached` path, i.e. only what is left when
#     that nested parse ran out of input rather than objecting to something.
#     `$(a |` and `$(!` are therefore still the `)`, not a blamed token.
#
# The construct that *contained* the substitution is not consulted either: a
# `${`, a `$((` or a `"` around it never gets to miss its own delimiter, because
# the nested parse dies first. The boundary is that only `$( )`, `<( )` and
# `>( )` get that nested parse — `` ` ` ``, `${ }` and `$(( ))` are read by
# `parse_matched_pair`, which just counts, so an unterminated one of those is
# reported as itself.
#
# Each probe runs under `eval` in a subshell: an error inside a substitution is
# fatal to whatever was reading it, so it would otherwise abandon the script.
# `eval`'s string is numbered from its own first line, which is what makes the
# multi-line probes' numbers small.

e() { ( eval "$1" ) 2>&1; echo "  rc=$?"; }

echo "=== the body's own error, not the paren that never came"
e 'echo $(fi'
e 'echo $(;'
e 'echo $(done'
e 'echo $( ]]'
e 'echo $(echo a; fi'
e 'x=$(fi'
e '$(fi'

echo "=== a body that merely ran out is the paren, on the line past the end"
e 'echo $(echo a'
e 'echo $('
e 'echo $(a |'
e 'echo $(!'
e 'echo $(if true'

echo "=== the body is numbered physically, from the line its ( sits on"
e 'echo $(
fi'
e 'echo one
echo $(fi'
e 'echo $(echo a
echo b
fi'

echo "=== process substitution is parsed the same way"
e 'echo <(fi'
e 'echo >(fi'
e 'echo <(echo a'

echo "=== …and the enclosing construct never gets to miss its delimiter"
e 'echo ${x:-$(fi'
e 'echo $(( $(fi'
e 'echo "$(fi'
e 'echo $(echo x; $(fi'

echo "=== but an unterminated quote *inside* the body is that quote's error"
e 'echo $(echo "x'
e "echo \$(echo 'x"
e 'echo $(echo `fi'
e 'echo "$(fi"'
e 'echo "x$(fi"y"'

echo "=== the constructs with no nested parse are unchanged"
e 'echo `fi'
e 'echo ${x'
e 'echo $((1+'
e 'echo $[1+'

echo "=== a here-document the body declared is still warned about"
e 'echo $(cat <<E'
e 'echo $(cat <<E
body
fi'

echo "=== a body that closes was already parsed where it stood"
e 'echo $(fi)'
e 'echo "$(fi)"'
e 'echo $(echo ok)'
