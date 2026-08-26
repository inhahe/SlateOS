# A command word that spelled a *path* is handed to the OS unresolved, and when
# the OS refuses it bash does not simply print the errno. `shell_execve` asks
# the filesystem what the file is first:
#
#     last_command_exit_value = (i == ENOENT) ?  EX_NOTFOUND : EX_NOEXEC;
#     if (file_isdir (command))
#       internal_error (_("%s: %s"), command, strerror (EISDIR));
#     else if (executable_file (command) == 0)
#       { errno = i; file_error (command); }        /* execute_cmd.c:5970-5981 */
#
# so a *directory* reads `Is a directory` however the kernel spelled its
# refusal — and every kernel spells it as something else: `execve` on a
# directory is EACCES on Linux and ERROR_ACCESS_DENIED on Windows, i.e.
# `Permission denied` on both. Only ENOENT exits 127; everything else here is
# 126.
#
# A trailing `/` is the other half of this. POSIX makes it an assertion that the
# name in front of it resolves to a directory, so `dir/` is the directory,
# `nosuch/` is ENOENT and — the shape this file was written for — a word that
# ends in a separator because a substitution put one there is still just a path
# that is not there.
#
# A trailing **backslash** is that shape only on Windows. Everywhere else — and
# on SlateOS, whose paths admit every byte but `/` and NUL — `\` is an ordinary
# filename byte, so a word carrying one has no separator in it at all and is a
# plain `$PATH` lookup:
#
#     x=$(printf 'b\\'); "$x"      # bash: b\: command not found, 127
#     ddir\                        # bash: ddir\: command not found, 127
#
# even though `ddir` is a directory sitting right there. Both lines read `No
# such file or directory` here until 2026-08-26, which was not a bash fact but
# an MSYS one: this corpus's reference bash was an MSYS build, whose runtime
# takes a backslash for a separator like the Windows host underneath it.
#
# The exit statuses are the interesting part: `Is a directory` is 126 and the
# missing ones are 127, and both are decided by the file rather than by the
# error the host handed back.
#
# Verified against bash 5.2.21 (glibc). The `\` lines differ under an MSYS or
# Windows-hosted reference, where the byte really is a separator.

mkdir -p ddir
r() { printf '== %-11s' "$1"; "$2"; printf 'rc=%s\n' "$?"; }

echo "=== 1. a name that is not there, with a separator stuck on the end"
r 'slash'   'nosuch/'
r 'bslash'  'nosuch\'
r 'deep'    './ddir/x/'
# The shape that started this: the trailing byte came out of a substitution.
x=$(printf 'b\\')
r 'from sub' "$x"
# Without the separator the host says ENOENT itself and nothing needs correcting.
r 'plain'   './nosuch'

echo "=== 2. a directory, named plainly or with the separator"
r 'dir'     './ddir'
r 'dir/'    'ddir/'
r 'dir\'    'ddir\'
r 'dot'     './'
r 'root'    '/'

echo "=== 3. a bare word is a \$PATH lookup and never a file"
# No separator in it, so nothing is stat'ed: it is the `command not found` 127.
r 'bare'    'nosuchcmd'
r 'empty'   ''

echo "=== 3b. a backslash is a filename byte, so a name holding one is bare"
mkdir -p bsdir
printf 'echo bs-ran\n' > 'bsdir/we\ird.sh'; chmod +x 'bsdir/we\ird.sh'
# Found on `$PATH` by that name, because the word never looked like a path.
OLDPATH=$PATH; PATH=bsdir:$PATH
r 'off PATH' 'we\ird.sh'
PATH=$OLDPATH; unset OLDPATH
# And named as a path, where the backslash is just part of the last component.
r 'as path' './bsdir/we\ird.sh'
printf '== %-11s' 'type -P'; type -P './bsdir/we\ird.sh'; printf 'rc=%s\n' "$?"
# The same word with nothing behind it is a lookup that fails, not a stat.
r 'no such'  'no\such.sh'

echo "=== 4. the same answers off the pipeline and the background paths"
./ddir | cat
echo "rc=$?"
'nosuch/' | cat
echo "rc=$?"
./ddir &
wait
'nosuch/' &
wait

echo done
