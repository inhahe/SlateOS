## A-FWUPDATE-INVENTED-THREE-FIRMWARE-DEVICES-AND-REPORTED-FLASHES-THAT-NEVER-HAPPENED (lane A, 2026-09-15) — **Status: FIXED**, pending a boot

**What it was, in two halves.** The second is the serious one.

**Half one -- invented devices.** `fs::fwupdate::init_defaults()` seeded three `FirmwareDevice` entries, and `cmd_fwupdate` calls it before listing, so an operator asking what firmware was present caused three devices to exist and was shown them:

| name | version | available | vendor |
|---|---|---|---|
| System UEFI | 1.20 | **1.22** | SystemVendor |
| TPM 2.0 | 7.85 | — | TPMVendor |
| **Intel I225-V** | 1.68 | **1.70** | Intel |

The third names a **real product** -- an actual 2.5GbE controller -- so this was not a placeholder but a specific claim about the machine. A claimed TPM 2.0 is worse still, being a security device software gates on. The kernel enumerates no firmware whatsoever.

**Half two -- reported flashes that never happened.** `apply_update` was documented `(simulated)`. It writes no firmware: it moves the reported version, sets `PendingReboot`, and pushes an `UpdateRecord { success: true }`. `kshell` then printed **"Firmware update applied for device N. Reboot required."** So the operator was told a flash completed and that they must power-cycle to finish it.

That is a fabricated **action**, not a fabricated fact, and it is the only one of the four found today in that category. `PendingReboot` is an instruction rather than a description; a wrong fact misinforms, a wrong instruction recruits the reader into acting on it. See design-decisions 945.

**The near-miss worth recording.** The call site already carried a careful comment about the danger of applying to the *wrong* device (`fwupdate apply 1O` once parsed as device 0), ending "firmware is also the one thing here that a reboot does not undo." Someone reasoned correctly about the stakes and fixed the argument parsing. Nobody asked whether it wrote firmware at all.

**What changed.** The seeds are gone. `apply_update`'s doc now says it writes no firmware and that `success` means the record was written. `kshell` prints "RECORDED ... NO FIRMWARE WAS WRITTEN" and says not to reboot expecting a flash, following the operator's Q21 precedent for `nft`/`iptables`. Because the module had **no registration path at all** -- devices could only ever come from the seeds -- it gained `register_device` and `unregister_device`, which is the API real firmware enumeration will need anyway, and which lets the self-test build its own fixture instead of asserting against invented constants.

**Note on residue, which differs from the driverupdate case.** `fwupdate::self_test` already wrapped itself in `selftest::with_pristine`, so its fixture never reached `/proc`; `driverupdate::self_test` did not, and its did. The new step 9 here is therefore not what keeps the fixture out of `/proc` -- it is what proves `unregister_device` works, which nothing else would.

**How it was found.** A one-off scan of `kernel/**` for functions that seed named entities into module state, run with the two already-fixed cases as positive controls (neither appeared, as required). 82 candidates, of which nearly all are legitimate definition tables -- a keybinding label, a colour-blindness preset -- and this was the one with the shape. The scan is not a gate and cannot become one: telling an invented device from a defined constant needs prose understanding.
