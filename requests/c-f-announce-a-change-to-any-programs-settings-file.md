# C -> F -- announcing a change to any program's settings file, not only the four the compositor knows

**From:** Lane C. **To:** Lane F (`gui/remote`, `gui/compositor`, `gui/window`).
**Filed:** 2026-09-27. **Status:** OPEN -- a protocol change lane C cannot land
alone; lane C's watcher is built on it.

**In short:** the operator decided (C-Q26, `design-decisions.md` §1418) that
each program keeps its settings in a file of its own, and that a service tells
every open window when a settings file changes, so a change shows at once --
beside the saving, never in front of it. The compositor already relays exactly
this for four files (`SettingsChanged { group }`: appearance, input,
notifications, session). What is missing is the same announcement for any other
program's file (`$XDG_CONFIG_HOME/slateos/<name>.yaml`, written through
`gui/settingsfile`), and something that notices a change without the saving
program having to announce it.

## What is asked of lane F

1. **A `SettingsGroup` for a program's own file.** Proposed shape, which keeps
   the type `Copy` (the compositor passes it by value) and keeps names off the
   wire unless validated:

   ```rust
   pub enum SettingsGroup {
       Appearance, Notifications, Input, Session,
       /// Another program's settings file: `<name>.yaml` under the settings folder.
       Program(SettingsName),
   }

   /// A settings file's name: 1..=32 bytes of `[a-z0-9_-]` -- the shape of
   /// every name in use today (the longest, `markdowneditor`, is 14). Inline,
   /// so `Copy`; `SettingsName::new(&[u8]) -> Option<Self>` is the one
   /// validation, used by the decoder.
   pub struct SettingsName { len: u8, bytes: [u8; 32] }
   ```

   `SettingsGroup` lives in `guitk::event` (lane C's), but the codec that
   matches on it exhaustively is yours (`gui/remote/src/input.rs`, the
   `GROUP_*` codes), so the two cannot land apart without one of our builds
   going red. **Lane C's suggestion:** you make the whole change -- the variant
   and `SettingsName` in `guitk::event` as above, the codec (a new group code
   followed by the name's length and bytes, refused on decode unless
   `SettingsName::new` accepts it), and the relay -- in one commit, and lane C
   treats that one addition to `guitk/src/event.rs` as agreed by this request.
   If you would rather lane C land the type first behind a codec change of yours
   that tolerates it, say which order and lane C follows it.

2. **A request that announces it:** `RequestBody::AnnounceSettings { name:
   SettingsName }`, relayed as `SettingsChanged { group: Program(name) }` to
   every window, on the same terms as `ReloadNotifications` -- announcing, not
   adopting; `Ok` whatever the file says, so the answer leaks nothing. For the
   four names the compositor already knows (`appearance`, `input`,
   `notifications`, `session`) it should do what the existing `Reload*` request
   does, so a sender need not know which is which.

3. **Nothing new in `gui/window`**, as far as lane C can see: `SettingsChanged`
   already reaches `App::event`. Say if it needs routing.

## What lane C does once it is in

A small service, `gui/settingswatch`: it watches the settings folder with
`inotify` (which the kernel and `posix` provide), and on a file being written or
renamed into place sends `AnnounceSettings` for it. So a program saving its
settings writes its own file and nothing more -- the operator's "not as the same
function that saves" -- and an edit by hand, or by another program, is announced
too. The desktop starts it with the session. `gui/settingsfile` gains the helper
a program uses to recognise its own name in an announcement, and starts refusing
to write a name `SettingsName` would not accept -- today it takes any string --
so every file it writes is one that can be announced.

**An alternative lane C would equally accept:** the watch in the compositor
itself, which already relays and already lives for the whole session -- one
process fewer. If you prefer that, the service's part is yours and lane C only
adds the `settingsfile` helper; say so.

## If this is never done

Settings still save and still take effect the next time a program starts; only
an open window does not see a change made elsewhere until it is reopened. The
four groups the compositor knows keep working as they do now.
