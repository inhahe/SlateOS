## 1518. A service's key is a capability for its name, checked by the kernel on the connection's peer

**Date:** 2026-10-02 · **Decided by:** Claude (autonomous), within the policy the operator set in C-Q25 (§1417) · **Lane:** A

**In short:** the operator decided that a program may ask the password
manager for a stored password only if it holds "a system-issued key" for
exactly that. This entry is what that key is, and how the password manager
checks it.
- **The key** is an entry in the program's capability table (the list of
  things the kernel lets it do), naming the service. Like every other
  capability, it can only be handed on by someone who holds it, typically
  the launcher when it starts the program, following the user's choices in
  Settings.
- **The check:** the password manager asks the kernel one question about the
  program connected to it: "does the program at the other end hold my key?"
  The kernel answers from its own records. The password manager never
  handles the key and cannot be fooled about who is asking
  (`requests/c-a-a-capability-to-ask-the-credential-service-for-a-password.md`).

**What changed:**
- A service's key is `(ResourceType::Service, key_id(name), Rights::READ)`.
  `ipc::service::key_id(name)` is FNV-1a-64 of the name, never 0, so it never
  names the class.
- A connection remembers which service it was made to
  (`channel::service_key`, recorded by `service::connect`).
- `SYS_CHANNEL_PEER_HAS_KEY` (1103), and `slate_channel_peer_has_key` (1005)
  by descriptor, answer 1 or 0. The peer is the process the kernel recorded
  at connect time, and it must still hold its end.

**Alternatives:**

| | What changes | For | Against |
|---|---|---|---|
| **A. The service asks the kernel about its peer (chosen)** | one call: "does my peer hold my key?" | the client does nothing special to be checked; the service cannot name the wrong key, since the kernel takes it from the connection; nothing moves out of anyone's table | the service learns only yes or no, not what else the client holds -- which is the right amount |
| B. The client presents the key in its request | the client transfers the capability with the message; the service asks "is this genuine?" | the textbook capability shape: the request carries its authority | this kernel's transfer *moves* a capability, so presenting it spends it -- the client would need a fresh one per request, or the service would have to give it back |
| C. A key the service issues and checks itself | the service keeps its own list | no kernel work | an allowlist, not a capability -- what lane C said it must not invent |

**Smaller decisions:**

| decision | alternative | why this one |
|---|---|---|
| The key is a `Service` capability with a per-name id and `READ` | a new resource type | the type already means "about services" (`WRITE` on the class lets a process register names); a per-name id with `READ` reads as "may use this service", with no new type for every table and match |
| The id is a public, unkeyed FNV-1a of the name | a registry id handed out at registration | keys are granted at spawn, often before the service has registered; anyone may compute the id, but an id is not a key -- only a table entry is, and only a holder can hand one on |
| The peer must still hold its end of the channel | trust the recorded pid alone | a client that exited could have its pid reused; a reused pid holds no end of this channel, so it answers 0 |
| No class-wide "any service" key | a `(Service, 0, READ)` that opens every service | a superkey is a policy the operator has not asked for; `has_resource` matches ids exactly, so none exists |

**Who grants keys:** whoever starts the program, from its own table (the
spawn subset rules). Lane C's Settings page records the user's choice; the
launcher that reads it must itself hold the key it passes on. Which process
first holds each key -- the session manager, given them at boot -- belongs
to the launcher's design, not this entry.

**Revisit** if a service needs to tell a client which keys it would accept,
or if keys should expire.
