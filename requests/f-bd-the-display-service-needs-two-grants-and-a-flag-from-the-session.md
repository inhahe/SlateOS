# F → B, D — the display service needs two grants and a flag from whatever starts the session

**From:** Lane F. **To:** Lane D (`services/init`, `/etc/startup.conf` in the
rootfs recipe) and Lane B (`init/session`, `init/loginmgr`: whatever ends up
starting the user's desktop shell).
**Filed:** 2026-10-03. **Status:** OPEN. Not urgent: nothing starts the
graphical session yet, and lane A's channel descriptors are not on `main`
yet. This is what the session has to do once it starts one.

**In short:** the compositor can now tell the real desktop shell from an
ordinary program, and keep ordinary programs from reading every window title
on the screen (`known-issues/TD-C-ANY-CLIENT-CAN-READ-EVERY-WINDOW-TITLE.md`).
It does that by asking the kernel who is on each connection, which only works
for programs that connect through the display *service* (a kernel channel)
rather than over TCP. Three things are needed from whoever starts the session.
Without them nothing breaks: programs fall back to TCP and the compositor
lets everyone act as the shell, as it does today.

## What is asked

1. **The compositor may register a service.** Registering
   `org.slateos.Display` needs a `Service` capability with `WRITE`
   (`sys_service_register` checks `require_cap_type(Service, WRITE)`, with any
   resource id). In `/etc/startup.conf` terms (design-decisions §706):
   `caps:Service/0/w`, beside the `InputDevice/0/r` the compositor already
   needs. Without it the compositor logs
   `not serving the display service org.slateos.Display (...)` and serves
   TCP only.
2. **The shell holds the display service's key.** This is a
   `(Service, key_id("org.slateos.Display"), READ)` capability. `key_id` is
   lane A's FNV-1a 64 over the name (`kernel/src/ipc/service.rs`, §1518),
   which for this name is **8925341740578567520** (`0x7bdd2f064a26f560`). The
   shell is `gui/desktop` (lane C's). Only the shell may hold it: any process
   that holds it can act as the shell.
3. **The compositor is started with `--require-shell-key`** in the same
   change that grants the key. The flag arms the gate (§1336). Without it the
   key is granted and nothing checks for it; with it but without the key, the
   taskbar, switcher and panels are refused. So the two belong together, in
   one line of configuration if possible:

   ```text
   /bin/compositor caps:InputDevice/0/r,Service/0/w args:--require-shell-key
   ```

## What lane F has done

- Clients (`oswindow::connect`, `guiremote::Socket::connect_display`) connect
  to the service `org.slateos.Display` when `SLATE_DISPLAY` is unset, and
  fall back to `127.0.0.1:7373` only when there is no such service.
- The compositor, with no address given, listens on both. It records each
  local client's pid, uid and gid as the kernel gives them, and whether the
  client holds the key, then logs one line per connection saying which.
- `ShellGate::KeyHolders` refuses all 22 of the shell's requests to anyone
  the kernel does not name as a key holder. A test pins each request.

## One thing for lane A to know, not to act on here

`Service`/`WRITE` with any resource id lets its holder register **any** name,
so a service granted it could take `org.slateos.Display` before the
compositor starts, and local programs would then connect to it. Today only
granted services hold that capability, so this is a note rather than a bug.
If registration ever becomes per-name (`key_id(name)` with `WRITE`, say), the
compositor's grant in point 1 would name the display service's key id
instead of `0`.
