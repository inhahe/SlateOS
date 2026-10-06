## 1489. A moving background is a program the desktop starts, and a video is one such program

**Date:** 2026-10-06 &middot; **Decided by:** Claude (autonomous) &middot; **Lane:** C

**In short:** `design.txt` asks for a desktop background that is "not just
a video, but a program constantly changing it, with various input events
such as anything that's happening on the desktop". Both are built as one
thing. The desktop starts a *background program*, shows each picture the
program writes as the wallpaper -- placed by the user's fit, like a picture
file -- and tells it a few things about the desktop: the background's size,
when nobody can see it, the pointer over it, which virtual desktop shows,
where the windows are, and the light or dark look. A video chosen as the
wallpaper is played by a small program of the desktop's own, `wallvideo`;
any other program that writes pictures the same way can be chosen as the
background (`wallpaper.program`). The program never learns a window's title
or anything typed.

**Where:** `gui/backdrop` (the pictures and the events: `write_frame`,
`read_frame`, `Event`), `gui/wallvideo` (the video player: `cover_size`,
`shrink`, `due`, `main.rs`), `gui/desktop/src/background_program.rs`
(`Program`, `Source`, `is_video`, `wallvideo_path`),
`gui/desktop/src/session.rs` (`start_background_program`,
`collect_background`, `sync_background`), `gui/appearance`
(`wallpaper_program`).

| Choice | Instead of | For | Against |
|---|---|---|---|
| **A separate process draws the background** | decoding the video in the desktop | A video is a file from anywhere and its decoders are large (AV1, VP9, VP8 ports); a fault costs the background, not the desktop that holds the session. A program somebody else wrote is in its own process by nature. | Each picture crosses two pipes (program to desktop, desktop to compositor): 8 MB a frame at 1080p. |
| **One mechanism for a video and for a program** | a video wallpaper and a separate plug-in API | `design.txt` names them together; a video is just the background program that reads a file. A script can be a background. | A program must write the picture format and read the event lines. |
| **The program paces itself; the desktop shows the newest picture read** | the desktop asking for frames on a clock | A film knows its frame times, a clock-driven background its own; the desktop needs no timer and never queues stale pictures. | A program that writes faster than the screen wastes its own time -- the desktop drops all but the newest. |
| **A frame is sent no larger than needed to cover the background** (`wallvideo::cover_size`, a box-filter `shrink`) | sending the film's own size | A 4K film on a 1080p screen would move four times the bytes the screen can show, twice. | A user who chooses "fit" on a film far wider than the screen gets it scaled once more by the compositor. |
| **Told only where windows are, never what they are** | the window list as the shell has it | A background is something the user installed to look at; titles and programs are not its business. Rectangles are enough to move out of the windows' way. | A background cannot react to which program is in front. |
| **Paused behind the login screen, under a maximised window on the desktop shown, and after its first picture with motion turned off** | always running | Nobody sees it; the accessibility switch for less motion is honoured as a picture viewer honours it. A maximised window leaves only the taskbar's strip, whose glass shows the background frozen. | A background that wanted to keep a clock running behind the login screen cannot. |
| **The video player is beside the desktop's own program** (`current_exe`'s directory) | a path in the settings | Installed together (`/usr/bin`), built together (`target/...`); nothing to configure. | Moving one without the other loses video backgrounds -- said on screen when it happens. |
| **A program that stops says why and is not restarted** | starting it again | As a picture that will not open is not read again at every repaint: a crashing background would otherwise spin. | Choosing the same wallpaper again is needed after fixing it. |

### What it does not do yet

- **Choosing it in Settings**: the Wallpaper page's file chooser filters for
  pictures; a video, and a program as the background, are lane E's to offer
  (`requests/c-e-a-video-or-a-program-as-the-wallpaper.md`). Until then both
  are set in `appearance.yaml` (`wallpaper:` naming a video,
  `wallpaper.program:`).
- **Pausing on battery**, and under windows that cover the background
  without being maximised: the login screen, a maximised window and reduced
  motion are the pauses today.
- **Sound** from a video background: none, by design -- a wallpaper is seen,
  not heard.
