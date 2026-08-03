# bash normally describes every builtin the same way — `NAME is a shell builtin`
# — but in posix mode it calls out the sixteen special ones: `NAME is a special
# shell builtin`. That is the only place the distinction is ever *spoken*, and
# it is only spoken in posix mode, because that is the only mode where being
# special means anything (the special builtins are found before functions there,
# and their failures end the shell).
#
# It reaches `type`, `type -a` and `command -V` — the three that describe a name
# in words. `type -t` is unaffected: its answer is the machine-readable
# `builtin` either way, as is `command -v`'s bare name.

echo "=== outside posix mode there is no such thing as special"
for n in unset export set eval : . source readonly shift trap exit exec break continue return times; do
  type "$n"
done
type cd
command -V unset

echo "=== in posix mode the sixteen say so"
set -o posix
for n in unset export set eval : . source readonly shift trap exit exec break continue return times; do
  type "$n"
done

echo "=== …and only the sixteen"
for n in cd read echo printf local declare let command builtin test; do
  type "$n"
done

echo "=== the same for -a and for command -V, but not for -t or -v"
type -a unset
type -a unset export cd
command -V unset
echo "type -t:  $(type -t unset)"
echo "type -at: $(type -at unset)"
echo "command -v: $(command -v unset)"

echo "=== leaving the mode takes the word away again"
set +o posix
type times
set -o posix
type times

echo "=== a function is still described as a function, and still listed first"
set +o posix
unset() { :; }
POSIXLY_CORRECT=1
type unset
type -a unset
command -V unset
echo "command -v: $(command -v unset)"

# `enable -n` puts the function back in front, so the `unset -f` below is the
# function itself — a no-op — and the function survives its own removal.
echo "=== disabling the builtin hands the name back to the function"
enable -n unset
unset -f unset
type -a unset; echo "  rc=$?"

echo "=== and with no function left there is nothing to describe"
enable unset
unset -f unset
type -a unset; echo "  rc=$?"
command -V unset; echo "  rc=$?"
enable -n unset
type -a unset; echo "  rc=$?"
command -V unset; echo "  rc=$?"
