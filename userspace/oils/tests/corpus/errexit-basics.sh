# errexit: which failures abort and which are exempt.
set -e
if false; then echo no; fi
echo after-if
false || echo after-or
! false
echo after-bang
while false; do :; done
echo after-while
false
echo unreachable
