## B-LOGIND-SERVES-A-THREAD-PER-CLIENT-UNTIL-A-PORT-CAN-WAIT-ON-CHANNELS — `logind` holds a thread for every connected client, because the kernel cannot wake one waiter for many channels (lane B, 2026-10-01) — **Status: OPEN (debt, deliberate; blocked on lane A)**

**In short:** a server should wait on all its clients, and on new ones
arriving, from one place. On SlateOS that place is a *completion port* (a
kernel object a program waits on to hear about many things at once), and
today it cannot do the job: a message arriving on a channel does not wake
it, and "a client is connecting" is not something it can wait for at all.
So `logind` gives each connected client a thread that waits on that client
alone. That works, and costs a thread per idle client.

**Where:** `userspace/logind/src/main.rs`, `serve` and `serve_client`;
`MAX_CLIENTS` (64) and `CLIENT_STACK` (512 KiB) bound it. The reasoning and
the alternatives are `design-decisions.md` §1054.

**What lifts it:** both of these from lane A --
`requests/f-a-a-channel-handle-can-be-guessed-and-any-process-can-use-it.md`
point 4 (lane F's: `channel::send` must notify the ports that registered
the channel) and
`requests/b-a-a-server-cannot-wait-for-a-new-client-and-its-clients-at-once.md`
(a "listener" wait source). Then `libservicebus` gets `register_listener`
back with a real source, and `serve` goes back to one event loop.

**Related, and also lane A's:** the same request's point 1 (a channel handle
can be guessed, and the channel syscalls do not check the caller holds it),
which means `Connection::peer_credentials` proves who *connected*, not who is
sending; and point 3 (a Linux-ABI process cannot reach these syscalls at
all). Neither has a workaround in lane B's code.

**How to see it:** nothing visible today -- `logind` is not on the image yet
(§1053's staging request). On the device, `/proc/<logind pid>/task` would
list one thread per connected client plus the accepting thread.
