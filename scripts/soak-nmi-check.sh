#!/usr/bin/env bash
# One-off soak: boot current kernel repeatedly under the i6300esb NMI watchdog
# with a bounded timeout, stopping on the first NMI catch, liveness dump, or
# nobootok. Verifies whether the current classify_nmi catches the silent wedge.
set -u
cd "$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)" || exit 2
SERIAL="build/serial-test.txt"
OUT="build/hang-catches"
mkdir -p "$OUT"
N="${1:-12}"
for i in $(seq 1 "$N"); do
  echo "=== soak-nmi iter $i/$N ($(date +%H:%M:%S)) ==="
  bash scripts/boot-test.sh --no-build --hard-lockup-watchdog --timeout=300 >"$OUT/snmi-$i.stdout" 2>&1
  if grep -q "NMI WATCHDOG FIRED" "$SERIAL" 2>/dev/null; then
    echo "!!! iter $i: NMI WATCHDOG FIRED — wedge caught with RIP !!!"
    cp "$SERIAL" "$OUT/SNMI-CAUGHT-$i-hardlockup.txt"; exit 1
  fi
  if grep -q "\[liveness\] \(SYSTEM HANG\|BOOT DEADLINE\)" "$SERIAL" 2>/dev/null; then
    echo "!!! iter $i: LIVENESS DUMP fired !!!"
    cp "$SERIAL" "$OUT/SNMI-CAUGHT-$i-liveness.txt"; exit 1
  fi
  if ! grep -q "BOOT_OK" "$SERIAL" 2>/dev/null; then
    echo "!!! iter $i: BOOT_OK missing, NO watchdog dump — SILENT wedge !!!"
    cp "$SERIAL" "$OUT/SNMI-CAUGHT-$i-silent.txt"; exit 1
  fi
  echo "iter $i: clean BOOT_OK — miss"
done
echo "=== $N clean boots, no catch ==="; exit 0
