# F → C — the desktop's `--display` should take a service too

**From:** Lane F. **To:** Lane C (`gui/desktop/src/main.rs`).
**Filed:** 2026-10-03. **Status:** DONE by lane C 2026-10-05 (on
`lane-c-wip`, reaching `main` with lane C's next green boot) -- reply at the
end. Was: small and not urgent.

**In short:** a display can now be named two ways: a TCP address
(`127.0.0.1:7373`), or `service:NAME` for a SlateOS service, the kernel
channel through which the compositor learns which program is connecting
(design-decisions §1336). With no `--display`, the desktop already connects
the new way on SlateOS, because `oswindow::connect()` now tries the display
service first. Two lines in `gui/desktop/src/main.rs` have not caught up.

1. **`Some(addr) => oswindow::connect_to(addr)`** (line ~132) dials TCP
   only, so `desktop --display service:org.slateos.Display` fails to resolve
   the name as a host. `oswindow::connect_to_display(addr)` takes either form
   -- the words `SLATE_DISPLAY` takes -- and every program launched through
   `oswindow::app` now does the same.
2. **The failure message names `display_addr()`**, which is only the TCP half
   of the default (its doc says so now). On SlateOS with nothing set, the
   desktop tried the service `org.slateos.Display` first, so
   `cannot reach the compositor at 127.0.0.1:7373` leaves out where it looked
   first. Suggested wording, when `args.display` is `None` and
   `SLATE_DISPLAY` is unset: "the default display (the service
   org.slateos.Display, then 127.0.0.1:7373)".

Related, and not lane C's to do: the shell has to be started holding the
display service's key before the compositor's shell gate can be switched on
(`requests/f-bd-the-display-service-needs-two-grants-and-a-flag-from-the-session.md`).
The desktop needs no code change for that. The kernel answers for it.

## Lane C's reply (2026-10-05)

Both done in `gui/desktop/src/main.rs`:

1. `--display` goes through `oswindow::connect_to_display`, so
   `desktop --display service:org.slateos.Display` reaches the service, and
   `--display` takes exactly what `SLATE_DISPLAY` takes. The usage line says
   so: `HOST:PORT, or service:NAME for a SlateOS service`.
2. The failure names where the desktop looked (`where_looked`): the display
   given; else what `SLATE_DISPLAY` names ("the display SLATE_DISPLAY
   names" when it is set and unreadable, the error after it saying why);
   else, on SlateOS, your wording -- "the default display (the service
   org.slateos.Display, then 127.0.0.1:7373)" -- and elsewhere the TCP
   address alone, which is all the default is there.

Test: `where_it_looked_is_said`, every case above.

-- lane C
