#!/usr/bin/env bash
# Differential test: our du against GNU du.
#
# ## Why this one is not shaped like the others
#
# Every other `*-diff.sh` builds a *Windows* binary and pipes text at it. That
# cannot work here. `du`'s entire answer is derived from `st_dev`, `st_ino` and
# `st_blocks` — which device a file is on, which inode it is, how many 512-byte
# blocks it actually occupies — and Windows has none of the three. Our `du.rs`
# says so out loud: the whole `RealTree` is behind `#[cfg(unix)]`, and the
# Windows `main` is a two-line stub that prints "unix-only utility". Pointing
# the usual harness at it would compare GNU's output against that stub.
#
# So this harness runs *inside* WSL, on both sides, over a fixture tree built
# in WSL's own ext4. That last part is not incidental either: a tree built on
# `/mnt/d` is 9p, where `st_blocks` is synthesised and hard links do not behave,
# so the two properties this utility exists to report would both be fiction.
#
# The cost is a second Rust toolchain, in WSL, which this script installs
# nothing to obtain — it expects `~/.cargo/bin/cargo` and skips loudly if it is
# absent, with the one command that fixes that in the message. The benefit is
# not confined to `du`: `ls`, `find`, `stat`, `df` and `ln` all answer
# questions that only a real inode can answer, and this is the road for them
# too.
#
# ## What is compared
#
# stdout, stderr and the exit status, byte for byte, for each case below. Not
# whitespace-normalised: `du`'s output is a number, a tab and a name, and a
# comparison that trimmed would be blind to the `-0` cases, where the whole
# point is which byte ends the row.
#
# Directory order is compared too, deliberately. Neither implementation sorts —
# GNU passes a null comparator to `fts_open`, we hand `read_dir` straight
# through — so both see the same `getdents` order and a difference here would
# mean one of us had started sorting.
#
#     sh scripts/du-diff.sh          # run it
#     OURS=/usr/bin/du sh scripts/du-diff.sh   # control: should be all green
#
set -u

# ------------------------------------------------------------------ outer ---
# Run from MSYS, this half only hands the whole job to WSL and gets out of the
# way. `wsl` inherits the Windows cwd translated to `/mnt/...`, which is why
# the relative path below resolves — an absolute MSYS path would be mangled.
if [ "${DU_DIFF_INNER:-}" != 1 ]; then
    cd "$(dirname "$0")/.." || exit 1
    if ! wsl -e true >/dev/null 2>&1; then
        echo "du-diff: WSL is not available; SKIPPED"
        exit 0
    fi
    exec wsl -e env DU_DIFF_INNER=1 "OURS=${OURS:-}" LC_ALL=C.UTF-8 \
        bash ./scripts/du-diff.sh
fi

# ------------------------------------------------------------------ inner ---
export LC_ALL=C.UTF-8
# `du` reads three of these and would otherwise inherit whatever the operator's
# shell has set, which would change both sides but is not what we mean to test.
unset DU_BLOCK_SIZE BLOCK_SIZE BLOCKSIZE POSIXLY_CORRECT

# Resolved to a path here, while `du` still means the system's — a few lines
# below it becomes a shell function and `command -v du` would answer "du".
GNU=${GNU:-$(command -v du)}
OURS=${OURS:-}

if [ -z "$OURS" ]; then
    export PATH="$HOME/.cargo/bin:$PATH"
    if ! command -v cargo >/dev/null 2>&1; then
        cat >&2 <<'MISSING'
du-diff: no cargo inside WSL, so our du cannot be built for Linux.

Install one (a per-user toolchain, nothing system-wide):

  wsl -e sh -c "curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs \
      | sh -s -- -y --default-toolchain stable --profile minimal"

MISSING
        echo "du-diff: cargo missing inside WSL; SKIPPED"
        exit 0
    fi
    # Built every run, from the package named, for the same reason
    # `scripts/diff-subject.sh` gives: a harness that merely *runs* a path
    # measures whatever was last written there, which need not be current and
    # need not even be this crate.
    #
    # The target directory is under WSL's own home rather than the repository:
    # the repository is on 9p through `/mnt/d`, where a Rust build is an order
    # of magnitude slower, and a second `target/` inside the tree would be a
    # tens-of-gigabytes surprise for whoever next runs `du -sh` on the
    # worktree.
    root=$(cd "$(dirname "$0")/.." && pwd) || exit 1
    ( cd "$root/userspace/coreutils" \
      && CARGO_TARGET_DIR="$HOME/du-diff-target" \
         cargo build --bin du --target x86_64-unknown-linux-gnu ) >&2 || {
        echo "du-diff: the build failed"
        exit 1
    }
    OURS="$HOME/du-diff-target/x86_64-unknown-linux-gnu/debug/du"
fi

if [ ! -x "$OURS" ]; then
    echo "du-diff: $OURS is not executable"
    exit 1
fi

# Both are about to become the target of a symlink in a directory that is not
# this one, and a relative `OURS=` — which the override exists to accept — would
# then point at nothing.
GNU=$(cd "$(dirname "$GNU")" && pwd)/$(basename "$GNU") || exit 1
OURS=$(cd "$(dirname "$OURS")" && pwd)/$(basename "$OURS") || exit 1

pass=0; fail=0; xfail=0; xpass=0

fixtures=$(mktemp -d /tmp/du-diff.XXXXXX) || exit 1
trap 'rm -rf "$fixtures"' EXIT

# Each implementation gets a directory of its own containing one symlink named
# `du`, and a case is run with that directory first on `PATH`. The point is
# `argv[0]`: see the comment on the `du` function below. The directories are
# under `$fixtures` so the existing trap removes them, but *not* under the tree
# the cases walk — a `bin` directory inside `t` would change every size in
# every expectation.
mkdir -p "$fixtures/bin-gnu" "$fixtures/bin-ours" || exit 1
ln -s "$GNU" "$fixtures/bin-gnu/du" || exit 1
ln -s "$OURS" "$fixtures/bin-ours/du" || exit 1
GNU_PATH="$fixtures/bin-gnu:$PATH"
OURS_PATH="$fixtures/bin-ours:$PATH"

mkdir -p "$fixtures/tree" || exit 1
cd "$fixtures/tree" || exit 1

# ---------------------------------------------------------------- fixtures ---
#
# Each of these exists to make one rule observable:
#
#   t/f, t/sub/g   an ordinary file at two depths, so `-d`, `-S` and `-a` have
#                  something to disagree about
#   t/sub/deep/h   a third level, which is what tells `-S`'s two accumulators
#                  apart: at two levels their totals coincide
#   t/hard         a second name for t/f's inode — the case the old du got
#                  wrong, counting the blocks twice
#   t/sparse       apparent size 1 MiB, zero blocks on disk: the one file where
#                  `--apparent-size` and the default cannot agree
#   t/big          larger than one block, so a block size is a division rather
#                  than a rounding
#   t/dangle       a symlink to nothing, which `-L` turns into the errno-less
#                  "cannot access" and the default counts as itself
#   t/loop         a symlink to its own directory, so following is not free
#   t/.hidden      a leading dot, which `--exclude` patterns treat as ordinary
#   t/sp ace, t/n  a space and a non-UTF-8 byte in a name, which is the case
#                  the old du could not be handed at all
mkdir -p t/sub/deep
dd if=/dev/zero of=t/f bs=1024 count=3 status=none
dd if=/dev/zero of=t/sub/g bs=1024 count=5 status=none
dd if=/dev/zero of=t/sub/deep/h bs=1024 count=7 status=none
dd if=/dev/zero of=t/big bs=1024 count=3000 status=none
ln t/f t/hard
truncate -s 1M t/sparse
ln -s nowhere t/dangle
ln -s ../sub t/sub/loop
: > t/.hidden
: > "t/sp ace"
printf '' > "$(printf 't/n\377ame')"
mkdir -p t/empty

printf 'sub\n\n*ig\n' > excl.txt
printf 't/f\0t/sub\0' > list0
# One empty name *between* two good ones, which is what shows whether the
# complaint is made where it stands or in a pass of its own.
printf 't/f\0\0t/sub\0' > list0-empty
printf '\0\0t/f\0' > list0-two-empties
# A list whose last name has no NUL after it. It is still a name — the final
# empty element is only dropped when there *is* one.
printf 't/f\0t/sub' > list0-no-trailing-nul
printf 't/f\0' > "sp ace list"

# ------------------------------------------------------------------- cases ---
#
# One shell command line per case, `du` standing for whichever du is running.
# Blank lines and `#` lines are ignored; a line beginning `!` is a case we
# expect to differ, and the text after `!` says why.
#
# `du` is a *function*, not a textual substitution. Rewriting the word would
# misfire the first time a case names a file with `du` in it, and it could not
# express the `BLOCK_SIZE=1M du t` cases at all, where the thing to replace is
# not the first word. The function is evaluated inside a command substitution,
# which is a subshell, so a leading variable assignment cannot leak from one
# case into the next — bash does let an assignment prefixed to a *function*
# call persist, and several cases here deliberately set a variable that would
# change every case after them.
#
# It resolves `du` through `PATH` rather than running a path directly, and that
# is not a stylistic choice. gnulib's `set_program_name` keeps the whole of
# `argv[0]`, so GNU du invoked as `/usr/bin/du` prefixes every diagnostic with
# `/usr/bin/du: ` — while ours says `du: `, because every utility in this tree
# spells its own name (`Program::new("du", 1)`). Comparing those would report a
# difference in 48 cases that is entirely an artefact of how the harness
# started the program. A `PATH` lookup makes `argv[0]` the word `du` on both
# sides, which is also what the shell will do on the target.
du() { PATH=$DU_PATH command du "$@"; }

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

    a=$(DU_PATH=$GNU_PATH; eval "$line" 2>&1; printf 'rc=%s' "$?")
    b=$(DU_PATH=$OURS_PATH; eval "$line" 2>&1; printf 'rc=%s' "$?")

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
    printf '  gnu:  %s\n' "$(printf '%s' "$a" | sed -n '1,12p' | cat -v | sed 's/^/        /')"
    printf '  ours: %s\n' "$(printf '%s' "$b" | sed -n '1,12p' | cat -v | sed 's/^/        /')"
    printf '  ---- unified ----\n'
    diff <(printf '%s\n' "$a" | cat -v) <(printf '%s\n' "$b" | cat -v) \
        | sed 's/^/        /' | sed -n '1,24p'
}

while IFS= read -r case_line; do
    case $case_line in ''|'#'*) continue ;; esac
    run_case "$case_line"
done <<'CASES'
# --- the shapes of the listing ---
du t
du -a t
du -s t
du -c t
du -S t
du -a -c -S t
du -a -S t
du --separate-dirs --total t
du -d0 t
du -d1 t
du -d2 t
du -d2 -a t
du -d 0x2 -a t
du -d 010 t
du -d -1 t
du --max-depth=1 -c t
du -s -c t
du -a -c t

# --- what is being counted ---
du -b t
du -a -b t
du --apparent-size t
du -a --apparent-size t
du --inodes t
du -a --inodes t
du -c --inodes t
du -s --inodes t
du -l t
du -a -l t
du -a -l -c t
du -x t
du -a -x t

# --- how it is rendered ---
du -k t
du -m t
du -h t
du -a -h t
du --si t
du -h -a -c t
du -B 1 t
du -B 512 t
du -B 1024 t
du -B 1K t
du -B 0x400 t
du -B 1KiB t
du -B 1KB t
du -B 1M t
du -B K t
du -B M t
du -B G t
du -B KB t
du -B KiB t
du -B MiB t
du -B human-readable t
du -B human t
du -B h t
du -B si t
du -B s t
du --block-size=1M t
du -B "'1" t
du -B "'1048576" t
du -0 t
du -0 -c t
du -0 -a t
du --null -a -c t

# --- the last one wins, and which flags are sticky ---
du -b -k t
du -k -b t
du --apparent-size -k t
du -h -k t
du -k -h t
du --si -h t
du -h --si t
du -B 1M -k t
du -k -B 1M t

# --- the environment ---
DU_BLOCK_SIZE=1M du t
BLOCK_SIZE=1M du t
BLOCKSIZE=1M du t
DU_BLOCK_SIZE=1M BLOCK_SIZE=512 du t
DU_BLOCK_SIZE= BLOCK_SIZE=1 du t
BLOCK_SIZE=zz du t
BLOCK_SIZE=0 du t
POSIXLY_CORRECT=1 du t
POSIXLY_CORRECT=1 du -B 1M t
DU_BLOCK_SIZE=1M du -k t

# --- thresholds ---
du -t 4096 t
du -t -4096 t
du --threshold=1M t
du --threshold=-1M t
du -t 1K -a t
du -t 1M -c t
du -t 0 t
du --inodes -t 2 t

# --- exclusion ---
du --exclude=sub t
du --exclude=sub -a t
du --exclude='*g' -a t
du --exclude='t/*' t
du --exclude='*/deep' -a t
du --exclude='.hidden' -a t
du --exclude='.*' -a t
du --exclude=t t
du --exclude='t' 't/'
du -X excl.txt -a t
du -X excl.txt -c t
du --exclude-from=excl.txt t
du -X - t < excl.txt
du -X '' t
du -X t/empty t

# --- symlinks ---
du -a t/dangle
du -a -L t/dangle
du -a -P t/dangle
du -a -D t/dangle
du -a -H t/dangle
du -L t
du -a -L t
du -D t
du -H t
du -a -L -c t
du -L -H t
du -H -L t
du -P -L t
du -L -P t

# --- names ---
du -a "t/sp ace"
du -a t/.hidden
du -a t/empty
du t//
du t/
du t/sub/
du -a t/sub/
du t/f
du t t
du -c t t
du -s t t
du t/f t/f
du -c t/f t/sub
du -a -c t/sub t/sub/deep

# --- --files0-from ---
du --files0-from=list0
du --files0-from=list0 -c
du --files0-from=list0 -a
du --files0-from=list0-empty
du --files0-from=- < list0
du --files0-from=list0 t
du --files0-from=nosuchfile
du --files0-from=t
du --files0-from=t/empty
du --files0-from=
du --files0-from=- < list0-empty
du --files0-from=list0-two-empties
du --files0-from=list0-no-trailing-nul
du --files0-from="sp ace list"

# --- an empty name, from the command line rather than from a list ---
du ""
du t/f ""
du -c t/f ""

# --- diagnostics ---
du nosuchfile
du t nosuchfile
du -c t nosuchfile
du -q t
du --nosuchoption t
du --e=x t
du -s -a t
du -s -d1 t
du -s -d0 t
du -s --max-depth=2 t
du -B Z t
du -B Y t
du -B R t
du -B Q t
du -B 1B t
du -B 1E100 t
du -B 0 t
du -B -1 t
du -B "" t
du -B Si t
du -B ki t
du -B 1i t
du -B 1e3 t
du -d zz t
du -d 9223372036854775808 t
du --max-depth=zz t
du -t zz t
du -t 1X t
du -t -0 t
du --threshold=-0 t
du -X nosuchfile t
du -X nosuchfile -d zz t

# --- deliberate differences ---
!our --help omits --time/--time-style, which we do not implement|du --help
!our version string is ours|du --version
!--time is refused rather than implemented|du --time t
!--time-style is refused rather than implemented|du --time-style=full-iso t
!strtol_fatal escapes an embedded quote; GNU interpolates it raw|du -B "1'024" t
CASES

# The unreadable-directory case only means anything as a non-root user: root
# reads it regardless, so both sides would silently agree on the wrong thing.
if [ "$(id -u)" != 0 ]; then
    mkdir -p t/noperm/inner
    chmod 000 t/noperm
    printf 't/f\0' > unreadable && chmod 000 unreadable
    for c in "du t" "du -a t" "du -c t" "du t/noperm" \
             "du --files0-from=unreadable" "du -X unreadable t/f"; do
        run_case "$c"
    done
    chmod 755 t/noperm
    rm -f unreadable
else
    printf 'note: running as root, so the unreadable-directory cases were skipped\n'
fi

total=$((pass + fail + xfail + xpass))
printf '%d case(s): %d passed, %d differed, %d differ on purpose, %d unexpectedly agreed\n' \
    "$total" "$pass" "$fail" "$xfail" "$xpass"
[ "$fail" = 0 ] || exit 1
