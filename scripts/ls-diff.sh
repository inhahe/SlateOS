#!/bin/sh
# Compare our `ls` against GNU's, case for case, inside WSL.
#
# `ls` is the second utility that cannot be measured from the development host,
# and for the same reason `du` could not (see `du-diff.sh`, and
# `design-decisions.md` §365): almost everything `ls -l` prints comes from
# fields Windows does not have. A mode string is twelve permission bits; the
# link count, owner, group, inode number and block count are `struct stat`
# fields with no Windows equivalent; and the "is this a fifo / socket / device"
# question that `-F` and the type column answer cannot even be asked. So this
# harness re-execs itself into WSL, builds our `ls` for
# `x86_64-unknown-linux-gnu`, and runs both against the same fixture tree on
# WSL's own ext4.
#
# Everything else about the shape of this script is `du-diff.sh`'s, deliberately
# so -- both sides reached through a `PATH` lookup of the bare word so that
# `argv[0]` matches, the subject built every run from the package that owns it,
# a skip rather than a failure when WSL or cargo is missing, and a
# `!reason|command` line for a difference we intend.
#
# Three things are specific to `ls` and worth stating.
#
# **The reference is built, not found.** Every other harness here measures
# against whatever GNU binary WSL ships. This one cannot: WSL ships coreutils
# 9.4, and 9.4 and 9.5 lay out columns differently for any name holding a byte
# they cannot display. So the block below builds 9.5 from source into a cache
# under `$HOME` and refuses to run against any other version. See
# `design-decisions.md` §366.
#
# **Neither side has a terminal.** A command substitution is a pipe, so both
# programs take their non-tty defaults: one entry per line, literal quoting, no
# colour. That is the right default to test -- it is what every script that
# parses `ls` sees -- but it means the column layout, the quoting styles and
# the colouring have to be reached explicitly, by `-C -w N`, by
# `--quoting-style=`, and by `--color=always`. They are, below.
#
# **The fixture is built once and both sides see the same inodes**, so inode
# numbers, link counts, sizes, owners and timestamps are identical between the
# two runs by construction and can be compared literally rather than masked.
# The one field that is not stable is the *time*, and it is stable enough: the
# fixture is created in a single burst, so both runs render the same mtimes.
set -u

if [ "${LS_DIFF_INNER:-}" != 1 ]; then
    cd "$(dirname "$0")/.." || exit 1
    if ! wsl -e true 2>/dev/null; then
        echo "ls-diff: no WSL on this machine; SKIPPED"
        exit 0
    fi
    exec wsl -e env LS_DIFF_INNER=1 "OURS=${OURS:-}" LC_ALL=C.UTF-8 bash ./scripts/ls-diff.sh
fi

# `ls` reads more of the environment than most utilities, and every one of
# these would change both sides at once -- which is worse than changing one,
# because it hides a difference rather than inventing one.
unset COLUMNS TABSIZE QUOTING_STYLE TIME_STYLE LS_COLORS LS_BLOCK_SIZE BLOCK_SIZE BLOCKSIZE POSIXLY_CORRECT
export TZ=America/New_York

# ------------------------------------------------------- the reference ls ---
#
# GNU coreutils **9.5**, built from source -- deliberately *not* the `ls` on
# this machine's PATH, which under WSL is 9.4.
#
# The two are not interchangeable. 9.5 changed the flags it passes to gnulib's
# `mbsnwidth`, so the two versions measure the width of any name holding a
# control character or an invalid byte differently, and then lay out every
# column format differently as a result. This port was written from the 9.5
# source, so measuring it against a 9.4 binary produces two correct-looking
# answers that contradict each other -- which is what happened, across two
# commits, before it was noticed. `design-decisions.md` §366 and §367 have the
# detail.
#
# So: build 9.5 once, cache it under $HOME, and **skip rather than fall back**
# if it cannot be built. A silent fallback to the system `ls` is the one
# outcome worse than not running at all.
LS_DIFF_VERSION=9.5
LS_DIFF_CACHE=${LS_DIFF_CACHE:-$HOME/.cache/slateos-ls-diff}
GNU=${GNU:-}

if [ -z "$GNU" ]; then
    src=$LS_DIFF_CACHE/coreutils-$LS_DIFF_VERSION
    if [ -x "$src/src/ls" ]; then
        GNU=$src/src/ls
    else
        tarball=coreutils-$LS_DIFF_VERSION.tar.xz
        mkdir -p "$LS_DIFF_CACHE" || exit 1
        if [ ! -f "$LS_DIFF_CACHE/$tarball" ]; then
            url=https://ftp.gnu.org/gnu/coreutils/$tarball
            if command -v curl >/dev/null 2>&1; then
                curl -fsSL -o "$LS_DIFF_CACHE/$tarball.part" "$url" \
                    && mv "$LS_DIFF_CACHE/$tarball.part" "$LS_DIFF_CACHE/$tarball"
            elif command -v wget >/dev/null 2>&1; then
                wget -q -O "$LS_DIFF_CACHE/$tarball.part" "$url" \
                    && mv "$LS_DIFF_CACHE/$tarball.part" "$LS_DIFF_CACHE/$tarball"
            fi
        fi
        if [ ! -f "$LS_DIFF_CACHE/$tarball" ]; then
            echo "ls-diff: could not fetch $tarball; SKIPPED"
            echo "  (put it in $LS_DIFF_CACHE/, or set GNU=/path/to/a/9.5/ls)"
            exit 0
        fi
        ( cd "$LS_DIFF_CACHE" \
          && tar xf "$tarball" \
          && cd "coreutils-$LS_DIFF_VERSION" \
          && ./configure --quiet --disable-nls \
          && make -s -j"$(nproc 2>/dev/null || echo 4)" src/ls ) >&2 || {
            echo "ls-diff: coreutils $LS_DIFF_VERSION did not build; SKIPPED"
            echo "  (a C compiler and make are needed; or set GNU=/path/to/a/9.5/ls)"
            exit 0
        }
        GNU=$src/src/ls
    fi
fi

# A reference of the wrong version is worse than none: it fails cases that are
# right and passes cases that are wrong, in the same run.
gnu_version=$("$GNU" --version 2>/dev/null | head -1)
case $gnu_version in
    *" $LS_DIFF_VERSION")  ;;
    *)
        echo "ls-diff: reference is '$gnu_version', not coreutils $LS_DIFF_VERSION"
        echo "  (see design-decisions.md §366 for why the version has to match)"
        exit 1
        ;;
esac

OURS=${OURS:-}

if [ -z "$OURS" ]; then
    export PATH="$HOME/.cargo/bin:$PATH"
    if ! command -v cargo >/dev/null 2>&1; then
        cat >&2 <<'MISSING'
ls-diff: no cargo inside WSL, so our ls cannot be built for Linux.

Install one (a per-user toolchain, nothing system-wide):

  wsl -e sh -c "curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs \
      | sh -s -- -y --default-toolchain stable --profile minimal"

MISSING
        echo "ls-diff: cargo missing inside WSL; SKIPPED"
        exit 0
    fi
    root=$(cd "$(dirname "$0")/.." && pwd) || exit 1
    ( cd "$root/userspace/coreutils" \
      && CARGO_TARGET_DIR="$HOME/du-diff-target" \
         cargo build --bin ls --target x86_64-unknown-linux-gnu ) >&2 || {
        echo "ls-diff: the build failed"
        exit 1
    }
    OURS="$HOME/du-diff-target/x86_64-unknown-linux-gnu/debug/ls"
fi

if [ ! -x "$OURS" ]; then
    echo "ls-diff: $OURS is not executable"
    exit 1
fi

GNU=$(cd "$(dirname "$GNU")" && pwd)/$(basename "$GNU") || exit 1
OURS=$(cd "$(dirname "$OURS")" && pwd)/$(basename "$OURS") || exit 1

pass=0; fail=0; xfail=0; xpass=0

fixtures=$(mktemp -d /tmp/ls-diff.XXXXXX) || exit 1
trap 'chmod -R u+rwX "$fixtures" 2>/dev/null; rm -rf "$fixtures"' EXIT

mkdir -p "$fixtures/bin-gnu" "$fixtures/bin-ours" || exit 1
ln -s "$GNU" "$fixtures/bin-gnu/ls" || exit 1
ln -s "$OURS" "$fixtures/bin-ours/ls" || exit 1
GNU_PATH="$fixtures/bin-gnu:$PATH"
OURS_PATH="$fixtures/bin-ours:$PATH"

# Both sides' output, kept beside the fixture tree rather than inside it -- a
# case listing `.` must not find the harness's own scratch files.
out_a="$fixtures/out-gnu"
out_b="$fixtures/out-ours"

mkdir -p "$fixtures/tree" || exit 1
cd "$fixtures/tree" || exit 1

# ---------------------------------------------------------------- fixtures ---
#
# Each entry makes one column, one indicator or one sort key observable:
#
#   t/a          setuid, so the mode string's `s` overload is exercised and
#                `-F` marks it executable
#   t/hard       a second link to t/a, so the link count is not 1
#   t/bb         four bytes, so the size column is not all zeroes
#   t/dir        setgid directory -> `drwxr-s---`
#   t/sub        sticky directory -> `drwxrwxrwt`
#   t/fifo       a named pipe: type letter `p`, indicator `|`
#   t/sock       a socket: type letter `s`, indicator `=`
#   t/link       a symlink that resolves, so `-l` prints `-> a` and `-L`
#                reports the target's own stat
#   t/dangle     a symlink that does not, which `-L` cannot stat at all
#   t/.hidden    a leading dot, for `-a` and `-A`
#   t/'sp ace'   a space, which decides quoting in every style
#   t/"qu'ote"   an apostrophe, which is the case shell quoting switches from
#                `'...'` to `"..."` for
#   t/tab<TAB>here, t/n<0xFF>ame
#                a control character and a byte that is not valid UTF-8 --
#                the two cases a `char`-based renderer cannot even be handed
#   t/z.txt, t/y.tar.gz, t/x
#                extensions, for `-X`
#   t/f10 f9 f2  digits, for `-v` against plain byte order
#   t/big        1 MiB, so `-h`, `--block-size` and the `total` line have
#                something to divide
mkdir -p t/sub t/dir
: > t/a
chmod 4755 t/a
ln t/a t/hard
printf 'xxxx' > t/bb
chmod 2750 t/dir
chmod 1777 t/sub
mkfifo t/fifo
python3 -c 'import socket,sys; s=socket.socket(socket.AF_UNIX); s.bind(sys.argv[1])' t/sock 2>/dev/null || :
ln -s a t/link
ln -s nowhere t/dangle
: > t/.hidden
: > "t/sp ace"
: > "t/qu'ote"
: > "$(printf 't/tab\there')"
: > "$(printf 't/n\377ame')"
: > t/z.txt
: > t/y.tar.gz
: > t/x
: > t/f10
: > t/f9
: > t/f2
dd if=/dev/zero of=t/big bs=1024 count=1024 status=none

# A third tree whose names' byte counts and column counts disagree, which is
# what every column format is actually laying out. `wide` is two CJK
# characters (six bytes, four columns), `zw` is a letter and a combining
# acute (three bytes, one column) and `bell` a control character -- which GNU
# refuses to measure at all, and then underflows. See `design-decisions.md`
# §367.
mkdir -p w
: > "$(printf 'w/wide\u4e00\u4e00')"
: > "$(printf 'w/zw-e\u0301')"
: > "$(printf 'w/bell\a')"
: > w/AAAA
: > w/BBBB
: > w/CCCC

# A fourth tree holding the one character our width table and coreutils 9.5's
# disagree about *in this fixture*: the soft hyphen, which gnulib calls zero
# columns and glibc calls one. It is kept out of `w/` so that the divergence
# fails two cases here rather than every column case there. The three plain
# names are what make the width visible: with `shy` at three columns it sorts
# below them, with `shy` at four it sorts among them by name.
# See `open-questions.md` "Which width table should `charwidth` follow".
mkdir -p y
: > "$(printf 'y/shy\u00ad')"
: > y/AAAA
: > y/CCCC
: > y/zzzz

# A second tree for -R, with something at every level.
mkdir -p r/one/two
: > r/top
: > r/one/mid
: > r/one/two/leaf

# ------------------------------------------------------------------- cases ---
#
# One shell command line per case, `ls` standing for whichever ls is running.
# Blank lines and `#` lines are ignored; a line beginning `!` is a case we
# expect to differ, and the text after `!` says why.
#
# `ls` is a *function* rather than a textual substitution, and it resolves
# through `PATH` rather than running a path directly, for the two reasons
# `du-diff.sh` sets out at length: a rewrite would misfire on a file name
# containing `ls` and could not express a leading `VAR=value`, and gnulib's
# `set_program_name` keeps the whole of `argv[0]`, so a GNU reached by
# absolute path prefixes every diagnostic with `/usr/bin/ls: `.
ls() { PATH=$LS_PATH command ls "$@"; }

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

    # Captured through *files*, not command substitution. `ls --zero` and
    # `-b`'s escapes are exactly the cases worth measuring, and `$(…)` drops a
    # NUL byte from its result -- from **both** sides, so a difference in where
    # the NULs fall would be reported as agreement. bash even says so
    # ("ignored null byte in input"); a harness that answers a question it did
    # not ask is worse than one that skips it.
    ( LS_PATH=$GNU_PATH; eval "$line" >"$out_a" 2>&1; printf 'rc=%s' "$?" >>"$out_a" )
    ( LS_PATH=$OURS_PATH; eval "$line" >"$out_b" 2>&1; printf 'rc=%s' "$?" >>"$out_b" )

    if cmp -s "$out_a" "$out_b"; then
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
    printf '  ---- unified (gnu < , ours >) ----\n'
    diff <(cat -v "$out_a") <(cat -v "$out_b") \
        | sed 's/^/        /' | sed -n '1,24p'
}

while IFS= read -r case_line; do
    case $case_line in ''|'#'*) continue ;; esac
    run_case "$case_line"
done <<'CASES'
# --- the default listing, and what changes it ---
ls t
ls -1 t
ls -a t
ls -A t
ls -d t
ls -d t/dir t/a
ls t/a
ls t/dir
ls
ls .
ls t r

# --- the long listing ---
ls -l t
ls -l -a t
ls -n t
ls -o t
ls -g t
ls -l -h t
ls -l --si t
ls -l -i t
ls -l -s t
ls -s t
ls -l --block-size=1 t
ls -l --block-size=K t
ls -l -k t
ls -l t/a
ls -l t/link
ls -l t/dangle
ls -l -L t/link
ls -l -H t/link
ls -l -d t/dir

# --- column layouts ---
ls -C -w 40 t
ls -C -w 80 t
ls -C -w 20 t
ls -C -w 200 t
ls -x -w 40 t
ls -x -w 80 t
ls -m -w 40 t
ls -m -w 200 t
ls -C -w 40 -a t
ls -C -T 4 -w 40 t
ls -C -T 0 -w 40 t

# --- sorting ---
ls -t t
ls -S t
ls -r t
ls -U t
ls -X t
ls -v t
ls -t -r t
ls -S -r t
!std::fs::read_dir drops . and .. so we choose a position; ext4 disagrees. known-issues TD-B-LS-INVENTS-A-POSITION-FOR-THE-DOT-ENTRIES|ls -f t
ls --sort=size t
ls --sort=none t
ls --sort=extension t
ls --sort=version t
ls --sort=time t
ls --group-directories-first t
ls --group-directories-first -l t
ls -l -c t
ls -l -u t
ls -t -c t
ls -t -u t

# --- indicators ---
ls -F t
ls -p t
ls --indicator-style=none t
ls --indicator-style=slash t
ls --indicator-style=file-type t
ls --indicator-style=classify t
ls -F -l t

# --- quoting ---
ls -Q t
ls -b t
ls -N t
ls --quoting-style=literal t
ls --quoting-style=shell t
ls --quoting-style=shell-always t
ls --quoting-style=shell-escape t
ls --quoting-style=shell-escape-always t
ls --quoting-style=c t
ls --quoting-style=c-maybe t
ls --quoting-style=escape t
ls -q t
ls --show-control-chars t
ls --hide-control-chars t
QUOTING_STYLE=escape ls t
QUOTING_STYLE=shell-escape ls t
ls --quoting-style=nosuchstyle t
ls -Q -l t
ls -b -l t

# --- recursion ---
ls -R r
ls -R -a r
ls -R -l r
ls -R t
ls -R -1 r

# --- hiding and ignoring ---
ls -I 'f*' t
ls -I '*.txt' t
ls --hide='f*' t
ls --hide='f*' -a t
ls -B t
ls -I 'f*' -I '*.txt' t
ls -a -I 'f*' t

# --- time styles ---
ls -l --time-style=full-iso t
ls -l --time-style=long-iso t
ls -l --time-style=iso t
ls -l --time-style=locale t
ls -l --time-style=+%Y t
ls -l --time-style=+%Y-%m-%d%n%H:%M t
ls -l --full-time t
TIME_STYLE=long-iso ls -l t
TIME_STYLE=+%s ls -l t

# --- other output shapes ---
ls --zero t
ls -w 0 -C t
ls -1 -R r
ls --dereference-command-line-symlink-to-dir t/link
ls -l --dereference-command-line t/link

# --- diagnostics ---
ls nosuchfile
ls t nosuchfile
ls nosuchfile t
ls -l nosuchfile
ls --nosuchoption t
ls -Z t
ls -w nan t
ls -T nan t
ls --sort=nosuch t
ls --time-style=nosuch t
ls --indicator-style=nosuch t
ls --format=nosuch t
ls --color=nosuch t
ls ''
ls -l ''

# --- format aliases ---
ls --format=long t
ls --format=verbose t
ls --format=single-column t
ls --format=vertical -w 40 t
ls --format=across -w 40 t
ls --format=commas -w 40 t

# --- widths that are not byte counts ---
#
# The fixture's `wide` holds two CJK characters (two columns each) and `zw` a
# letter and a combining acute (one column for the pair). Both are laid out
# against names whose byte count and column count differ, which is the whole
# point.
ls -C -w 40 w
ls -C -w 12 w
ls -x -w 40 w
ls -m -w 40 w
ls -C -w 40 -b w
ls -C -w 40 -q w
ls --sort=width -1 w
ls --sort=width -1 t
ls -C -w 40 --quoting-style=escape w
ls -C -w 40 --quoting-style=shell-escape w
ls -l w

# --- deliberately different ---
!our width table is bash's and 9.5's is gnulib's; open-questions "Which width table should charwidth follow"|ls --sort=width -1 y
!same entry: the soft hyphen is zero columns to gnulib and one to glibc, so 22 columns hold one row of four for GNU and two rows of two for us|ls -C -w 22 y
!--help text is ours|ls --help
!--version text is ours|ls --version
!colour is not implemented; known-issues TD-B-LS-ACCEPTS-COLOUR-AND-HYPERLINK-WITHOUT-EMITTING-EITHER|ls --color=always t
!colour is not implemented|ls --color=always -l t
!colour is not implemented|LS_COLORS='di=01;34:ex=01;32' ls --color=always t
!colour is not implemented|ls --color=always -F t
!hyperlinks are not implemented; same entry|ls --hyperlink=always t
!hyperlinks are not implemented|ls --hyperlink=always -l t
CASES

# The unreadable-directory case only means anything as a non-root user.
if [ "$(id -u)" != 0 ]; then
    mkdir -p t/noperm/inner
    chmod 000 t/noperm
    # `ls -R t` is the one case here that prints to *both* streams, so it is
    # the one that shows the flush order -- the listing has to reach the
    # terminal before the diagnostic about the directory it could not open,
    # which is what gnulib's `error()` gets from its `fflush (stdout)`. The
    # other three print a diagnostic and nothing else, so there is no listing
    # for it to be out of order with.
    for c in "ls t/noperm" "ls -l t/noperm" "ls -R t" "ls t"; do
        run_case "$c"
    done
    chmod 755 t/noperm
    rm -rf t/noperm
else
    printf 'note: running as root, so the unreadable-directory cases were skipped\n'
fi

total=$((pass + fail + xfail + xpass))
printf '%d case(s): %d passed, %d differed, %d differ on purpose, %d unexpectedly agreed\n' \
    "$total" "$pass" "$fail" "$xfail" "$xpass"
[ "$fail" = 0 ] || exit 1
