# `return` sets the function's status; a bare `return` reuses `$?`.
f() { return 3; }
f
echo "a=$?"
g() { false; return; }
g
echo "b=$?"
h() { echo out; }
h
echo "c=$?"
