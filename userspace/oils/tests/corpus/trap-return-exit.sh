# RETURN fires for a returning function under functrace; its `exit` unwinds.
set -T
trap 'echo RET; exit 4' RETURN
f() { echo in; }
f
echo after
