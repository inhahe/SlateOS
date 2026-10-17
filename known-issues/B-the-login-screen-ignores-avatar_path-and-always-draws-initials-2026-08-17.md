## [B] The login screen ignores `avatar_path` and always draws initials (2026-08-17)
**Status:** OPEN -- the fix is lane C's now (2026-10-16): `init/loginmgr` stopped drawing when a111b5429 made it a service with no screen of its own, and the login screen is lane C's greeter (`gui/desktop`, a second `[[bin]]`), whose user tile draws a placeholder too. Lane C told.

**In short:** An account can name a picture to show next to it on the login
screen — the `avatar_path:` field in `/etc/users.yaml`, which `useradm mod
--avatar` sets. The login screen never looks at it. It draws a coloured circle
with the user's initials for every account, so setting an avatar appears to
work, reports success, and changes nothing anyone can see.

**Where:** lane C's login screen (`gui/desktop`'s greeter, and
`gui/desktop/src/login_screen.rs` before it), which draws every user tile
without looking at the account's avatar. (Until a111b5429 this pointed at
`init/loginmgr/src/main.rs`'s `UserAccount::avatar_path`, which that commit
deleted with the rest of loginmgr's drawing.) `loginusers::Account::avatar`
carries the field to the screen.

### Proper fix

Load the named image and draw it clipped to the circle, falling back to the
initials when the field is unset, the file is missing, or it does not decode.
The fallback is not optional: an avatar path can point at a file on a
filesystem that is not mounted yet at login time, and a login screen that
refuses to draw a user it cannot find a picture for is a login screen that
cannot log that user in.

The screen and an image decoder are both lane C's (`gui/`), so the change
is theirs.

**Severity:** cosmetic, but it is a silent no-op in a command that reports
success, which is the kind of thing that gets diagnosed as a broken file
rather than a missing feature.
