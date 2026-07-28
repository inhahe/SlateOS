# A syntax error names the offending word by its *source spelling*, quotes and
# all — `[[ a '<' b ]]` is reported near `'<'`, not near some placeholder. Each
# probe runs in its own subshell so the error does not take the script with it.
echo "=== every quoting of the offending word"
( eval "[[ a '<' b ]]" ); echo "rc=$?"
( eval '[[ a "<" b ]]' ); echo "rc=$?"
( eval '[[ a \< b ]]' ); echo "rc=$?"
( eval '[[ a $x-q b ]]' ); echo "rc=$?"
( eval "[[ -n a ']]' ]]" ); echo "rc=$?"
# Not probed: `$'…'`, whose source spelling osh does not keep — see
# TD-OILS-ANSIC-ERROR-SPELLING in known-issues.md.

echo "=== a bare word is still spelled plainly"
( eval '[[ a -q b ]]' ); echo "rc=$?"
( eval '[[ "a b" -q c ]]' ); echo "rc=$?"

echo "=== and outside a conditional"
( eval 'echo hi; ;' ); echo "rc=$?"
( eval 'for i in a; do done' ); echo "rc=$?"
( eval 'case x in y) esac; f() { :; } z' ); echo "rc=$?"
