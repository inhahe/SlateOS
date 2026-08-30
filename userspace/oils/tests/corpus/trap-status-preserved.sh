# A synchronous handler must not clobber `$?` for the command that triggered it.
trap 'true' ERR
false
echo "status=$?"
trap 'false' DEBUG
true
echo "status2=$?"
