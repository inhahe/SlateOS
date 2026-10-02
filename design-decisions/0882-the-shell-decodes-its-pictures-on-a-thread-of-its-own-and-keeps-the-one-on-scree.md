## 882. The shell decodes its pictures on a thread of its own, and keeps the one on screen until the next is ready

**Date:** 2026-09-26 &middot; **Decided by:** Claude (autonomous) &middot; **Lane:** C

**In short:** The desktop read and decoded its wallpaper, and the login
screen's picture, on the same thread that draws the desktop and answers its
clicks. A photograph takes about a second to decode, so at login, whenever a
wallpaper was chosen and at every step of a slideshow, the whole desktop froze
for that second. A thread of the shell's own now does the decoding and wakes
the desktop when a picture is ready. The picture already on screen stays there
until its replacement is ready, rather than the desktop going blank between
the two.

### The calls

| Question | Chosen | The alternative | Why |
|---|---|---|---|
| Where the decode runs | one worker thread per session (`gui/desktop/src/pictures.rs`); the session collects what is ready at the start of each pump, and the worker wakes a parked loop through `EventLoop::waker` | a thread per picture; or decoding in pieces on the loop's thread | two pictures that change at the speed of a person need one thread; the decoders decode a whole picture in one call, so a piece-at-a-time decode is not on offer, and it would still stall. |
| Where the upload runs | on the loop's thread, as before | uploading from the worker | the connection is the loop's, and a picture has to be up before the frame that names it (§557, §861) -- an order only the loop can keep. |
| What is drawn while the next picture decodes | the picture already up, at its own size (`WallpaperManager::shown_picture`); the greeter likewise keeps its picture | the plain colour until the new one is ready; or the new id at once | a slideshow would flash the plain colour between every two slides; naming the new id before its pixels exist draws nothing, silently. |
| Several requests before the first is done | only the newest for each slot is decoded; the others are answered "superseded" at once, and the session drops any answer for a picture it no longer wants | decoding each in turn | trying five wallpapers in a row would cost five decodes and flash each one. Every request still gets exactly one answer, which is what lets a test wait for all it is owed. |
| An answer that arrives after the wallpaper or the greeter's style has moved on, with no repaint since | what is wanted is asked again first; the stale answer is dropped and the new request sent at once | upload it, and let the next paint replace it | putting it up would flash a picture the user has already replaced -- and on the greeter, one from a style they have left. Only the repaint path puts a picture on the greeter, so there is one place that decides what it shows. |
| A new picture that will not decode | the old picture is given back and the plain colour drawn, with the reason posted | keeping the old picture up | the setting names a picture that cannot be shown, and keeping the old one would say the change had worked. The same as before this change. |
| The greeter showing the desktop's picture | one decode shared between the two when they ask one after the other for an unchanged file (same path, length and modification time) | a decode per slot; or a cache kept by path | at login both ask for the same photograph at once, so this halves the work there. The shared copy is let go whenever the queue runs dry, so an idle desktop holds no extra picture, and a file edited in place is decoded again. |
| A thread that will not start, or no waker | the plain background (no thread), or collection on the loop's next pass (no waker) | failing the session | a desktop without its wallpaper is still a desktop. |

### What is not done here

- The two applications with the same stall (`apps/photomanager`,
  `apps/imageviewer`) are lane E's; `known-issues.md`
  `TD-C-DECODING-A-PHOTOGRAPH-BLOCKS-THE-FRAME-THAT-ASKED-FOR-IT` stays open
  for them.
- The upload is still on the loop's thread. For a 4K picture that is a copy of
  about 33 MB into the socket: milliseconds, not the second the decode was.
