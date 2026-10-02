## D-Q4 — [D] Background services that run before anyone signs in need passwords too. Where should they keep them? — Status: OPEN (raised 2026-09-27)

**In short:** some programs that run in the background need a password to do
their job, and must do it while nobody is signed in: dynamic DNS (keeping a
web address such as `myhome.duckdns.org` pointed at a home network) needs the
DNS provider's password; a scheduled backup to another computer needs that
computer's password; joining Wi-Fi at startup needs the network's passphrase.
The password manager cannot give them one at startup: each user's store is
locked with that user's own master password, so until they sign in and unlock
it, nothing can read it -- even a program you have allowed to (your C-Q25
answer). These passwords need a home a background service can reach at
startup, and the choice is how well that home is protected.

**Terms used below.** *Background service*: a program the system starts at
boot, before any sign-in (the backup scheduler and the dynamic-DNS updater are
two). *Encrypted at rest*: stored scrambled, so reading the disk directly -- a
stolen laptop, or the disk moved to another machine -- shows nothing useful.
*TPM*: a security chip in most PCs that keeps a key and hands it over only to
this machine's own, unmodified startup. *Capability*: a permission a program
holds as a token, as in C-Q25's "a capability key specifically for this".

| Option | *What changes:* |
|---|---|
| **A.** A file only the service can read -- what Linux does for Wi-Fi (`/etc/wpa_supplicant/wpa_supplicant.conf`) and NetworkManager's saved networks | Works at startup, simple. The password is stored unscrambled, guarded by who may open the file: other programs on the running system cannot read it, but anyone who reads the disk directly can -- unless the whole disk is encrypted, which then covers it (the kernel has a volume-encryption module, `fs::diskencrypt`; nothing encrypts the system disk with it yet). |
| **B.** A *system* section of the password manager, unlocked at startup with a key the TPM keeps, readable by services holding a capability for it -- C-Q25's idea, extended to startup | Encrypted at rest even without whole-disk encryption, and one place to see and revoke every stored service password. Needs TPM support, which does not exist yet; on a machine without a TPM the unlocking key must sit on disk, which makes it A with extra steps. |
| **C.** No stored passwords for background services -- they run only while their owner is signed in and has unlocked the password manager, reading it through C-Q25's capability | Nothing new to protect. Dynamic DNS, backups to another computer and Wi-Fi at startup stop whenever nobody is signed in -- which for a home server is all the time. |

**If never answered:** nothing gets worse today -- no background service
stores a password yet. It blocks the part of the dynamic-DNS updater that signs
in to the provider (`requests/e-ad-dynamic-dns-is-a-userspace-service-not-a-kernel-table.md`);
the rest of it can be built meanwhile.

**Claude's recommendation:** **A now, B when a TPM-backed store exists.** A is
what every Linux system does for these same passwords, and whole-disk
encryption, once the system disk uses it, gives A the at-rest protection B
would. The service, not Settings, writes the file (Settings hands it the
password and the service checks Settings may), and each service reads its
passwords through one small function -- so moving to B later changes that
function and nothing else.

**Where it bites:** `services/dyndns` (lane D, not yet written); lane E's
Dynamic DNS page in `apps/settings/src/remote.rs`, which would hand the
password to the service rather than store it in the password manager; later,
Wi-Fi at startup (`userspace/wpa`, lane B) and backups to another computer
(`requests/e-db-the-backup-service-runs-backup-run-due.md`).
