# An argument reaches an external command exactly as the shell expanded it.
#
# On a Unix host that is a tautology — `execve` takes an argv vector. On the
# Windows development host it is not: there is no argv, only a command line the
# parent writes and the child parses, and a Cygwin/MSYS child parses it by its
# own rules (backslash escapes, `"` delimits, and an *unquoted* argument is
# glob- and tilde-expanded by the child on the assumption no shell did it).
# So this file is a real test there and a formality here.  See
# `known-issues.md` TD-OILS-WIN-ARG-QUOTING.

# `env` is used rather than the builtin `printf` so the argument genuinely
# crosses the process boundary.
env printf '[%s]\n' 'a"b' 'e\f' 'q"r' 'back\' '\1' 'C:\dir\file' '""' '\\'

# Characters the *shell* would have acted on, had it seen them again: they were
# quoted here, so the child must receive them literally rather than expanded a
# second time on the far side.
env printf '[%s]\n' '*' '?' '~' '~root' '$v' '`x`' 'a;b|c&d' '(a)<b>c^d' '%PATH%'

# An empty argument is still an argument, and one with spaces is still one.
env printf '[%s]\n' '' 'a b' ' ' 'a  b '

# A backslash that is part of a regex, not a quoting artefact: these are the
# shapes that made external commands unusable in this corpus while arguments
# were mis-encoded.
printf 'SECONDS=3\n' | sed -E 's/^([A-Z]+)=[0-9]+$/\1=Q/'
printf 'y="3"\n' | sed -E 's/="[0-9]+"/=N/'
echo x | sed 's/x/\\/'
printf 'a\nb\n' | tr '\n' '~'
echo

# Bytes with no textual meaning at all: a tab and a newline inside one
# argument, and the high-bit bytes a UTF-8 locale would be tempted to rewrite.
env printf '[%s]\n' "$(printf 'a\tb')" "$(printf 'a\nb')" 'é±'

# Word splitting happens in the shell, so the child sees the count the shell
# decided on — not one the child re-derived from spaces in the command line.
set -- 'one two' 'three' ''
env printf '(%s)' "$@"; echo
env sh -c 'echo $#' _ "$@"
