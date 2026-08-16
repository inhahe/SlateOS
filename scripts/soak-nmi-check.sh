#!/usr/bin/env bash
# One-off soak: boot current kernel repeatedly under the i6300esb NMI watchdog
# with a bounded timeout, stopping on the first NMI catch, liveness dump, or
# nobootok. Verifies whether the current classify_nmi catches the silent wedge.
set -u
cd "$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)" || exit 2
SERIAL="build/serial-test.txt"
# The RIP-capture register dump boot-test.sh writes on timeout (sibling of
# $SERIAL).  Preserved alongside any catch so the wedged RIP/CR2/CR3 survive.
REGS="build/serial-test-regs.txt"
OUT="build/hang-catches"
mkdir -p "$OUT"
N="${1:-12}"
clean=0
# Preserve the serial log AND (if present) the HMP register dump for a catch.
save_catch() {
  local tag="$1"
  cp "$SERIAL" "$OUT/SNMI-CAUGHT-$tag.txt"
  if [ -f "$REGS" ]; then
    cp "$REGS" "$OUT/SNMI-CAUGHT-$tag-regs.txt"
    echo "    wedged-RIP dump saved: $OUT/SNMI-CAUGHT-$tag-regs.txt"
    grep -iE 'RIP=|CR2=|RFL=' "$REGS" 2>/dev/null | head -1 || true
  fi
}
for i in $(seq 1 "$N"); do
  echo "=== soak-nmi iter $i/$N ($(date +%H:%M:%S)) ==="
  rm -f "$REGS"   # don't let a stale dump from a prior iter masquerade as this one's
  bash scripts/boot-test.sh --no-build --hard-lockup-watchdog --timeout=300 >"$OUT/snmi-$i.stdout" 2>&1
  rc=$?
  # rc=4: another lane's live run held the QEMU lock and boot-test refused to
  # start a second emulator, so nothing booted.  The "BOOT_OK missing" test
  # below would score that as a SILENT WEDGE and abort the soak on a catch that
  # never happened.
  if [ "$rc" -eq 4 ]; then
    echo "iter $i: boot lock held by another lane — no boot; retrying in 60s"
    sleep 60
    continue
  fi
  if grep -q "NMI WATCHDOG FIRED" "$SERIAL" 2>/dev/null; then
    echo "!!! iter $i: NMI WATCHDOG FIRED — wedge caught with RIP !!!"
    save_catch "$i-hardlockup"; exit 1
  fi
  if grep -q "\[liveness\] \(SYSTEM HANG\|BOOT DEADLINE\)" "$SERIAL" 2>/dev/null; then
    echo "!!! iter $i: LIVENESS DUMP fired !!!"
    save_catch "$i-liveness"; exit 1
  fi
  if ! grep -q "BOOT_OK" "$SERIAL" 2>/dev/null; then
    echo "!!! iter $i: BOOT_OK missing, NO watchdog dump — SILENT wedge (RIP captured via HMP) !!!"
    save_catch "$i-silent"; exit 1
  fi
  clean=$(( clean + 1 ))
  echo "iter $i: clean BOOT_OK — miss"
done
# Report boots actually completed, not $N: an iteration skipped for lock
# contention proves nothing, and a summary that cannot tell them apart is how
# "12 clean boots" gets claimed for three.
echo "=== $clean clean boots (of $N attempts), no catch ==="; exit 0
