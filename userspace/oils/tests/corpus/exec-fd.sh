# `exec` with only redirections rewires the *current* shell's file descriptors,
# and the {var}>… form lets the shell pick a free fd. Everything here writes to
# files in the case's own cwd so the ordering is deterministic.

# `exec` with no command applies its redirections permanently.
exec 3> out3.txt
echo to-fd-3 >&3
echo to-fd-3-again >&3
exec 3>&-            # close it
echo "closed-status=$?"
cat out3.txt
echo write-after-close >&3 2>/dev/null; echo "write-closed-status=$?"

# Reading through a saved fd: fd 4 keeps its own file offset across commands.
printf 'l1\nl2\nl3\n' > in4.txt
exec 4< in4.txt
read -r line <&4; echo "first=$line"
read -r line <&4; echo "second=$line"
exec 4<&-

# Saving and restoring stdout is the classic use.
exec 5>&1
exec 1> redirected.txt
echo this-goes-to-the-file
exec 1>&5 5>&-
echo this-goes-to-the-terminal
cat redirected.txt

# {var}> allocates a descriptor and stores its number in the variable. The
# number itself is host-dependent (bash starts at 10), so only its properties
# are checked.
exec {fd}> auto.txt
echo "auto-fd-ge-10=$(( fd >= 10 ))"
echo via-auto >&$fd
exec {fd}>&-
cat auto.txt

# A redirection on a compound command applies to everything inside it.
{ echo grouped-1; echo grouped-2; } > group.txt
cat group.txt
for i in 1 2; do echo "loop-$i"; done > loop.txt
cat loop.txt

# `>&` and `&>` both send stdout+stderr to the same place; the order of `2>&1`
# relative to `>file` matters.
noisy() { echo out; echo err >&2; }
noisy > both1.txt 2>&1
cat both1.txt
noisy 2>&1 > both2.txt
echo "--- both2:"; cat both2.txt
noisy &> both3.txt
cat both3.txt

# Appending vs truncating.
echo first > trunc.txt
echo second > trunc.txt
cat trunc.txt
echo first > app.txt
echo second >> app.txt
cat app.txt

# `<>` opens for both reading and writing without truncating.
printf 'ABCDEF' > rw.txt
exec 6<> rw.txt
printf 'xy' >&6
exec 6>&-
cat rw.txt; echo

# noclobber makes `>` refuse to overwrite an existing file; `>|` overrides it.
set -C
echo clobber-me > nc.txt
echo second > nc.txt 2>/dev/null; echo "noclobber-status=$?"
cat nc.txt
echo forced >| nc.txt; cat nc.txt
set +C
echo unset-noclobber > nc.txt; cat nc.txt

# Redirecting a builtin does not affect the shell afterwards.
echo before-builtin-redir
: > /dev/null
echo after-builtin-redir

# A failed redirection makes the command not run, and in a non-interactive
# shell without errexit it merely sets a nonzero status.
echo should-not-run > nodir/nofile.txt 2>/dev/null; echo "bad-redir-status=$?"

rm -f out3.txt in4.txt redirected.txt auto.txt group.txt loop.txt \
      both1.txt both2.txt both3.txt trunc.txt app.txt rw.txt nc.txt
