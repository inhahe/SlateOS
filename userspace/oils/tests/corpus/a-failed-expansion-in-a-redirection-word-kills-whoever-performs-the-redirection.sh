# A word that fails to expand *fatally* — a `set -u` reference, a `${x:?}`, an
# invalid indirect — is not a redirection failure at all. bash gives up on the
# word where the error is, so:
#
#   - nothing more is said about it: the fields are never counted, and the
#     "ambiguous redirect" an empty expansion would otherwise earn is
#     unreachable;
#   - the partial text is never opened, so no file is created from a path the
#     redirect never named;
#   - and the error is *fatal to the process performing the redirection*.
#
# That last point is the whole shape of it. bash decides who performs the
# redirection before it does it, and forks for some commands and not others, so
# the same failing word ends the shell or merely fails a command depending on
# which side of the fork it landed on:
#
#   forked, so the shell lives   an external program; a `( … )` subshell; a null
#                                command whose list rebinds fd 0 or holds a
#                                `{v}>`; a pipeline stage
#   not forked, so the shell dies   a builtin; a function; a brace group;
#                                `while`/`if`/`for`; `exec`; a plain null command
#
# `command` is looked through — reaching past a function to the external is what
# it is for — but `builtin` is not, and neither is `command -v`.

echo '=== an external is forked, so only the child dies'
( set -u; cat < $nope; echo "rc=$?"; echo tail )
( set -u; /bin/true > $nope; echo "rc=$?"; echo tail )
( set -u; nosuchcmd > $nope; echo "rc=$?"; echo tail )
( set -u; x=1 cat > $nope; echo "rc=$?"; echo tail )

echo '=== a builtin and a function are run by the shell, so the shell dies'
( set -u; echo hi > $nope; echo "rc=$?"; echo tail )
( set -u; true > $nope; echo "rc=$?"; echo tail )
( set -u; f(){ :; }; f > $nope; echo "rc=$?"; echo tail )
( set -u; eval echo > $nope; echo "rc=$?"; echo tail )

echo '=== `command` reaches past the function to the external; `builtin` does not'
( set -u; command cat > $nope; echo "rc=$?"; echo tail )
( set -u; f(){ :; }; command f > $nope; echo "rc=$?"; echo tail )
( set -u; command echo > $nope; echo "rc=$?"; echo tail )
( set -u; command -v cat > $nope; echo "rc=$?"; echo tail )
( set -u; builtin echo > $nope; echo "rc=$?"; echo tail )
( set -u; builtin command cat > $nope; echo "rc=$?"; echo tail )
( set -u; command -p cat > $nope; echo "rc=$?"; echo tail )
( set -u; command -p echo > $nope; echo "rc=$?"; echo tail )
( set -u; command -pv cat > $nope; echo "rc=$?"; echo tail )
( set -u; command -x cat > $nope; echo "rc=$?"; echo tail )
( set -u; command -- cat > $nope; echo "rc=$?"; echo tail )
( set -u; command -- > $nope; echo "rc=$?"; echo tail )
( set -u; command > $nope; echo "rc=$?"; echo tail )
( set -u; command command command cat > $nope; echo "rc=$?"; echo tail )
( set -u; command() { :; }; command cat > $nope; echo "rc=$?"; echo tail )
( set -u; enable -n echo; echo hi > $nope; echo "rc=$?"; echo tail )

echo '=== `exec` redirects the shell itself, so there is nothing to confine it to'
( set -u; exec > $nope; echo "rc=$?"; echo tail )
( set -u; exec 3>&1 > $nope; echo "rc=$?"; echo tail )

echo '=== a subshell is forked before its list is applied; every other compound is not'
( set -u; ( echo hi ) > $nope; echo "rc=$?"; echo tail )
( set -u; { echo hi; } > $nope; echo "rc=$?"; echo tail )
( set -u; while false; do :; done > $nope; echo "rc=$?"; echo tail )
( set -u; if true; then :; fi > $nope; echo "rc=$?"; echo tail )
( set -u; for i in 1; do :; done > $nope; echo "rc=$?"; echo tail )

echo '=== a definition-time list wraps the body in the shell, so it dies too'
( set -u; f() { :; } > $nope; f; echo "rc=$?"; echo tail )
( set -u; f() { echo hi; } > $nope; ( f ); echo "rc=$?"; echo tail )

echo '=== a null command forks only for a list that rebinds fd 0 or holds a {v}>'
( set -u; < $nope;            echo "rc=$?"; echo tail )
( set -u; <> $nope;           echo "rc=$?"; echo tail )
( set -u; 0<&$nope;           echo "rc=$?"; echo tail )
( set -u; {v}> $nope;         echo "rc=$?"; echo tail )
( set -u; </dev/null > $nope; echo "rc=$?"; echo tail )
( set -u; 0<&- > $nope;       echo "rc=$?"; echo tail )
( set -u; > $nope;            echo "rc=$?"; echo tail )
( set -u; >> $nope;           echo "rc=$?"; echo tail )
( set -u; 3< $nope;           echo "rc=$?"; echo tail )
( set -u; 2< $nope;           echo "rc=$?"; echo tail )
( set -u; 0<&2 > $nope;       echo "rc=$?"; echo tail )
( set -u; 3>&- > $nope;       echo "rc=$?"; echo tail )
( set -u; <<< "[$nope]";      echo "rc=$?"; echo tail )

echo '=== the status is the expansion error own, put through the invocation mode'
( set -u; ( true > $nope; echo "in=$?" ); echo "out=$?" )
( set -u; ( cat < $nope; echo "in=$?" ); echo "out=$?" )
( set -u; ( ( true > $nope ); echo "mid=$?" ); echo "out=$?" )
( set -u; v=$( true > $nope ); echo "out=$? v=[$v]" )

echo '=== a `${x:?}` is the same error, and a bad indirect the same shape at 1'
( set -u; true > "${nope:?boom}"; echo "rc=$?"; echo tail )
( set -u; cat < "${nope:?boom}"; echo "rc=$?"; echo tail )
( true > "${!bad}"; echo "rc=$?"; echo tail )
( cat < "${!bad}"; echo "rc=$?"; echo tail )
( < "${!bad}"; echo "rc=$?"; echo tail )
( f(){ :; }; f > "${!bad}"; echo "rc=$?"; echo tail )
( ( true > "${!bad}" ); echo "rc=$?"; echo tail )
( eval 'true > "${!bad}"'; echo "rc=$?"; echo tail )

echo '=== a here body is a redirection word too, and cannot fail as a redirect'
( set -u; cat <<< "[$nope]"; echo "rc=$?"; echo tail )
( set -u; read v <<< "[$nope]"; echo "rc=$?"; echo tail )
( set -u; cat <<EOF
[$nope]
EOF
echo "rc=$?"; echo tail )

echo '=== nothing further is said, so no ambiguity complaint is ever reached'
( set -u; true > $nope 2>&1; echo "rc=$?" )
( set -u; a=(x y); true > "${a[@]}$nope"; echo "rc=$?" )

echo '=== and the partial text is never opened'
mkdir one two
( set -u; true > "$nope"one/x )
( set -u; exec > "$nope"one/y )
echo "created=[$(ls one)]"
( set -u; cat </dev/null > "$nope"two/z )
( set -u; {v}> "$nope"two/w )
echo "created=[$(ls two)]"

echo '=== a failure that really is the redirection is untouched, and only fails the command'
( true > $nope; echo "rc=$?"; echo tail )
( a=(1 2); true > $a; echo "rc=$?"; echo tail )
( a=(1); true > "${a[-9]}"; echo "rc=$?"; echo tail )
( true > /nodir/f; echo "rc=$?"; echo tail )
( set -C; :> f1; true > f1; echo "rc=$?"; rm -f f1; echo tail )

echo '=== an earlier redirect in the same list still took effect'
mkdir three
( set -u; true 3> three/made > $nope )
echo "made=[$(ls three)]"

echo '=== done'
