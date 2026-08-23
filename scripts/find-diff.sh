#!/usr/bin/env bash
# Differential test: our find against GNU find.
#
# ## Why this one runs inside WSL
#
# For the reason `scripts/du-diff.sh` gives at length, and more so. `find`'s
# answer is a walk over real directory entries, and half its vocabulary asks
# questions only a real inode can answer: `-inum`, `-links`, `-samefile`,
# `-perm`, `-user`, `-type l`, `-xtype`, `-fstype`, `%i`, `%n`, `%b`. Windows
# has none of them, which is why `find.rs`'s `RealTree` and its `main` are both
# `#[cfg(unix)]` and the Windows build is a stub. Pointing the ordinary
# `*-diff.sh` shape at it would be comparing GNU against that stub.
#
# The fixture tree is built in WSL's own `/tmp` rather than under `/mnt/d`. On
# 9p the link count of a directory is synthesised, hard links do not share an
# inode, and `st_blocks` is invented — so `-links`, `-samefile` and `%b` would
# all be comparing two implementations against fiction.
#
# ## Why `LC_ALL=C.UTF-8` and not `C`
#
# findutils sets `err_quoting_style = locale_quoting_style`, which renders a
# name in a diagnostic as `‘name’` in a UTF-8 locale and as `'name'` in the C
# locale. Ours always writes the curly pair, because the target has one locale.
# Under plain `C` every diagnostic case would differ on the quotes alone and
# nothing else would be visible; under `C.UTF-8` the quoting agrees and the
# comparison is about what the messages *say*.
#
# `TZ=UTC` for the same class of reason: `-ls` and `-printf %t` print a local
# time, so a harness that inherited the operator's zone would compare two runs
# of the same clock and call the agreement a result.
#
# ## What is compared
#
# stdout, stderr and the exit status, byte for byte. Directory order is
# compared too, deliberately: neither side sorts — GNU hands `fts_open` a null
# comparator, we hand `read_dir` straight through — so both see one
# `getdents` order and a difference here would mean one of us had started
# sorting.
#
#     sh scripts/find-diff.sh                      # run it
#     OURS=/usr/bin/find sh scripts/find-diff.sh   # control: should be all green
#
set -u

# ------------------------------------------------------------------ outer ---
# Run from MSYS, this half only hands the whole job to WSL. `wsl` inherits the
# Windows cwd translated to `/mnt/...`, which is why the relative path below
# resolves where an absolute MSYS path would be mangled.
if [ "${FIND_DIFF_INNER:-}" != 1 ]; then
    cd "$(dirname "$0")/.." || exit 1
    if ! wsl -e true >/dev/null 2>&1; then
        echo "find-diff: WSL is not available; SKIPPED"
        exit 0
    fi
    exec wsl -e env FIND_DIFF_INNER=1 "OURS=${OURS:-}" LC_ALL=C.UTF-8 TZ=UTC \
        bash ./scripts/find-diff.sh
fi

# ------------------------------------------------------------------ inner ---
export LC_ALL=C.UTF-8 TZ=UTC
# find reads all three. They would otherwise be inherited from the operator's
# shell, which changes both sides at once — the worst kind of difference,
# because it hides rather than reports.
unset FIND_BLOCK_SIZE POSIXLY_CORRECT FIGNORE

# Resolved while `find` still means the system's: a few lines below it becomes a
# shell function, and `command -v find` would then answer "find".
GNU=${GNU:-$(command -v find)}
OURS=${OURS:-}

if [ -z "$OURS" ]; then
    export PATH="$HOME/.cargo/bin:$PATH"
    if ! command -v cargo >/dev/null 2>&1; then
        cat >&2 <<'MISSING'
find-diff: no cargo inside WSL, so our find cannot be built for Linux.

Install one (a per-user toolchain, nothing system-wide):

  wsl -e sh -c "curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs \
      | sh -s -- -y --default-toolchain stable --profile minimal"

MISSING
        echo "find-diff: cargo missing inside WSL; SKIPPED"
        exit 0
    fi
    # Built every run, from the package named, for the reason
    # `scripts/diff-subject.sh` gives: a harness that merely *runs* a path
    # measures whatever was last written there, which need not be current and
    # need not even be this crate.
    #
    # The target directory is under WSL's home rather than the repository: the
    # repository is on 9p through `/mnt/d`, where a Rust build is an order of
    # magnitude slower, and a second `target/` inside the tree would be a
    # tens-of-gigabytes surprise for whoever next runs `du -sh` on the worktree.
    root=$(cd "$(dirname "$0")/.." && pwd) || exit 1
    ( cd "$root/userspace/coreutils" \
      && CARGO_TARGET_DIR="$HOME/find-diff-target" \
         cargo build --bin find --target x86_64-unknown-linux-gnu ) >&2 || {
        echo "find-diff: the build failed"
        exit 1
    }
    OURS="$HOME/find-diff-target/x86_64-unknown-linux-gnu/debug/find"
fi

if [ ! -x "$OURS" ]; then
    echo "find-diff: $OURS is not executable"
    exit 1
fi

# Both are about to become the target of a symlink in another directory, and a
# relative `OURS=` — which the override exists to accept — would then dangle.
GNU=$(cd "$(dirname "$GNU")" && pwd)/$(basename "$GNU") || exit 1
OURS=$(cd "$(dirname "$OURS")" && pwd)/$(basename "$OURS") || exit 1

pass=0; fail=0; xfail=0; xpass=0

fixtures=$(mktemp -d /tmp/find-diff.XXXXXX) || exit 1
trap 'chmod -R u+rwX "$fixtures" 2>/dev/null; rm -rf "$fixtures"' EXIT

# Each implementation gets a directory containing one symlink named `find`, and
# a case runs with that directory first on `PATH`. The point is `argv[0]`:
# gnulib's `set_program_name` keeps the whole of it, so GNU find invoked as
# `/usr/bin/find` prefixes every diagnostic with `/usr/bin/find: ` while ours
# says `find: `. Comparing those would report a difference in every diagnostic
# case that is an artefact of how the harness started the program. A `PATH`
# lookup makes `argv[0]` the word `find` on both sides, which is also what the
# shell will do on the target.
#
# They live under `$fixtures` but *outside* the walked tree: a `bin` directory
# inside `t` would appear in every listing.
mkdir -p "$fixtures/bin-gnu" "$fixtures/bin-ours" || exit 1
ln -s "$GNU" "$fixtures/bin-gnu/find" || exit 1
ln -s "$OURS" "$fixtures/bin-ours/find" || exit 1
# `-execdir` refuses to run at all if `$PATH` holds a relative entry, and an
# empty entry is one. `$PATH` inside WSL routinely ends in `:` because Windows'
# own PATH is appended; keeping that would make every `-execdir` case a refusal
# on both sides, which agrees but tests nothing.
CLEAN_PATH=$(printf '%s' "$PATH" | tr ':' '\n' | grep -v '^$' | grep '^/' \
             | paste -sd: -)
GNU_PATH="$fixtures/bin-gnu:$CLEAN_PATH"
OURS_PATH="$fixtures/bin-ours:$CLEAN_PATH"

mkdir -p "$fixtures/tree" || exit 1
cd "$fixtures/tree" || exit 1

# ---------------------------------------------------------------- fixtures ---
#
# Each of these exists to make one rule observable:
#
#   t/f, t/sub/h        an ordinary file at two depths, so -maxdepth, -depth
#                       and -prune have something to disagree about
#   t/sub/deep/i        a third level, which is what tells -mindepth 2 from
#                       -mindepth 1
#   t/hard              a second name for t/f's inode: -links, -samefile, %n
#   t/big               larger than one block, so -size is a division
#   t/sparse            apparent size 1 MiB, no blocks: the one file where
#                       -size -1 and -size -2048c cannot agree
#   t/dangle            a symlink to nothing — the case that separates -type l
#                       from -xtype l, and -P from -L
#   t/link              a symlink to a file
#   t/dlink             a symlink to a *directory*, which is what -L descends
#   t/sub/up            a symlink that reaches back above itself, so -L has a
#                       loop to detect
#   t/.hidden           a leading dot, which fnmatch without FNM_PERIOD matches
#   t/sp ace, t/n?ame   a space and a non-UTF-8 byte, the names the old find
#                       could not be handed at all
#   t/exe, t/noread     the two -perm/-readable/-executable answers
#   t/setuid, t/sticky  the bits -perm /4000 and -perm -1000 are about
#   t/fifo              a -type the walk cannot open
#   t/empty             an empty directory, for -empty
#   t/emptyf            an empty file, which -empty also matches
mkdir -p t/sub/deep t/empty t/sticky
: > t/f
printf 'ggg' > t/g
printf 'hhhhh' > t/sub/h
dd if=/dev/zero of=t/sub/deep/i bs=1024 count=4 status=none
dd if=/dev/zero of=t/big bs=1024 count=2000 status=none
truncate -s 1M t/sparse
ln t/f t/hard
ln -s nowhere t/dangle
ln -s f t/link
ln -s sub t/dlink
ln -s .. t/sub/up
: > t/.hidden
: > "t/sp ace"
: > "$(printf 't/n\377ame')"
: > t/emptyf
printf '#!/bin/sh\nexit 0\n' > t/exe && chmod 755 t/exe
: > t/noread && chmod 000 t/noread
: > t/setuid && chmod 4644 t/setuid
chmod 1777 t/sticky
mkfifo t/fifo

# Distinct, fixed times, so `-newer`, `-mtime`, `-newermt` and `%T@` mean the
# same thing in June as in December. Set after the tree is built because
# creating a file inside a directory bumps that directory's mtime.
touch -d '2020-01-02 03:04:05 UTC' t/f t/hard
touch -d '2021-06-07 08:09:10 UTC' t/g
touch -d '2022-11-12 13:14:15 UTC' t/sub/h
touch -d '2023-04-05 06:07:08 UTC' t/big

# The `-files0-from` lists, and the `-fprint` targets, live beside the tree
# rather than in it: a file the walk can see would change every listing, and a
# `-fprint` target inside the tree would change the listing it is recording.
printf 't/f\0t/sub\0' > ../list0
printf 't/f\0\0t/sub\0' > ../list0-empty
printf 't/f\0t/sub' > ../list0-no-nul

# ------------------------------------------------------------------- cases ---
#
# One shell command line per case, `find` standing for whichever find is
# running. Blank lines and `#` lines are ignored; a line beginning `!` is a case
# expected to differ, and the text between `!` and `|` says why.
#
# `find` is a *function*, not a textual substitution: rewriting the word would
# misfire the first time a case names a file with `find` in it, and it could not
# express the `FIND_BLOCK_SIZE=1M find t` cases at all, where the thing to
# replace is not the first word.
find() { PATH=$FIND_PATH command find "$@"; }

# Run one case and return everything observable about it: stdout, stderr and
# the exit status.
#
# The `tr` is not cosmetic. A command substitution discards NUL bytes, so
# without it `-print0` and `-print` would capture identically and every case
# whose whole point is the terminator would agree by construction. `\001` is
# not a byte either implementation ever writes, and unlike `cat -v` it leaves
# the UTF-8 in the diagnostics alone, so a failure still prints readably.
capture() {
    FIND_PATH=$1
    { eval "$2" 2>&1; printf 'rc=%s' "$?"; } | tr '\0' '\001'
}

# Render a captured string for a human: NUL back to something visible, and the
# rest of the control bytes with it.
show() { printf '%s\n' "$1" | tr '\001' '\0' | cat -v; }

run_case() {
    line=$1
    expect_diff=0
    reason=""
    case $line in
        '!'*)
            expect_diff=1
            reason=${line#!}
            reason=${reason%%|*}
            line=${line#*|}
            ;;
    esac

    a=$(capture "$GNU_PATH" "$line")
    b=$(capture "$OURS_PATH" "$line")

    if [ "$a" = "$b" ]; then
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
    printf '  ---- gnu | ours ----\n'
    diff <(show "$a") <(show "$b") | sed 's/^/        /' | sed -n '1,24p'
}

while IFS= read -r case_line; do
    case $case_line in ''|'#'*) continue ;; esac
    run_case "$case_line"
done <<'CASES'
# --- the walk itself ---
find t
find t/
find t//
find t t
find .
find ./t
find t t/sub
find t/f
find t/sub/deep
find t/empty
find "t/sp ace"
find t/fifo
find
find -maxdepth 1
find t -maxdepth 0
find t -maxdepth 1
find t -maxdepth 2
find t -mindepth 1
find t -mindepth 2
find t -mindepth 1 -maxdepth 1
find t -mindepth 2 -maxdepth 2
find t -depth
find t -depth -maxdepth 1
find t -d
find t -depth -name sub
find t -name sub -prune
find t -name sub -prune -o -print
find t -path 't/sub*' -prune -o -print
find t -prune
find t -depth -prune
find t -quit
find t -name h -quit
find t -print -quit
find t -mount
find t -xdev
find t -noleaf
find t -ignore_readdir_race
find t -noignore_readdir_race

# --- the link-following options ---
find -P t
find -L t
find -H t
find -L t/dlink
find -P t/dlink
find -H t/dlink
find -L t/dangle
find -H t/dangle
find -P t/dangle
find -L t/link
find -L t -maxdepth 2
find -L t/sub
find -H t -maxdepth 1
find -follow t
find t -follow
find -L t -name up

# --- -name and friends ---
find t -name f
find t -name '*'
find t -name '*h'
find t -name '[a-z]'
find t -name '[!fg]*'
find t -name '.*'
find t -name 'sp ace'
find t -name 'sp*'
find t -name 'nosuch'
find t -name ''
find t -name 't'
find t -name '\*'
find t -name 'f?'
find t -iname 'F'
find t -iname '*H'
find t -path 't/sub'
find t -path 't/sub/*'
find t -path '*sub*'
find t -path 't'
find t -ipath 'T/SUB'
find t -wholename 't/sub'
find t -lname 'nowhere'
find t -lname 'f'
find t -lname '*'
find t -ilname 'NOWHERE'
find t -regex '.*/f'
find t -regex 't/f'
find t -regex 'f'
# The fixture tree contains `t/n\377ame`, whose name is not valid UTF-8, and
# this is the one case in the file where `.*` has to decide what a "character"
# is. GNU compiles the pattern with glibc's multibyte matcher, which cannot
# decode \377 in a UTF-8 locale and so declines to match the name at all; ours
# is byte-based and matches it. Ours is the deliberate answer, not an
# oversight: a path on this system is a byte string with no encoding attached
# (design-decisions.md §322), and a `find -regex '.*'` that silently skips
# files is worse than one that matches every byte string, which is what `.*`
# reads as. Every other `-regex` case in this file agrees exactly, because
# every other one is over a name that decodes.
!our regex engine is byte-based, GNU's is multibyte, and one fixture name is not UTF-8|find t -regex '.*'
find t -iregex '.*/F'
find t -regex 't/su.'
find t -regextype posix-basic -regex 't/su.'
find t -regextype posix-extended -regex 't/(f|g)'
find t -regextype posix-egrep -regex 't/(f|g)'
find t -regextype emacs -regex 't/su.'
find t -regextype ed -regex 't/su.'
find t -regextype sed -regex 't/su.'
find t -regextype awk -regex 't/(f|g)'
find t -regextype posix-awk -regex 't/(f|g)'
find t -regextype grep -regex 't/su.'

# --- -type / -xtype ---
find t -type f
find t -type d
find t -type l
find t -type p
find t -type b
find t -type c
find t -type s
find t -type f,d
find t -type l,p
find t -xtype f
find t -xtype d
find t -xtype l
find -L t -type l
find -L t -xtype l
find -H t -xtype l
find t/dangle -xtype l
find t/dangle -xtype f
find t/link -xtype f
find t/link -type l

# --- -size ---
find t -size 0
find t -size 1
find t -size -1
find t -size +1
find t -size 0c
find t -size 3c
find t -size -3c
find t -size +3c
find t -size 1k
find t -size -1k
find t -size +1k
find t -size 1M
find t -size +1M
find t -size 2M
find t -size -2048c
find t -size 4096c
find t -size 8b
find t -size +0b
find t -size 1w
find t -size +2G
find t -size -1T
find t -empty
find t -type f -empty
find t -type d -empty

# --- -perm ---
find t -perm 644
find t -perm 755
find t -perm 0644
find t -perm -644
find t -perm -444
find t -perm /444
find t -perm /111
find t -perm -111
find t -perm +111
find t -perm /4000
find t -perm -4000
find t -perm 4644
find t -perm 1777
find t -perm -1000
find t -perm 000
find t -perm -0
find t -perm /0
find t -perm u+w
find t -perm -u+w
find t -perm /u+x
find t -perm g=r
find t -perm -g=r
find t -perm a=rwx
find t -type f -perm -o=r

# --- inodes, links, owners ---
find t -links 1
find t -links 2
find t -links +1
find t -links -2
find t -samefile t/f
find t -samefile t/hard
find t -samefile t/link
find -L t -samefile t/link
find t -user "$(id -un)"
find t -user "$(id -u)"
find t -group "$(id -gn)"
find t -group "$(id -g)"
find t -uid "$(id -u)"
find t -gid "$(id -g)"
find t -nouser
find t -nogroup
find t -readable
find t -writable
find t -executable
find t -type f -readable
find t -type f ! -readable
find t -fstype ext4
find t -fstype tmpfs
find t -fstype nosuchfs
find t -inum 0

# --- times ---
find t -newer t/f
find t -newer t/big
find t -anewer t/big
find t -cnewer t/big
find t -newermt '2021-01-01'
find t -newermt '2030-01-01'
find t -newermt '2000-01-01'
find t ! -newermt '2021-01-01'
find t -newerat '2021-01-01'
find t -newerct '2021-01-01'
find t -newermm t/g
find t -mtime +1000
find t -mtime -1000
find t -mtime 0
find t -mtime +100000
find t -mmin +1000
find t -mmin -1
find t -atime +1000
find t -ctime -1000
find t -daystart -mtime +1000
find t -daystart -mtime 0
find t -used 0
find t -used +1

# --- the operators ---
find t -name f -o -name g
find t -name f -a -name g
find t -name f -and -type f
find t -name f -or -name g
find t ! -name f
find t -not -name f
find t '(' -name f ')'
find t '(' -name f -o -name g ')' -type f
find t -name f -o -name g -print
find t '(' -name f -o -name g ')' -print
find t -name f , -name g
find t -name f , -name g -print
find t -true
find t -false
find t -true -o -print
find t -false -o -print
find t -false -a -print
find t ! ! -name f
find t ! '(' -name f -o -name g ')'
find t -name f -o -true
find t -type d -a '(' -name sub -o -name empty ')'

# --- the actions ---
find t -print
find t -print -print
find t -name f -print
find t -print0
find t -name f -print0
find t -ls
find t -name f -ls
find t -type d -ls
find t -ls -print
find t -name f -fls /dev/stdout
find t -name f -fprint /dev/stdout
find t -name f -fprint0 /dev/stdout
find t -name f -fprint ../out ; cat ../out
find t -name f -fprint ../out -fprint ../out ; cat ../out
find t -type d -fprint ../out -o -fprint ../out2 ; cat ../out ../out2
find t -name f -printf '%p\n'
find t -maxdepth 1 -printf '%p|%f|%h|%H|%P|%d|%y\n'
find t -maxdepth 1 -printf '%s %b %k\n'
find t -maxdepth 1 -printf '%m %M\n'
find t -maxdepth 1 -printf '%n %i\n'
find t -maxdepth 1 -printf '%u %g %U %G\n'
find t -maxdepth 1 -printf '%F\n'
find t -maxdepth 1 -printf '%l\n'
find t -maxdepth 1 -printf '%Y\n'
find t -name f -printf '%T@ %TY-%Tm-%Td %TH:%TM:%TS\n'
find t -name f -printf '%Tj %TU %Tw %TZ\n'
find t -name f -printf '%t\n'
find t -name f -printf '%TF %TT %TD %Tr\n'
find t -name f -printf '%Ts %TX %Tx %Tc\n'
find t -name f -printf '%A@ %C@\n'
find t -name f -printf 'a\tb\nc\\d\n'
find t -name f -printf '%%\n'
find t -name f -printf '%p'
find t -name f -printf 'x'
find t -name f -printf '\a\b\f\r\v\0end\n'
find t -name f -printf '%10p|%-10p|%.2p|\n'
find t -name f -printf '%010s|%-6s|\n'
find t -name f -printf '%h %H\n'
find ./t -name f -printf '%h|%H|%P\n'
find t/ -name f -printf '%h|%H|%P\n'
find t -maxdepth 1 -printf '[%p]\n' -o -printf 'no\n'
find t -name f -printf 'end\c' ; echo
find t -name f -fprintf /dev/stdout '%p\n'
find t -name f -fprintf ../out '%p\n' ; cat ../out

# --- -exec and friends ---
find t -name f -exec echo A '{}' ';'
find t -name f -exec echo A '{}' +
find t -maxdepth 1 -name '*' -exec echo '{}' +
find t -name f -exec echo '{}' '{}' ';'
find t -name f -exec echo 'x{}y' ';'
find t -name f -exec true ';' -print
find t -name f -exec false ';' -print
find t -name f -exec false ';' -o -print
find t -name f -execdir echo A '{}' ';'
find t -name h -execdir echo A '{}' ';'
find t -name f -execdir echo A '{}' +
find t -name f -exec nosuchprogram ';'
find t -name f -exec nosuchprogram +
find t -maxdepth 1 -exec echo '{}' +
find t -name f -exec sh -c 'echo $0' '{}' ';'
find t -exec test -d '{}' ';' -print
find t -name f -ok echo A '{}' ';' < /dev/null
find t -name f -okdir echo A '{}' ';' < /dev/null

# --- -printf %Z and SELinux ---
!we render %Z empty; GNU asks the kernel and has no policy loaded|find t -name f -printf '%Z\n'
# Agrees, but only because the reference build has SELinux compiled out and so
# refuses `-context` with the same sentence we do. Against an SELinux-enabled
# GNU this case would legitimately differ — that it is a plain case here is a
# statement about the reference build, not a promise about every build.
find t -context '*'

# --- the leading options ---
find -D help t
find -O0 t -name f
find -O1 t -name f
find -O2 t -name f
find -O3 t -name f
find -O t -name f
find -- t -name f
find t -- -name f
find -P -L -H t -maxdepth 0
find -L -P t/dangle
find t -files0-from ../list0
find -files0-from ../list0 t
find t -warn -name f
find t -nowarn -name f
find t -maxdepth 1 -warn
FIND_BLOCK_SIZE=1024 find t -name f -printf '%k\n'
POSIXLY_CORRECT=1 find t -name f -printf '%k\n'
POSIXLY_CORRECT=1 find t -size 1

# --- -files0-from ---
find -files0-from ../list0
find -files0-from ../list0 -print
find -files0-from ../list0-empty
find -files0-from ../list0-no-nul
find -files0-from ../nosuchlist
find -files0-from ''
find -files0-from ../list0 -name f
find -files0-from - < ../list0
find -files0-from ../list0 -files0-from ../list0

# --- diagnostics ---
find nosuchfile
find t nosuchfile
find nosuchfile t
find ''
find t ''
find t -zzz
find t -name
find t -type
find t -type q
find t -type ff
find t -type f,
find t -type ,f
find t -maxdepth
find t -maxdepth x
find t -maxdepth -1
find t -mindepth -1
find t -maxdepth 1 -maxdepth 2
find t -size
find t -size x
find t -size 1x
find t -size ++1
find t -size -
find t -perm
find t -perm zzz
find t -perm 8
find t -perm +rw
find t -user nosuchuser
find t -group nosuchgroup
find t -uid x
find t -gid x
find t -newer nosuchfile
find t -newermt zzz
find t -newerxy t/f
find t '('
find t '(' -name f
find t ')'
find t '(' ')'
find t -name f -o
find t -o -name f
find t '!'
find t -a
find t -a -name f
find t -name f -a
find t -exec
find t -exec echo
find t -exec echo '{}'
find t -exec echo ';' extra
find t -execdir
find t -ok
find t -print extra
find t -name f extra
find t extra -name f
find t -regextype nosuchtype -regex .
find t -regex

# --- malformed patterns, whose *wording* comes from glibc ---
#
# findutils compiles through GNU regex and prints back whatever
# `re_compile_pattern` returned, so these cases pin our `ere` crate's error
# classification against glibc's `re_error_msgid` table rather than against
# anything findutils itself decides. They are also the only cases here that
# check `ere` *accepts* what glibc accepts: a pattern GNU compiles silently and
# we reject shows up as a diagnostic on one side and none on the other.
find t -regex '['
find t -regex '[a'
find t -regextype posix-extended -regex '['
find t -regextype posix-extended -regex '[a'
find t -regextype posix-extended -regex '*'
find t -regextype posix-extended -regex 'a**'
find t -regextype posix-extended -regex '('
find t -regextype posix-extended -regex ')'
find t -regextype posix-extended -regex 'a{2'
find t -regextype posix-extended -regex 'a{,}'
find t -regextype posix-extended -regex 'a{1,0}'
find t -regextype posix-extended -regex '[[:foo:]]'
find t -regextype posix-extended -regex '[z-a]'
find t -regextype posix-extended -regex 'a\'
find t -regextype posix-extended -regex ''
find t -regextype posix-basic -regex 'a\('
find t -regextype posix-basic -regex 'a\)'
find t -regextype posix-basic -regex 'a\{'
find t -regextype posix-basic -regex 'a\{2'
find t -regextype posix-basic -regex '[a'
find t -regextype posix-basic -regex 'a\'
find t -printf
find t -printf '%'
find t -printf '%q\n'
find t -printf '%T\n'
find t -printf '%TQ\n'
find t -printf '\q\n'
find t -printf '%A\n'
find t -fprint
find t -fprint /nosuchdir/out
find t -fls /nosuchdir/out
find t -fprintf /nosuchdir/out '%p\n'
find t -fprintf ../out
find t -fstype
find t -inum x
find t -links x
find t -mtime x
find t -mtime +
find t -used x
find t -files0-from
find t -D
find t -O9
find t -Ox
find --nosuchoption t
find -name f -type

# --- diagnostics that name an argument which is not text ---
#
# Every one of these makes find quote an argv token back at the user, and every
# token here contains a byte that decodes to no character in a UTF-8 locale.
# They exist to pin the *lossless* half of that rendering: whatever the two
# sides print, ours must let the reader work out which byte was passed, which a
# U+FFFD does not. See design-decisions.md §369 for why we escape where upstream
# writes the byte raw, and why that is not the same question as the deliberate
# differences at the foot of this file.
#
# The five marked cases are the complete set of find diagnostics that quote a
# single undecodable byte: GNU writes the raw byte (`M-^?` through od -c), we
# write `\377`. Both name the same byte and both are recoverable; the marking
# records that the difference is the escape and nothing else. If find's
# String-typed Fatal/errmsg plumbing is ever converted to bytes, these five
# should flip back to plain cases and be expected to pass. `-perm` is *not*
# marked, and must keep passing: its diagnostic does not echo the argument, so
# it is the control that shows the marking above is about rendering and not
# about the parse.
!we escape an undecodable argument byte as \377, GNU writes it raw (design-decisions.md §369)|find t -type $'\377'
find t -perm $'\377'
!we escape an undecodable argument byte as \377, GNU writes it raw (design-decisions.md §369)|find t -used $'\377'
!we escape an undecodable argument byte as \377, GNU writes it raw (design-decisions.md §369)|find t -size $'\377'
!we escape an undecodable format byte as \377, GNU writes it raw (design-decisions.md §369)|find t -printf $'%\377\n'
!we escape an undecodable predicate byte as \377, GNU writes it raw (design-decisions.md §369)|find t $'-\377'

# --- deliberate differences ---
# The only two messages in the whole interface that are *about the program
# rather than about the files*, and so the only two that must not imitate
# upstream: a binary that answers "which program are you" with GNU's banner,
# or sends its bug reports to GNU's tracker, is lying about its identity to
# the one question asked to establish it. See the `VERSION` doc comment.
!our version string is ours, deliberately|find --version
!our help sends bug reports to us, not to GNU|find --help
# GNU reaches `assert (! isnan (...))` in `get_comp_type` and aborts with a
# core dump; we refuse the argument. The case is unusable as a straight
# comparison anyway — the shell prints the crashed child's *pid* in its job
# message, so the two sides could not agree even against themselves.
!GNU asserts on a NaN interval; we diagnose it|find t -mtime nan
CASES

# ------------------------------------------------------- the mutating cases ---
#
# `-delete` cannot be run twice over one tree: the second side would walk what
# the first left behind and the harness would report the deletion as a
# difference. Each of these gets a scratch tree rebuilt immediately before each
# side runs, so both see the same starting state.
reset_mut() {
    chmod -R u+rwX ../mut 2>/dev/null
    rm -rf ../mut
    mkdir -p ../mut/sub/deep ../mut/empty
    : > ../mut/f
    printf 'ggg' > ../mut/g
    : > ../mut/sub/h
    ln -s nowhere ../mut/dangle
    ln -s f ../mut/link
    touch -d '2020-01-02 03:04:05 UTC' ../mut/f
}

run_mut_case() {
    line=$1
    reset_mut
    a=$(capture "$GNU_PATH" "$line"; printf '\n--- left behind ---\n'
        LC_ALL=C ls -a1R ../mut 2>&1)
    reset_mut
    b=$(capture "$OURS_PATH" "$line"; printf '\n--- left behind ---\n'
        LC_ALL=C ls -a1R ../mut 2>&1)
    if [ "$a" = "$b" ]; then
        pass=$((pass + 1))
        return
    fi
    fail=$((fail + 1))
    printf 'FAIL   %s\n' "$line"
    printf '  ---- gnu | ours ----\n'
    diff <(show "$a") <(show "$b") | sed 's/^/        /' | sed -n '1,24p'
}

while IFS= read -r case_line; do
    case $case_line in ''|'#'*) continue ;; esac
    run_mut_case "$case_line"
done <<'MUTCASES'
find ../mut -name f -delete
find ../mut -name nosuch -delete
find ../mut -type f -delete
find ../mut -name empty -delete
find ../mut -name sub -delete
find ../mut -delete
find ../mut -name dangle -delete
find ../mut -name link -delete
find ../mut -delete -name f
find ../mut -name f -delete -print
find ../mut -depth -name f -delete
find ../mut -name f -exec rm '{}' ';' -print
MUTCASES
rm -rf ../mut

# --------------------------------------------------- the unreadable cases ---
#
# Only meaningful as a non-root user: root reads the directory regardless, so
# both sides would silently agree on the wrong thing.
if [ "$(id -u)" != 0 ]; then
    mkdir -p t/noperm/inner
    chmod 000 t/noperm
    # Readable but not searchable is the case that makes `FTS_NOSTAT`
    # observable: the names can be listed, but nothing about them can be
    # stated. `find t/nosearch` prints every name; `-printf '%s'` reports
    # `Permission denied` for each.
    mkdir -p t/nosearch
    : > t/nosearch/a
    : > t/nosearch/b
    chmod 600 t/nosearch
    for c in "find t" "find t -type f" "find t/noperm" "find t/noperm -name x" \
             "find t -name noperm -prune" "find t/nosearch" \
             "find t/nosearch -type f" "find t/nosearch -printf '%s %p\n'" \
             "find t/nosearch -name a" "find t/nosearch -empty" \
             "find -L t/noperm" "find t -readable" "find t -ignore_readdir_race"; do
        run_case "$c"
    done
    chmod 755 t/noperm t/nosearch
else
    printf 'note: running as root, so the unreadable-directory cases were skipped\n'
fi

total=$((pass + fail + xfail + xpass))
printf '%d case(s): %d passed, %d differed, %d differ on purpose, %d unexpectedly agreed\n' \
    "$total" "$pass" "$fail" "$xfail" "$xpass"
[ "$fail" = 0 ] || exit 1
