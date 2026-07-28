# `$PWD` and `$OLDPWD` are reconciled with the *real* working directory when a
# shell starts: the environment is not trusted, because whoever exported those
# names may have moved (or lied) afterwards. Measured against bash 5.2:
#   * an inherited `$PWD` that does not name the current directory is replaced
#     by the real one — even when it names some *other* perfectly good
#     directory;
#   * an inherited `$OLDPWD` is kept only while it still names a directory
#     (a relative one is kept verbatim), otherwise it is unset, so `cd -`
#     cannot be handed a stale path;
#   * both names carry the export attribute from the start, even while unset —
#     so a later `cd` publishes them to children with no `export` — but an
#     explicit `export -n PWD` / `unset OLDPWD` still drops it for good.
#
# The child shells are started through `$BASH`, which both shells set to their
# own executable; its spelling differs per host, so it is never printed.
mkdir -p root/sub
cd root
ROOT=$PWD
: >f

probe() {
    PWD=$1 OLDPWD=$2 "$BASH" -c '
        echo "  pwd-is-real=$([ "$PWD" = "$(pwd)" ] && echo y || echo n)"
        echo "  old=${OLDPWD-UNSET}"
        echo "  exported: pwd=$(env | grep -c "^PWD=") old=$(env | grep -c "^OLDPWD=")"
    '
}

echo "=== inherited PWD is bogus"
probe /no/such/dir /no/such/dir
echo "=== inherited PWD names another real directory"
probe "$ROOT/sub" /no/such/dir
echo "=== inherited OLDPWD is a real directory"
probe /no/such/dir sub
echo "=== inherited OLDPWD is a plain file"
probe /no/such/dir "$ROOT/f"

echo "=== the export attribute survives cd"
cd sub
echo "after-cd: pwd=$(env | grep -c '^PWD=') old=$(env | grep -c '^OLDPWD=')"
cd ..

echo "=== but export -n / unset drop it for good"
export -n PWD
cd sub
echo "after-export-n: pwd=$(env | grep -c '^PWD=')"
cd ..
unset OLDPWD
cd sub
echo "after-unset: old=$(env | grep -c '^OLDPWD=')"
