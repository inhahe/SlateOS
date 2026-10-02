## [B] The login screen ignores `avatar_path` and always draws initials (2026-08-17)
**Status:** OPEN — re-verified 2026-09-24: the user tile still draws only the initials circle. Worth less than it looks: nothing launches `init/loginmgr` today (no service file, rootfs entry or kernel spawn names it); the login screen a user actually meets is `gui/desktop/src/login_screen.rs` (lane C), which draws a placeholder glyph instead of the picture too.

**In short:** An account can name a picture to show next to it on the login
screen — the `avatar_path:` field in `/etc/users.yaml`, which `useradm mod
--avatar` sets. The login screen never looks at it. It draws a coloured circle
with the user's initials for every account, so setting an avatar appears to
work, reports success, and changes nothing anyone can see.

`init/loginmgr/src/main.rs`: `UserAccount::avatar_path` carries an
`#[allow(dead_code)]` precisely because no drawing code calls it; the avatar is
rendered by the initials-and-circle path in the user-tile drawing code, with no
branch on whether a path is set.

### Proper fix

Load the named image and draw it clipped to the circle, falling back to the
initials when the field is unset, the file is missing, or it does not decode.
The fallback is not optional: an avatar path can point at a file on a
filesystem that is not mounted yet at login time, and a login screen that
refuses to draw a user it cannot find a picture for is a login screen that
cannot log that user in.

Needs an image decoder reachable from `init/` — lane C owns `gui/`, so if the
decoder lives there this becomes a request rather than a local change. Check
what `gui/toolkit` exposes before assuming.

**Severity:** cosmetic, but it is a silent no-op in a command that reports
success, which is the kind of thing that gets diagnosed as a broken file
rather than a missing feature.
