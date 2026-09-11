#!/bin/bash
# Self-test for `slate_ensure_src` in scripts/lib/worktree.sh.
#
# That function resolves the pinned source tarball for a port, and it was four
# near-identical copies until 2026-09-11 -- one per port, 92-97% identical to
# each other after renaming the package out of them. None of them had a test.
#
# The rules being pinned here are the ones each copy was carrying separately,
# and each is a rule some copy had already got wrong at least once:
#
#   * a tarball already on disk is accepted only if it hashes to the pin;
#   * one that does not hash is IGNORED, never deleted, because it may be the
#     evidence of the truncated download the pin exists to catch;
#   * a download that fails leaves nothing behind that a later run would treat
#     as cached -- a 404 body once became a permanent "cached tarball" here;
#   * every message goes to stderr, because the caller captures stdout to get
#     the path. This one is new with the refactor and is the load-bearing one:
#     a single stray `echo` without `>&2` would put prose into a filename and
#     the failure would be a confusing "no such file", nowhere near the cause.
#
# No network: the download path is exercised through a `file://` URL.
set -uo pipefail

. "$(dirname "${BASH_SOURCE[0]}")/lib/worktree.sh" || exit 1

PASS=0
FAIL=0

ok() { PASS=$((PASS + 1)); }
bad() {
    FAIL=$((FAIL + 1))
    echo "test-worktree FAIL $1"
}

TD="$(mktemp -d)" || exit 1
trap 'rm -rf "$TD"' EXIT

# A payload and its real digest. Computed, never hard-coded: a fixture whose
# expected hash is a literal stops testing the hashing the moment the payload
# is edited.
mkdir -p "$TD/origin" "$TD/cand" "$TD/cache"
printf 'slateos test payload\n' > "$TD/origin/widget-1.0.tar.gz"
GOOD="$(sha256sum "$TD/origin/widget-1.0.tar.gz" | cut -d' ' -f1)"
WRONG="0000000000000000000000000000000000000000000000000000000000000000"
URL="file://$TD/origin/widget-1.0.tar.gz"

export SLATE_ZIG_CACHE="$TD/cache"

# 1. A candidate that hashes to the pin is returned, and nothing is downloaded.
cp "$TD/origin/widget-1.0.tar.gz" "$TD/cand/"
got="$(slate_ensure_src widget 1.0 "$GOOD" "$URL" "$TD/cand" 2>/dev/null)"
if [ "$got" = "$TD/cand/widget-1.0.tar.gz" ]; then ok; else
    bad "a matching candidate should be returned as-is, got '$got'"
fi
if [ -f "$TD/cache/widget-1.0.tar.gz" ]; then
    bad "a matching candidate must not trigger a download"
else ok; fi

# 2. A candidate that does NOT hash is ignored -- and survives.
printf 'truncated\n' > "$TD/cand/widget-1.0.tar.gz"
got="$(slate_ensure_src widget 1.0 "$GOOD" "$URL" "$TD/cand" 2>/dev/null)"
if [ "$got" = "$TD/cache/widget-1.0.tar.gz" ]; then ok; else
    bad "a mismatching candidate should be skipped and the pin fetched, got '$got'"
fi
if [ -f "$TD/cand/widget-1.0.tar.gz" ]; then ok; else
    bad "the mismatching candidate was DELETED; it is the only copy of the bad bytes"
fi

# 3. Every message goes to stderr, so stdout is exactly the path. This is what
#    the four callers rely on and what nothing checked before.
rm -rf "$TD/cache"
out="$(slate_ensure_src widget 1.0 "$GOOD" "$URL" "$TD/cand" 2>/dev/null)"
if [ "$(printf '%s' "$out" | wc -l)" -eq 0 ] && [ -f "$out" ]; then ok; else
    bad "stdout must be one path and nothing else, got: $out"
fi

# 4. A pin that does not match the artifact is refused, and the caller can tell.
rm -rf "$TD/cache"
if slate_ensure_src widget 1.0 "$WRONG" "$URL" > /dev/null 2>&1; then
    bad "a tarball whose sha256 is not the pin must be refused"
else ok; fi

# 5. ...and the refusal says which digest it got, not merely that it failed.
rm -rf "$TD/cache"
msg="$(slate_ensure_src widget 1.0 "$WRONG" "$URL" 2>&1 >/dev/null)"
case "$msg" in
    *"$GOOD"*) ok ;;
    *) bad "the mismatch message should name the digest it computed: $msg" ;;
esac

# 6. A download that cannot be made leaves nothing a later run would trust.
rm -rf "$TD/cache"
if slate_ensure_src widget 1.0 "$GOOD" "file://$TD/origin/no-such-file.tar.gz" \
    > /dev/null 2>&1; then
    bad "a failed download must not report success"
else ok; fi
if [ -e "$TD/cache/no-such-file.tar.gz" ] || [ -e "$TD/cache/no-such-file.tar.gz.part" ]; then
    bad "a failed download left a file behind for a later run to treat as cached"
else ok; fi

echo "test-worktree: $PASS/$((PASS + FAIL)) cases pass"
[ "$FAIL" -eq 0 ]
