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
# A trailing `/` (or, on Windows, `\`) is the other half of this. POSIX makes it
# an assertion that the name in front of it resolves to a directory, so `dir/`
# is the directory, `nosuch/` is ENOENT and — the shape this file was written
# for — a word that ends in a separator because a substitution put one there is
# still just a path that is not there:
#
#     x=$(printf 'b\\'); "$x"      # bash: b\: No such file or directory, 127
#
# The exit statuses are the interesting part: `Is a directory` is 126 and the
# missing ones are 127, and both are decided by the file rather than by the
# error the host handed back.
#
# Verified against bash 5.2.37.

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
