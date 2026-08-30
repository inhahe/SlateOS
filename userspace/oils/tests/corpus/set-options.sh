# `set` options that change error behaviour: -u (nounset), -C (noclobber),
# -f (noglob), and how `set -o`/`set +o` report and restore them.
# Each fatal-under-`set -u` probe runs in a subshell so the script survives it.

# -u: expanding an unset variable is a fatal error (status 1 from the subshell),
# while an explicitly-empty one is fine and `${x-default}` still works.
empty=
(set -u; echo "empty=[$empty]")
echo "empty-status=$?"
(set -u; echo "default=${nosuch-ok}")
echo "default-status=$?"
(set -u; echo "boom=$nosuch") 2>/dev/null
echo "unset-status=$?"
# "$@" with no positional parameters is NOT an unset-variable error.
(set -u; set --; echo "at=[$*] n=$#")
echo "at-status=$?"

# -C (noclobber): `>` refuses to truncate an existing file, `>|` overrides it,
# and `>>` is unaffected.
echo original > nc.txt
set -C
echo overwrite > nc.txt 2>/dev/null
echo "noclobber-status=$? contents=[$(cat nc.txt)]"
echo forced >| nc.txt
echo "forced-status=$? contents=[$(cat nc.txt)]"
echo appended >> nc.txt
echo "append-contents=[$(cat nc.txt | tr '\n' ',')]"
# A *new* file is still creatable under -C.
echo fresh > nc2.txt
echo "new-file-status=$? contents=[$(cat nc2.txt)]"
set +C

# -f disables pathname expansion entirely, so a glob stays literal even when it
# would have matched.
touch g1.txt g2.txt
echo "glob-on=$(echo *.txt)"
set -f
echo "glob-off=$(echo *.txt)"
set +f
echo "glob-back=$(echo *.txt)"

# `set -o` reports the current state; `set +o` prints commands that restore it.
set -u
echo "nounset-on=$(set -o | grep '^nounset' | tr -s ' ' ' ')"
set +u
echo "nounset-off=$(set -o | grep '^nounset' | tr -s ' ' ' ')"
echo "plus-o-has-nounset=$(set +o | grep -c 'nounset')"

# $- carries the single-letter flags of the current shell.
set -f
case "$-" in *f*) echo "dash-has-f=yes" ;; *) echo "dash-has-f=no" ;; esac
set +f
case "$-" in *f*) echo "dash-still-f=yes" ;; *) echo "dash-still-f=no" ;; esac

# -e is scoped to the shell that set it: a subshell inherits it, but a command
# whose status is *tested* never triggers it.
(set -e; false; echo "not reached") 2>/dev/null
echo "errexit-subshell-status=$?"
(set -e; if false; then :; fi; false || true; echo "tested-ok")
echo "tested-status=$?"
