### A-USB-HID-RECEIVES-FLOOD-THE-RING-AND-FULL-REPORTS-READ-AS-EMPTY -- 2026-10-02 -- FIXED the same day (lane A)

**Status:** FIXED 2026-10-02 (lane A), awaiting a boot on `main`.

**In short:** a USB keyboard or mouse could not be relied on once the kernel
had taken over the USB controller from the firmware. Three faults in how the
driver collected their reports, any one of which loses keystrokes:
- **Full reports were thrown away.** A keyboard sends 8 bytes. The driver read
  the controller's "bytes left over" count as "bytes received", so an 8-byte
  report into an 8-byte request read as 0 bytes and was discarded. Only a
  device whose packets are larger than its reports got through, by accident.
- **The request queue overflowed.** A new "send me your next report" request
  was queued every 8 ms whether or not the last had been answered, so the
  63-entry queue wrapped within half a second of nobody typing, and the
  driver overwrote the request the controller was waiting on. The keyboard
  then alternated between working and silent every half second.
- **Reports went to the wrong place, or nowhere.** All devices share one queue
  of answers. A control request waiting for its own answer took the first
  answer from *any* device, and threw away a keyboard report that came first.

Nobody had seen it because the boot test's USB keyboard is never typed on;
the PS/2 keyboard QEMU also provides carries every test's input.

**Where:** `kernel/src/xhci.rs` -- `poll_hid_report` (the length, and taking
any device's event), `poll_keyboard_locked` and `poll_mouse` (a receive posted
on every poll), `wait_for_event` (other events dropped), `control_transfer`
(the first Transfer Event of any kind accepted as its own).

**Fixed:**
- `IntIn` tracks each HID endpoint: at most one receive posted, a completed
  transfer held until the poll takes it, and the next receive posted only
  then.
- `transferred_len` subtracts the residual (`Trb::residual`).
- `route_event` keeps each HID completion for its own slot and endpoint,
  whoever is reading the event ring. `control_transfer` waits for its own
  slot's endpoint 1, and an event nobody waits for is counted and logged.
- The report is copied out of the receive buffer before the next receive is
  posted over it.
- A failed transfer (a stall, a transaction error) used to be ignored, leaving
  the endpoint halted for good. It now marks the endpoint, and the workqueue
  recovers it: Reset Endpoint, Set TR Dequeue Pointer past the old TRBs, and
  CLEAR_FEATURE(ENDPOINT_HALT) after a stall. After 8 recoveries with no report
  between them it gives up and says so.

`xhci::int_in_self_test` holds the bookkeeping and the residual arithmetic
without a device. A typed key through QEMU's `usb-kbd` is still not tested:
the harness's monitor (`sendkey`) cannot direct a key to the USB keyboard
rather than the PS/2 one. QMP's `input-send-event` can, and would be the way
to add it.
