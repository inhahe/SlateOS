#!/usr/bin/env bash
# Differential test: our df against GNU df.
#
# ## The problem this harness has and the others do not
#
# Every other `*-diff.sh` compares two programs reading the same *file*. `df`
# reads the machine: the mount table, and the free-space counters of every file
# system on it. Neither is an input the harness controls, and the second one
# moves while you look at it. Run GNU `df /` and then ours a millisecond later
# and the two disagree — not because either is wrong, but because writing GNU's
# output to a scratch file allocated blocks on the very file system being
# measured. A naive harness here reports a permanent, meaningless failure.
#
# Two mechanisms answer that, and both are load-bearing:
#
#   1. **A private mount namespace with file systems we own.** The harness
#      re-execs itself under `unshare -mUr` and mounts several `tmpfs`
#      instances of known size at known paths. Nothing writes to them during a
#      case, so their numbers are *identical* across the two runs and can be
#      compared byte for byte — which is the only way a block-size division or
#      a percentage rounding rule can actually be checked. The user-namespace
#      form is used because WSL's default user is unprivileged: plain
#      `unshare -m` fails with EPERM here, `unshare -mUr` succeeds.
#
#   2. **Digit masking for the rest.** A case that reports on a file system we
#      do not own — `df` with no operands, `df -a`, anything naming `/` — is
#      compared with `[0-9]` rewritten to `#`. That still tests everything this
#      transcription can get wrong about the *table*: which rows appear, in
#      what order, under which headings, at what column widths, with which
#      source and type strings, and it still notices a digit appearing or
#      disappearing. It cannot notice a wrong *value*, which is what the tmpfs
#      cases are for. A case is masked by prefixing it with `~`.
#
# Masking is a real, if small, flake risk: it preserves digit *count*, so a
# host file system that crosses a power of ten between the two runs changes a
# column width and the case fails. That is rare enough to be worth the signal,
# and it fails loudly rather than silently, which is the right way round.
#
# ## What the namespace holds
#
# The mounts are chosen so that each one makes a rule of `df`'s observable:
#
#   fs/a          4 MiB tmpfs with 100 KiB written into it, so Used, Avail and
#                 Use% are all non-zero and none of them coincide
#   fs/a/inner    a tmpfs mounted *inside* another, which is what tells the
#                 over-mount check apart from a plain lookup: `df fs/a/inner`
#                 must report the inner one
#   fs/b          64 MiB with a low inode limit, so `-i` has something to say
#                 that is not proportional to the block figures
#   fs/bind       a bind mount of fs/a: two table entries, one device, which is
#                 the duplicate-filtering rule
#   fs/dup        two tmpfs mounted at the same path, so the table has a
#                 shadowed entry that only the second of them is reachable
#                 through
#   fs/sp ace     a space in the mount point, which both table grammars escape
#   fs/n\377ame   a byte that is not UTF-8, which is the case the old df could
#                 not be handed at all: it read /proc/mounts as a String
#
# The host's own table is inherited and is itself a useful fixture — this WSL
# has a 9p mount whose source is `C:\134Program\040Files\134Docker\...`, four
# escapes in one field, and a dozen `snapfuse` mounts that exercise the dummy
# and duplicate rules at a scale no hand-built fixture would reach.
#
#     sh scripts/df-diff.sh                    # run it
#     OURS=/usr/bin/df sh scripts/df-diff.sh   # control: should be all green
#
set -u

# --- into a private mount namespace -------------------------------------------
#
# This has to happen *before* `diff-wsl.sh` is sourced, and after we are inside
# WSL. Before, because that file creates a scratch directory and an `EXIT` trap
# to remove it, and `exec` would replace the process without running the trap,
# leaking the directory every run. After, because `unshare` is a Linux command;
# on the Windows host it does not exist, and `diff-wsl.sh` has not yet done the
# re-exec that gets us to Linux.
#
# So the sequence is three entries to this file: on the host (no `wslpath`, no
# namespace), inside WSL (`wslpath`, still no namespace — this block fires),
# and inside the namespace (`DF_DIFF_NS` set — this block is skipped).
#
# If the namespace cannot be had, the run continues without one and every case
# is masked; `DF_DIFF_NS=none` is what the case runner reads to know that.
if command -v wslpath >/dev/null 2>&1 && [ -z "${DF_DIFF_NS:-}" ]; then
    if command -v unshare >/dev/null 2>&1 && unshare -mUr true 2>/dev/null; then
        export DF_DIFF_NS=1
        exec unshare -mUr bash "$0" "$@"
    fi
    export DF_DIFF_NS=none
fi

# `mount` is not in `DIFF_NEED`: without it the harness still runs, it just
# runs the masked half. `dd` is, because `fs/a`'s Used figure is the whole
# point of that mount and an empty tmpfs would make several cases vacuous.
DIFF_PROG=df
DIFF_NEED="dd"
DIFF_FORWARD="DF_DIFF_NS"
# shellcheck source=diff-wsl.sh
. "$(dirname "$0")/diff-wsl.sh"

# `df` reads all four and would otherwise inherit whatever the operator's shell
# has set — which would change both sides, but is not what is being tested.
unset DF_BLOCK_SIZE BLOCK_SIZE BLOCKSIZE POSIXLY_CORRECT

echo "df-diff:"
echo "  ours: $OURS"
echo "  gnu:  $gnu_real"

pass=0; fail=0; xfail=0; xpass=0; skip=0

GNU_PATH="$bindir/gnu:$PATH"
OURS_PATH="$bindir/ours:$PATH"

# --- fixtures -----------------------------------------------------------------
#
# `$fs` is deliberately *not* under `$DIFF_TMP/bin`: a mount inside the
# directory holding the two symlinks would appear in every whole-table case and
# make the harness's own scaffolding part of what it measures.
fs=$DIFF_TMP/fs
have_fixtures=0
nonutf8=$(printf 'n\377ame')

if [ "${DF_DIFF_NS:-none}" = 1 ]; then
    mkdir -p "$fs/a" "$fs/b" "$fs/bind" "$fs/dup" "$fs/sp ace" "$fs/$nonutf8" \
        || exit 1
    # `-o size=` is what makes the numbers ours rather than the machine's: a
    # default tmpfs is half of RAM, which differs between hosts and would make
    # every expectation here unportable.
    mount -t tmpfs -o size=4M   tmpfs "$fs/a"        &&
    mount -t tmpfs -o size=64M,nr_inodes=1000 tmpfs "$fs/b" &&
    mount -t tmpfs -o size=8M   tmpfs "$fs/sp ace"   &&
    mount -t tmpfs -o size=8M   tmpfs "$fs/$nonutf8" &&
    # Two at one path: the first is shadowed and only the second is reachable,
    # which is the shape `df`'s duplicate filtering exists to resolve.
    mount -t tmpfs -o size=2M   tmpfs "$fs/dup"      &&
    mount -t tmpfs -o size=16M  tmpfs "$fs/dup"      &&
    mount --bind "$fs/a" "$fs/bind"                  &&
    have_fixtures=1
    if [ "$have_fixtures" = 1 ]; then
        mkdir -p "$fs/a/inner" || exit 1
        mount -t tmpfs -o size=1M tmpfs "$fs/a/inner" || have_fixtures=0
        # 100 KiB, so Used is non-zero and is not a round fraction of the size:
        # a block-size case that divided wrongly would still look plausible
        # against a figure that happened to be a power of two.
        dd if=/dev/zero of="$fs/a/file" bs=1024 count=100 status=none
        : > "$fs/b/empty"
    fi
fi

if [ "$have_fixtures" = 0 ]; then
    echo "  note: no private mount namespace, so the fixture cases are skipped"
    echo "        and only the masked whole-table and diagnostic cases run."
fi

# The tmpfs mounts above are what makes `rm -rf "$DIFF_TMP"` fail with EBUSY at
# exit -- noisily, six lines of it, on a run that otherwise succeeded. The
# namespace dies with the process and takes the mounts with it, so the mounts
# themselves are not a leak; the *directory* would be, on the `DF_DIFF_NS=none`
# path where there is no namespace to die. `-l` because a mount can still be
# busy from the harness's own working directory.
diff_cleanup() {
    if [ "${DF_DIFF_NS:-none}" = 1 ]; then
        # `/proc/self/mounts` rather than `mount`, and the octal unescaping
        # `printf` gives, because two of the fixtures are mounted at paths the
        # kernel escapes: a space (`\040`) and a `\377` byte. Splitting the
        # output of `mount` on whitespace truncates the first of those to
        # `fs/sp`, which then does not unmount and reports EBUSY anyway.
        #
        # Deepest first: `fs/a/inner` before `fs/a`, or the second unmount is
        # the one that is busy.
        LC_ALL=C awk -v p="$fs/" 'index($2, p) == 1 {print length($2), $2}' \
            /proc/self/mounts | sort -rn | cut -d' ' -f2- \
            | while IFS= read -r m; do
                  # shellcheck disable=SC2059  # the escapes are the point
                  umount -l "$(printf "$m")" 2>/dev/null
              done
    fi
    chmod -R u+rwx "$DIFF_TMP" 2>/dev/null
    rm -rf "$DIFF_TMP"
}

# --- rows the reference hides and upstream GNU does not -----------------------
#
# Ubuntu's `df` is not upstream `df`. Its `ME_DUMMY_0` -- the macro naming the
# file system types that count as "dummy", i.e. hidden unless `-a` or an
# explicit operand asks for them -- carries two types that GNU coreutils 9.4
# does not have: `devtmpfs` and `squashfs`. That is visible in the shipped
# binary's string table, where the two sit inside the same run of literals as
# `autofs`, `subfs`, `rpc_pipefs` and `kernfs`, and it is not in the 9.4 source
# nor in gnulib master. The practical effect here is that the reference omits
# the `/dev` row from every whole-table listing and ours prints it, on 37 cases.
#
# Ours follows upstream, which is the specification this transcription is
# against, so the divergence is the reference's and must not be "fixed" in the
# subject. It is removed from the comparison instead: rows for file systems of
# those types are deleted from *both* sides. Both, not just ours -- the
# reference does print them when `-a` or an operand bypasses the dummy rule, and
# deleting one-sidedly would invent a failure there.
#
# The exemption marker `=` turns this off for a case that is *about* one of
# those file systems, where deleting the row would leave nothing to compare.
#
# The full reasoning, and what to do if a second distribution patch is ever
# found, is `design-decisions.md` section 700.
hidden_mounts=$DIFF_TMP/hidden-mounts
awk '{ target = $5
       for (i = 6; i <= NF; i++)
         if ($i == "-") { type = $(i + 1); break }
       if (type == "devtmpfs" || type == "squashfs") print target }' \
    /proc/self/mountinfo >"$hidden_mounts" 2>/dev/null || : >"$hidden_mounts"

# Delete from $1 every line having a field equal to one of those mount points.
# `print` reproduces the record verbatim, so column padding survives; awk does
# append a final newline the captured output did not have, but it does so to
# both sides and the `rc=` sentinel keeps a missing newline visible either way.
# `LC_ALL=C` because one fixture's mount point holds a `\377`, and a UTF-8
# locale makes awk warn about it on stderr — a warning that would then be
# indistinguishable from a diagnostic under test.
drop_hidden() {
    [ -s "$hidden_mounts" ] || return 0
    LC_ALL=C awk 'NR == FNR { hide[$0] = 1; next }
         { for (i = 1; i <= NF; i++) if ($i in hide) next
           print }' "$hidden_mounts" "$1" >"$1.kept" && mv "$1.kept" "$1"
}

# The scratch files the two sides are captured into live on the *host* file
# system, not on any of the fixtures above. Writing them is what would otherwise
# perturb the very numbers being compared: GNU's output lands, blocks are
# consumed, and ours then reports a smaller Avail for reasons that have nothing
# to do with df. Keeping them off the fixtures is what lets the fixture cases be
# compared unmasked.
a_file=$DIFF_TMP/gnu.out
b_file=$DIFF_TMP/ours.out

# A function, not a textual substitution, for the reason `du-diff.sh` gives:
# the cases include `BLOCK_SIZE=1M df ...`, where the word to replace is not the
# first one, and a case may name a file with `df` in it. Resolving through PATH
# rather than running a path directly is what makes `argv[0]` the bare word on
# both sides — gnulib keeps the whole of `argv[0]` in every diagnostic, so
# `/usr/bin/df: ` against our `df: ` would be a difference in every error case
# that is purely an artefact of how the harness started the program.
df() { PATH=$DF_PATH command df "$@"; }

# A case line may carry any number of leading markers, in any order:
#
#   !why|cmd  the two are expected to differ, for the stated reason (xfail)
#   ~cmd      compare with every digit rewritten to `#` (see the header)
#   @cmd      needs the private mount namespace; skipped without it
#   =cmd      compare the distro-hidden rows too (see `drop_hidden`)
run_case() {
    line=$1
    expect_diff=0
    mask=0
    needs_fixtures=0
    keep_hidden=0
    reason=""
    while :; do
        case $line in
            '!'*)
                expect_diff=1
                reason=${line#!}
                reason=${reason%%|*}
                line=${line#*|}
                ;;
            '~'*) mask=1;           line=${line#\~} ;;
            '@'*) needs_fixtures=1; line=${line#@} ;;
            '='*) keep_hidden=1;    line=${line#=} ;;
            *) break ;;
        esac
    done

    if [ "$needs_fixtures" = 1 ] && [ "$have_fixtures" = 0 ]; then
        skip=$((skip + 1))
        return
    fi

    # Captured into files rather than `$(...)`: command substitution deletes
    # trailing newlines, and `df`'s alignment is trailing-space-sensitive —
    # `mbsalign` is told not to pad the last column, and a comparison that
    # could not see the end of a line would be blind to exactly that rule.
    ( DF_PATH=$GNU_PATH;  eval "$line"; printf 'rc=%s' "$?" ) >"$a_file" 2>&1
    ( DF_PATH=$OURS_PATH; eval "$line"; printf 'rc=%s' "$?" ) >"$b_file" 2>&1

    # Before the mask, not after: a hidden mount point can carry digits
    # (`/snap/core24/1587`), and masking them first would stop it matching.
    if [ "$keep_hidden" = 0 ]; then
        drop_hidden "$a_file"
        drop_hidden "$b_file"
    fi

    if [ "$mask" = 1 ]; then
        sed -i 's/[0-9]/#/g' "$a_file" "$b_file"
    fi

    if cmp -s "$a_file" "$b_file"; then
        if [ "$expect_diff" = 1 ]; then
            xpass=$((xpass + 1))
            printf 'XPASS  %s\n     (expected to differ: %s)\n' "$line" "$reason"
        else
            pass=$((pass + 1))
        fi
        return
    fi
    if [ "$expect_diff" = 1 ]; then
        xfail=$((xfail + 1))
        return
    fi
    fail=$((fail + 1))
    printf 'FAIL   %s\n' "$line"
    printf '  gnu:\n%s\n'  "$(cat -v <"$a_file" | sed -n '1,12p' | sed 's/^/        /')"
    printf '  ours:\n%s\n' "$(cat -v <"$b_file" | sed -n '1,12p' | sed 's/^/        /')"
    printf '  ---- unified ----\n'
    diff <(cat -v <"$a_file") <(cat -v <"$b_file") \
        | sed 's/^/        /' | sed -n '1,24p'
}

# `$fs` and the odd names are interpolated here rather than written into the
# heredoc, because the heredoc is quoted: an unquoted one would expand `$fs`
# but would also mangle every `$?`, `\377` and backslash in the case list.
FS=$fs
export FS
NONUTF8=$nonutf8
export NONUTF8

while IFS= read -r case_line; do
    case $case_line in ''|'#'*) continue ;; esac
    run_case "$case_line"
done <<'CASES'
# --- the whole table ---------------------------------------------------------
# Masked: these report on file systems the harness does not own, whose free
# space moves between the two invocations.
~df
~df -a
~df -l
~df -T
~df -a -T
~df -i
~df -a -i
~df -h
~df -H
~df -k
~df -m
~df -P
~df -P -a
~df --total
~df -a --total
~df -T --total
~df -i --total
~df -h --total
~df --si
~df -v
~df --no-sync

# --- selecting by type -------------------------------------------------------
~df -t tmpfs
~df -x tmpfs
~df -a -t tmpfs
~df -t tmpfs -t ext4
~df -x tmpfs -x ext4
~df -t nosuchtype
~df -x nosuchtype
~df -F tmpfs
~df -l -t tmpfs
~df --type=tmpfs --total

# --- the output option -------------------------------------------------------
#
# The whole-table forms here all carry `target`, because that is the column
# `drop_hidden` identifies a row by and without it the reference's extra
# `devtmpfs` row cannot be told from any other `none`. The field rendering of
# the target-less forms is covered by the fixture cases below, which name one
# file system and so have nothing to identify. One target-less form is kept as
# an xfail, as the harness's own record that the divergence is still there.
~df --output
!Ubuntu hides devtmpfs; with no target column the extra row cannot be dropped|~df --output=source
~df --output=target
~df --output=fstype,target
~df --output=source,target
~df --output=size,used,avail,pcent,target
~df --output=itotal,iused,iavail,ipcent,target
~df --output=file,target
~df --output=target,source
~df --output=source,target --total
~df --output=target --total
~df --output=pcent,target
~df --output=ipcent,target
~df -a --output=source,fstype,target

# --- our own file systems, compared exactly ----------------------------------
# Nothing writes to these during a case, so the numbers are identical across
# the two runs and the comparison can be byte for byte.
@df "$FS/a"
@df "$FS/b"
@df -a "$FS/a"
@df -h "$FS/a"
@df -H "$FS/a"
@df -k "$FS/a"
@df -m "$FS/a"
@df -i "$FS/a"
@df -i "$FS/b"
@df -T "$FS/a"
@df -P "$FS/a"
@df -P -T "$FS/a"
@df --total "$FS/a" "$FS/b"
@df -i --total "$FS/a" "$FS/b"
@df --output "$FS/a"
@df --output=source,size,used,avail,pcent,target "$FS/a"
@df --output=file,target "$FS/a/file"
@df --output=fstype,itotal,iused,iavail,ipcent "$FS/b"
@df "$FS/a" "$FS/b" "$FS/a"
@df -h --total "$FS/a" "$FS/b"

# --- a mount inside a mount --------------------------------------------------
# The inner one is what must be reported, and the outer one is what a lookup
# that stopped at the first prefix match would report instead.
@df "$FS/a/inner"
@df -T "$FS/a/inner"
@df --output=target,size "$FS/a/inner"
@df "$FS/a" "$FS/a/inner"

# --- a bind mount and a shadowed mount ---------------------------------------
@df "$FS/bind"
@df -T "$FS/bind"
@df --output=source,target "$FS/bind"
@df "$FS/a" "$FS/bind"
@df "$FS/dup"
@df --output=target,size "$FS/dup"

# --- names that are not plain ASCII ------------------------------------------
@df "$FS/sp ace"
@df -T "$FS/sp ace"
@df --output=target,size "$FS/sp ace"
@df "$FS/$NONUTF8"
@df -T "$FS/$NONUTF8"
@df --output=target "$FS/$NONUTF8"
@df "$FS/sp ace" "$FS/$NONUTF8"

# --- block sizes -------------------------------------------------------------
@df -B 1 "$FS/a"
@df -B 512 "$FS/a"
@df -B 1K "$FS/a"
@df -B 1KB "$FS/a"
@df -B K "$FS/a"
@df -B KB "$FS/a"
@df -B 1M "$FS/a"
@df -B 1MB "$FS/a"
@df -B M "$FS/a"
@df -B 1G "$FS/a"
@df -B G "$FS/a"
@df -B 1T "$FS/a"
@df -B 3 "$FS/a"
@df -B 1024 "$FS/a"
@df -B 2048 "$FS/a"
@df -B 1000 "$FS/a"
@df -B 1000000 "$FS/a"
@df -B 1048576 "$FS/a"
@df -B 1E "$FS/a"
@df -B Z "$FS/a"
@df -B Y "$FS/a"
@df -B R "$FS/a"
@df -B Q "$FS/a"
@df -B 1Z "$FS/a"
@df -h -B 1M "$FS/a"
@df -B 1M -h "$FS/a"
@df -B 1M --total "$FS/a" "$FS/b"
@df -B 1M -P "$FS/a"
@df -B K -P "$FS/a"

# --- the environment ---------------------------------------------------------
@DF_BLOCK_SIZE=1M df "$FS/a"
@DF_BLOCK_SIZE=K df "$FS/a"
@BLOCK_SIZE=1M df "$FS/a"
@BLOCKSIZE=1M df "$FS/a"
@DF_BLOCK_SIZE=1M BLOCK_SIZE=1K df "$FS/a"
@BLOCK_SIZE=1M BLOCKSIZE=1K df "$FS/a"
@POSIXLY_CORRECT=1 df -P "$FS/a"
@POSIXLY_CORRECT=1 df "$FS/a"
@DF_BLOCK_SIZE=nonsense df "$FS/a"
@BLOCK_SIZE=nonsense df "$FS/a"
@DF_BLOCK_SIZE=1M df -B 1K "$FS/a"
@DF_BLOCK_SIZE=1M df -h "$FS/a"
@BLOCKSIZE=nonsense df "$FS/a"
~DF_BLOCK_SIZE=1M df

# --- operands that are devices and other odd things --------------------------
~df /
~df / /
=~df /dev
~df /proc
=~df /dev/null
=~df --output=target /dev/null
~df -a /proc

# --- diagnostics -------------------------------------------------------------
df nosuchfile
df / nosuchfile
df nosuchfile nosuchfile
df --total nosuchfile
df -i nosuchfile
df --output=target nosuchfile
df ""
df / ""
df --output=nosuch
df --output=size,size
df --output=size,
df --output=,size
df --output=
df -i --output=size
df --output=size -i
df -T --output=size
df --output=size -T
df -P --output=size
df --output=size -P
df -t ext4 -x ext4
df -t tmpfs -x TMPFS
df -B 0
df -B -1
df -B ""
df -B nonsense
df -B 1B
df -B Si
df -B ki
df -B 1i
df -B 1e3
df -B 1E100
df -B 18446744073709551616
df -q
df --nosuchoption
# `--tot` is unambiguous, so this one succeeds and prints a whole table; the
# rest are rejected and print nothing but a message. (Bare `--output` is an
# abbreviation of nothing, and is covered by the output section above.)
~df --tot
df --t
df --s
df --output nosuchfile

# --- deliberate differences --------------------------------------------------
!our --help is ours to word|df --help
!our version string is ours|df --version
!strtol_fatal escapes an embedded quote; GNU interpolates it raw|df -B "1'024" /
CASES

# `--sync` is run last and on its own: it calls sync(2), which flushes the whole
# machine and can take a second, and — more to the point — it changes the free
# space of every file system on the host, which is exactly what every masked
# case above is trying not to be sensitive to.
run_case '~df --sync'
run_case '@df --sync "$FS/a"'

total=$((pass + fail + xfail + xpass + skip))
printf '%d case(s): %d passed, %d differed, %d differ on purpose, %d unexpectedly agreed, %d skipped\n' \
    "$total" "$pass" "$fail" "$xfail" "$xpass" "$skip"
[ "$fail" = 0 ] || exit 1
