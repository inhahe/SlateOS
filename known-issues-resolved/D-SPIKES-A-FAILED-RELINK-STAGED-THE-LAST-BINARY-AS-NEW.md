## D-SPIKES-A-FAILED-RELINK-STAGED-THE-LAST-BINARY-AS-NEW — when a port's relink failed, its script staged the previous binary with a fresh timestamp, and the image's staleness gate let it through (lane D, 2026-10-01)

**Status:** FIXED 2026-10-05

**In short:** the image build relinks each ported program whenever our C
library changes, and refuses a program older than the library. When
bash's relink failed, its script copied the last good bash -- linked
against an older library -- into place anyway, with today's date on it, so
the refusal never fired. Every image built since 2026-09-30 in a worktree
that holds bash's build tree carried a bash that tested an older library,
while its self-test passed.

**What was measured:** `build/spike/bash-slateos.elf` dated 2026-10-01
15:39; the binary it was copied from, in bash's build tree, 2026-09-29
23:06; and `slate-link.log` beside that: `duplicate symbol:
glob_pattern_p`, our `libc.a` against bash's `lib/glob/libglob.a`.
`glob_pattern_p` joined our library on 2026-09-30 (11a8d6d6b), in a member
of its own -- but zig's cc driver had moved bash's `-lglob` behind our
`libc.a` (known-issues D-SPIKES-LINK-ZIGS-MUSL-BEHIND-OUR-LIBC), so ours
was taken first, and bash's `glob.o`, extracted for bash's other glob
functions, brought a second. ld.lld leaves an existing output alone when a
link fails; `scripts/bash-spike/slatelink.sh` then staged whatever
`bash-slateos` it found; and `create-ext4-rootfs.sh` compares the staged
file's mtime with libc.a's, which `cp` had just made new.

**Where else it could happen:** `scripts/cpython-spike/slatelink.sh`
(staged `python-slateos` if it existed), `scripts/espeak-spike/run.sh` (its
check missed a link that failed on duplicates alone) and
`scripts/cmake-spike/run.sh` (staged `cmake-slateos` if it existed). make
and pkgconf unpack a fresh tree on every run, coreutils empties its output
directory, and eSpeak's relink script checks the link's exit code.

**The fix:** each script removes its link output before linking, so a
failed link leaves nothing to stage, and exits non-zero when there is
nothing -- which `spike_rebuild_if_behind` reports with the log's last
lines, and stops the image on. bash's link keeps bash's own order now
(`slate_make_link_wrappers`), and links (known-issues
D-POSIX-GETENV-AND-GETCWD-COULD-NOT-BE-REPLACED).
