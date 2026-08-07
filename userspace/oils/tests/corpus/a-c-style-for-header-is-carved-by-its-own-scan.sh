# The `init; cond; update` of a `for (( … ))` is not split on `;`. bash's
# `make_arith_for_command` (make_cmd.c:278) walks the header with
# `skip_to_delim (start, 0, ";", SD_NOJMP|SD_NOPROCSUB)`, so a `;` inside a
# construct that scan steps over belongs to that construct and separates
# nothing — while a `;` inside one it does *not* step over ends a section
# however arithmetic the text around it looks.
#
# Which constructs those are is settled by the flag set and nothing else:
#
#   * stepped over — `'…'`, `"…"`, `` `…` ``, `$( … )`, `${ … }`, `$'…'`,
#     `$"…"`, and `\;`;
#   * not stepped over — `[ … ]` (that wants `SD_GLOB`), `<( … )` and `>( … )`
#     (suppressed by `SD_NOPROCSUB`), `$[ … ]` (which `skip_to_delim` does not
#     know at all) and a bare `( … )` (that wants `SD_ARITHEXP`).
#
# The count is then checked, and only three will do: fewer earns ``syntax
# error: arithmetic expression required``, more earns ``syntax error: `;'
# unexpected``, and either is followed by the header quoted back between the
# `((` and `))` the writer never typed twice. Both lines come from
# `parser_error` rather than `report_syntax_error`, so no source line is echoed
# under them, and both name the line the `((` was read on (`arith_for_lineno`,
# parse.y:4469) rather than the `done` the parser has reached by then.
#
# That it has reached the `done` at all is the other half: the header is
# counted in the grammar's *reduction* (parse.y:881), so the whole `do … done`
# is read first and a syntax error in the body is reported in its place.
#
# Verified against bash 5.2.37.

e() { ( eval "$1" ) 2>&1; echo "  rc=$?"; }

echo "=== a ; the scan steps over separates nothing ==="
e 'for (( ${x:-;}; 0; 0 )); do echo body; break; done'
e 'for (( "1;2"; 0; 0 )); do echo body; break; done'
e "for (( ' 1;2'; 0; 0 )); do echo body; break; done"
e 'for (( `echo 1;2`; 0; 0 )); do echo body; break; done'
e 'for (( $(echo 1;2); 0; 0 )); do echo body; break; done'
e 'for (( \;; 0; 0 )); do echo body; break; done'
# A nested `$(( … ))` needs no case of its own: the inner `(` is counted by the
# outer one.
e 'for (( $(( 1;2 )); 0; 0 )); do echo body; break; done'

echo "=== a ; it does not step over ends a section ==="
e 'for (( a[1;2]; 0; 0 )); do :; done'
e 'for (( $[1;2]; 0; 0 )); do :; done'
e 'for (( (1;2); 0; 0 )); do :; done'
e 'for (( 0?1;2:3; 0; 0 )); do :; done'
e 'for (( <(echo 1;2); 0; 0 )); do :; done'

echo "=== the count decides which complaint ==="
e 'for (( )); do :; done'
e 'for ((;)); do :; done'
e 'for ((0;0)); do :; done'
e 'for ((;;)); do echo inf; break; done'
e 'for ((0;0;0;)); do :; done'
e 'for ((;;;)); do :; done'
e 'for ((a;b;c;d)); do :; done'
# The header is quoted back exactly as written — its spacing, and its newlines.
e 'for ((  0 ;  0  )); do :; done'
e 'for ((0;
0)); do :; done'

echo "=== nothing runs of the unit that held it ==="
e 'echo before; for ((0;0)); do :; done; echo after'
e 'for ((0;0)); do :; done
echo next-unit'

echo "=== the body is read before the header is counted ==="
e 'for ((0;0)); do fi; done'
e 'for ((0;0)); do echo hi'
# …and a valid header is no different, which is what shows the ordering is the
# grammar's and not a special case.
e 'for ((0;0;0)); do fi; done'

echo "=== leading space and tab go, and nothing else ==="
# `declare -f` prints each section back from its first non-blank character
# onwards — trailing whitespace and all, and a leading newline is not blank
# enough to be dropped.
e 'f() { for ((  i=0 ; i<2 ; i++  )); do :; done; }; declare -f f'
e 'f() { for ((	i=0;0;0)); do :; done; }; declare -f f'
e 'f() { for ((
0;0;0)); do :; done; }; declare -f f'

echo done
