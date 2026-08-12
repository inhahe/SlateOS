# Turning `extdebug` on seeds `BASH_ARGC`/`BASH_ARGV` with a base frame holding
# the shell's positional parameters — but only the *first* time. bash's turn-on
# handler calls `init_bash_argv`, whose whole body sits under a
# `bash_argv_initialized` flag:
#
#      void init_bash_argv (void)
#      { if (bash_argv_initialized == 0) { save_bash_argv (); bash_argv_initialized = 1; } }
#
# so a second OFF→ON transition seeds nothing. The guard is load-bearing rather
# than thrifty. A call pops only the frame its own call pushed, so a *function*
# that turns extdebug off and on again would otherwise return leaving the extra
# base behind — and the stack would grow by one for every such round, forever.
#
# The base is also a snapshot: a later `set --` does not move it, and turning
# extdebug off does not clear the stack (only the frames' own pops do). What
# `shopt -u extdebug` does hide is the *live* part — the frames pushed by calls
# made while it was off are never pushed at all.
#
# Everything here turns `extdebug` on first, or says when it does not. Without
# it osh populates neither array — a deliberate, documented divergence; see
# known-issues.md TD-OILS-MISSING-SPECIAL-ARRAYS.

set -- x y z
p() { echo "c=[${BASH_ARGC[*]}] v=[${BASH_ARGV[*]}]"; }

echo "--- the first on captures the positionals as a base frame"
shopt -s extdebug; p

echo "--- and no later on captures anything, however the positionals have moved"
t1() { set -- 1 2; shopt -u extdebug; shopt -s extdebug; p; }; t1
t2() { shopt -u extdebug; shopt -s extdebug; shopt -u extdebug; shopt -s extdebug; p; }; t2
t3() { shopt -s extdebug; p; }; t3

echo "--- so a function that toggles leaves nothing behind"
t4() { f() { shopt -u extdebug; shopt -s extdebug; }; f q; p; }; t4
t5() { f() { shopt -u extdebug; }; f q; shopt -s extdebug; p; }; t5
t6() { f() { shopt -u extdebug; shopt -s extdebug; shopt -u extdebug; shopt -s extdebug; }; f; f; f; p; }; t6

echo "--- off does not clear what is already there"
t7() { shopt -u extdebug; echo "c=[${BASH_ARGC[*]}] v=[${BASH_ARGV[*]}]"; shopt -s extdebug; }; t7
t8() { shopt -u extdebug; declare -p BASH_ARGC; shopt -s extdebug; }; t8

echo "--- …but a call made while it is off pushes no frame at all"
t9() { shopt -u extdebug; f() { :; }; f a b; shopt -s extdebug; p; }; t9
ta() { shopt -u extdebug; f() { shopt -s extdebug; p; }; f a b; }; ta

echo "--- a subshell's toggling does not reach the shell that made it"
tb() { ( shopt -u extdebug; shopt -s extdebug; p ); p; }; tb
tc() { ( shopt -u extdebug; f() { :; }; f a; shopt -s extdebug; p ); p; }; tc

echo "--- and the base still sits under every live frame"
td() { f() { p; }; f a b c; }; td
te() { f() { shopt -u extdebug; shopt -s extdebug; g() { p; }; g w; }; f a; }; te
