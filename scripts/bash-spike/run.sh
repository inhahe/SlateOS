. "$(dirname "${BASH_SOURCE[0]}")/../lib/worktree.sh" || exit 1

set -x
cd "$SLATE_SPIKE/bash-5.2" || exit 1
./configure --without-bash-malloc >configure.log 2>&1
echo "CONFIGURE_EXIT=$?"
tail -5 configure.log
make -j8 >make.log 2>&1
echo "MAKE_EXIT=$?"
tail -15 make.log
ls -l bash 2>/dev/null && echo "BASH_BINARY_BUILT"
