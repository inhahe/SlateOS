# A `$PATH` search is not "find the first executable, else nothing". bash's
# `find_user_command_in_path` also keeps the *first* candidate that exists but
# is not executable — `file_to_lose_p` — and hands that one back when the walk
# ends having found nothing runnable, with `errno` set to `EACCES`:
#
#     if (file_to_lose_p) { errno = EACCES; return (file_to_lose_p); }
#
# So a name matched only by a file whose execute bit is off is not `command not
# found` at all. It is that file; the *spawn* of it is what fails, which is why
# the message names the path the search built rather than the word, and why the
# status is 126 rather than 127.
#
# The losing slot is first-come and takes anything `stat` answers for,
# directories included — but a directory cannot *be* the answer, so a name whose
# first match is a directory is not found even when a plain unexecutable file of
# the same name follows in a later entry. An executable hidden by `$EXECIGNORE`
# is skipped without claiming the slot at all.
#
# Which callers see the losing candidate is not uniform, and that is the point of
# section 3: running it does, plain `type`/`command -v`/`type -P` do, and the run
# even *hashes* it — while `type -a` and the `hash` builtin's own search, which
# ask only about executables, say `not found` for the very same name.
#
# `$PATH` is relative here so the reported candidate is `a/only` rather than a
# temporary directory nobody can predict; bash joins the entry exactly as
# written.
#
# Within the frozen scope (README, §305) on the second criterion: osh answered
# `command not found` and **127** where bash answers `Permission denied` and
# **126**, and a status that says "no such command" for a file that is right
# there is one a caller acts on — an installer that chmods and retries, a `||`
# fallback that installs a package. The wording is incidental; the status is not.
#
# Verified against bash 5.2.21.

mkdir -p a b
r() { printf '== %-11s' "$1"; shift; "$@"; printf 'rc=%s\n' "$?"; }

printf 'echo A-ran\n' > a/only;  chmod -x a/only
printf 'echo B-ran\n' > b/only;  chmod -x b/only
printf 'echo A-ran\n' > a/shadow; chmod -x a/shadow
printf 'echo B-ran\n' > b/shadow; chmod +x b/shadow
mkdir -p a/both;  printf 'echo B-ran\n' > b/both; chmod -x b/both
mkdir -p a/dx;    printf 'echo B-ran\n' > b/dx;   chmod +x b/dx
mkdir -p a/lonedir
printf 'echo A-ran\n' > a/ign; chmod +x a/ign
printf 'echo B-ran\n' > b/ign; chmod -x b/ign

export PATH=a:b

echo "=== 1. nothing runnable: the first thing that was not"
r 'only'    only
r 'shadow'  shadow

echo "=== 2. a directory takes the slot but cannot fill it"
r 'both'    both
r 'dx'      dx
r 'lonedir' lonedir

echo "=== 3. who sees the losing candidate"
hash -r
r 'type'      type only
r 'command -v' command -v only
r 'type -P'   type -P only
r 'type -t'   type -t only
r 'type -a'   type -a only
r 'hash'      hash only
echo "-- nothing above hashed it; a run does"
hash
only 2>/dev/null
hash
r 'type'      type only
r 'type -a'   type -a only
hash -r

echo "=== 4. an EXECIGNOREd executable does not become the losing candidate"
# a/ign is executable but hidden; b/ign is a plain file. Hiding a/ign must not
# put it in the losing slot, and must not stop b/ign from claiming it.
EXECIGNORE='a/ign'
r 'ign'       ign
hash -r
EXECIGNORE='a/ign:b/ign'
r 'both-ig'   ign
hash -r
unset EXECIGNORE

echo "=== 5. a word that spelled a path is run but not described"
r 'run'       ./a/only
r 'type'      type ./a/only
r 'command -v' command -v ./a/only
r 'type -a'   type -a ./a/only

echo "=== 6. a handler is for a name nothing was found for"
command_not_found_handle() { echo "HANDLER $1"; }
r 'only'      only
r 'lonedir'   lonedir
r 'nosuch'    nosuchcmd
unset -f command_not_found_handle

echo done
