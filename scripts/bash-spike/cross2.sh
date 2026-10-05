#!/bin/bash
# Cross-compile bash 5.2 -> x86_64-linux-musl (the ABI SlateOS's libc.a targets).
#
# Fixes over cross.sh: the compiler is reached through a wrapper on a path with
# no spaces (configure word-splits $CC, and the repo lives under "visual studio
# projects"), and the whole build happens on the Linux filesystem — /mnt/d is
# both space-laden and slow.
. "$(dirname "${BASH_SOURCE[0]}")/../lib/worktree.sh" || exit 1

set -x
SPIKE="$SLATE_SPIKE"
# Lane-keyed: cross2.sh writes this tree and slatelink.sh reads it, so two lanes
# sharing one bash-cross directory would relink one lane's objects into the
# other's shipped bash-slateos.elf. Keep this name in step with cross3.sh and
# slatelink.sh, which must agree on it.
#
# Under $SLATE_WORK, not /tmp: WSL wipes /tmp on every restart, which silently
# broke the automatic relink in create-ext4-rootfs.sh (see worktree.sh).
BUILD="$SLATE_WORK/bash-cross"
slate_adopt_legacy_work "/tmp/bash-cross-$SLATE_LANE" "$BUILD"

# The toolchain comes from slate_make_zig_wrappers, which resolves the pinned,
# hash-verified zig (per-worktree copy, else the shared cache, else download).
#
# This script used to require zig at "$SPIKE/zig/zig" and then hand-roll its own
# /tmp/zigcc wrappers. Both halves were wrong once worktree.sh started pinning
# the toolchain on 2026-08-16, and the fix that day only converted the *path*
# derivation here, not the provisioning -- so this script still demanded a
# hand-placed per-worktree zig and still wrote un-lane-keyed wrappers naming it.
# The stale /tmp/zigcc left over from 2026-08-14 pointed at
# `os/build/spike/zig/zig`, so a lane running this relinked a *shipped* artifact
# (design-decisions.md §305) with another worktree's unrecorded, unverified
# compiler -- the exact defect that day's work was supposed to close.
slate_make_zig_wrappers || exit 1
"$SLATE_ZIG" version || exit 1
"$SLATE_CC" --version || exit 1

# Sanity: can the wrapper build a hello-world at all?
echo 'int main(void){return 0;}' > "$SLATE_TMP/t.c"
"$SLATE_CC" "$SLATE_TMP/t.c" -o "$SLATE_TMP/t.out" && echo "WRAPPER_LINKS_OK" || { echo "WRAPPER_BROKEN"; exit 1; }

# The source is provisioned the way the compiler is — pinned by version and
# sha256, fetched from ftp.gnu.org when this machine has not got it. It used to
# be read straight from "$SPIKE/bash-5.2.tar.gz", a gitignored path that nothing
# in the tree ever wrote, so this script was unrunnable anywhere the tarball had
# not been dropped in by hand. See slate_ensure_bash_src in scripts/lib/worktree.sh.
slate_ensure_bash_src || exit 1

rm -rf "$BUILD"
mkdir -p "$BUILD"
tar xzf "$SLATE_BASH_TARBALL" -C "$BUILD" --strip-components=1 || exit 1
cd "$BUILD" || exit 1

export CC="$SLATE_CC" AR="$SLATE_AR" RANLIB="$SLATE_RANLIB"
# --disable-readline drops termcap (9 of the 23 unresolved symbols); a spike only
# needs `bash -c` and script execution to prove the port is real.
#
# bash_cv_getcwd_malloc=yes is the answer configure would find if it could run
# its test, which a cross build cannot: getcwd(NULL, 0) allocates, in our libc
# as in musl. Without it configure guesses "no" and compiles lib/sh/getcwd.c,
# a getcwd that walks `..` matching inode numbers -- which procfs, sysfs and
# devfs report as 0 for every entry, so in them it names the wrong directory --
# in place of ours, which reads the kernel's record of the directory. Ours
# lost to it once the link kept bash's own order, lib/sh/libsh.a ahead of the
# C library, on 2026-10-01; before that, zig's driver had moved -lsh behind
# our libc.a and ours won by accident. ac_cv_func_working_mktime=yes for the
# same reason: its test runs a program too, and the guess "no" compiles
# lib/sh/mktime.c, which nothing in bash calls today, and which the first
# call would reach instead of ours.
#
# The rest, 2026-10-05: each is what its configure test answers -- or would,
# run on SlateOS -- measured from our libc and kernel rather than guessed
# (known-issues-resolved/D-SPIKES-BASH-CROSS-CONFIGURE-GUESSED-WHAT-IT-COULD-NOT-RUN.md):
#
#   bash_cv_wexitstatus_offset=8  our wait status is Linux's, exit code in bits
#       8-15 (posix/src/process.rs: exit 1 is 256). The guess, 0, made the
#       status `lastpipe` synthesises for a pipeline's last command (jobs.c,
#       append_process) read as a death by signal: `shopt -s lastpipe;
#       true | false; echo $?` said 129, and `true | (exit 3)` 131 -- measured
#       with this build's own bash under WSL, whose wait status is Linux's
#       too, where Ubuntu's bash says 1 and 3.
#   bash_cv_printf_a_format=yes   our printf's %A, %a and the long-double %LA
#       bash's builtin uses are glibc 2.39's byte for byte (posix/src/printf.rs,
#       its conversion oracle); the guess turned the builtin's %a off.
#   bash_cv_unusable_rtsigs=no    the test asks only that SIGRTMIN be under
#       2*NSIG, and ours is 32; the guess dropped RTMIN..RTMAX from kill and trap.
#   bash_cv_sys_named_pipes=present  the test answers from mkfifo existing,
#       which ours does. It answers ENOSYS for now (the filesystem holds no
#       FIFO), so process substitution fails saying so -- and starts working,
#       with no rebuild, the day FIFOs do.
#   bash_cv_dev_fd=absent, bash_cv_dev_stdin=absent  configure reads these
#       from the *build* machine's /dev, and SlateOS has no /dev/fd, its
#       /dev/stdin, /dev/stdout and /dev/stderr are the console rather than the
#       process's descriptors (kernel/src/fs/devfs.rs), and /proc/self/fd/N
#       names nothing for a native process. Absent, bash opens those names
#       itself in its redirections (redir.c), as their descriptors: `echo x >
#       /dev/stderr` reaches fd 2 and not the console.
./configure --host=x86_64-linux-musl --build=x86_64-pc-linux-gnu \
    --without-bash-malloc --disable-nls --disable-readline --without-curses \
    bash_cv_getcwd_malloc=yes ac_cv_func_working_mktime=yes \
    bash_cv_wexitstatus_offset=8 bash_cv_printf_a_format=yes \
    bash_cv_unusable_rtsigs=no bash_cv_sys_named_pipes=present \
    bash_cv_dev_fd=absent bash_cv_dev_stdin=absent \
    >cross-configure.log 2>&1
echo "CROSS_CONFIGURE_EXIT=$?"
tail -15 cross-configure.log

make -j8 >cross-make.log 2>&1
echo "CROSS_MAKE_EXIT=$?"
tail -25 cross-make.log

if [ -x "$BUILD/bash" ]; then
  file "$BUILD/bash"
  ls -l "$BUILD/bash"
  echo "CROSS_BASH_BUILT"
  cp "$BUILD/bash" "$SPIKE/bash-musl.elf"
else
  echo "NO_CROSS_BINARY"
fi
