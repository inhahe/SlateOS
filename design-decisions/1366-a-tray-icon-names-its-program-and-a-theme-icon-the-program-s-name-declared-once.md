## 1366. A tray icon names its program and a theme icon; the program's name is declared once

**Date:** 2026-10-05
**Lane:** F
**Decided by:** Claude (autonomous), within the shape lane C proposed in
`requests/c-f-name-a-tray-icons-program.md` and
`requests/c-f-let-a-tray-icon-name-a-theme-icon.md`.

**In short:** a program's icon in the taskbar's tray could only be a single
character, and the pictures a tray most wants -- a battery, a speaker, a
network -- are emoji no font here can draw, so they showed as boxes. And the
shell had no lasting name for the program an icon came from, so it could not
remember where a user had put that program's icon. A tray icon now carries
the program's name and, optionally, the name of a picture in the icon theme
(`battery-caution`), which the shell draws in place of the character.

### What was decided, and the alternative each time

| Question | Decided | Alternative | Why |
|---|---|---|---|
| How the icon name is held | `guiremote::tray::IconName`, a type that cannot hold anything but 1-64 bytes of `a-z 0-9 - _` not starting with `-` | `Option<String>`, as the request sketched, checked by the shell | A string checked later is a path until someone checks it; a type checked when it is built cannot reach a theme folder as `../x`. A name off the wire that is not one fails the frame (`DecodeError::BadIconName`), as any malformed field does. |
| Where the program's name comes from | Each icon carries it (`TraySpec::app_id`), and `oswindow`'s loop fills it in from the name the program declared (`EventLoop::set_app_id`; `app::open` declares `App::app_id`) | One per connection, sent once; or the compositor copying the app id of the client's windows | A program in the tray may have no window to copy from, and a separate declaration is one more request to order against the first icon. Per icon costs a few bytes and cannot arrive late. |
| What the declared name also does | A window that names no program takes it too | Windows keep their own default (empty) | One declaration for the program everywhere the shell keys on one; a program that says nothing still sends empty names, as before. |
| Text too long | Cut on a character boundary -- glyph 32 bytes, tooltip 1 KiB, program name 255 -- never refused | Refuse the request | One rule for every text an icon carries. An icon shown with a cut name is better for the user than an icon not shown; the bounds are generous enough that no honest program meets them. |

The wire: `TRAY` frame version 2 and control version 25, each icon's
program name and icon name after its tooltip; the icon name as a one-byte
length (0 for none) and its bytes, as a settings name already travels.
`guiremote` cannot link `appearance` (every program links `guiremote`), so
it holds its own copy of the theme's name rule, and a compositor test,
`the_wires_icon_names_are_the_ones_a_theme_can_hold`, holds the two
together.

### Revisit if

A program needs a picture that is not in any theme -- its own logo, say. Then
the tray needs pixels or a path into the program's own icon folder, and the
choice between them is a new question.
