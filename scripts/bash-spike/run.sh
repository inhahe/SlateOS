. "$(dirname "${BASH_SOURCE[0]}")/../lib/worktree.sh" || exit 1

set -x
# The native baseline builds from the same pinned, hash-verified source as the
# cross build, extracted on demand. It used to `cd "$SLATE_SPIKE/bash-5.2"` and
# fail with a bare "No such file" in any checkout where that directory had not
# been unpacked by hand — see slate_ensure_bash_src in scripts/lib/worktree.sh.
slate_ensure_bash_src || exit 1
SRC="$SLATE_SPIKE/bash-$SLATE_BASH_VERSION"
if [ ! -f "$SRC/configure" ]; then
    rm -rf "$SRC"
    mkdir -p "$SRC"
    tar xzf "$SLATE_BASH_TARBALL" -C "$SRC" --strip-components=1 || exit 1
fi

cd "$SRC" || exit 1
./configure --without-bash-malloc >configure.log 2>&1
echo "CONFIGURE_EXIT=$?"
tail -5 configure.log
make -j8 >make.log 2>&1
echo "MAKE_EXIT=$?"
tail -15 make.log
ls -l bash 2>/dev/null && echo "BASH_BINARY_BUILT"
