# The program lists, inventoried before they become one

`design-decisions.md` §1425, the operator's answer to C-Q20: one list of the
installed programs, in a userspace library (this crate), and -- first -- "collect
all of the built-in apps, categories, MIME types, apps, file types, per-role
defaults, etc. and make sure they survive in the new place or wherever they
belong."

**In short:** fourteen places held some part of "which programs exist, what they
open, and what opens by default" -- nine kernel modules, four userspace ones and
one module already deleted. None could read another's. Every item any of them
held is listed below with where it now lives, or why it does not. The tests in
this crate (`tests/inventory.rs`) hold the "where it now lives" column to the
code, so an item cannot quietly go missing later.

Taken 2026-09-27 at `lane-c-wip` 154c9b1fd (main `62819f49c`).

## The sources

| # | Where | What it held | Who read it |
|---|---|---|---|
| K1 | kernel `fs::appregistry` | 9 programs (id, name, description, path, icon, categories, MIME types, keywords); 14 categories | the kernel shell's `appreg`; `fs::startmenu` |
| K2 | kernel `fs::defaultapps` | 14 roles, each with the MIME types and schemes that define it; 19 type defaults; 10 role defaults | the kernel shell |
| K3 | kernel `fs::associations` | 48 type defaults naming 9 programs, with priorities | the kernel shell, `fs::openwith` |
| K4 | kernel `fs::mime` | 123 extensions to MIME types; 36 content signatures; a type-to-kind function | the kernel shell, `fs::associations`, `fs::preview`, `fs::properties` |
| K5 | kernel `fs::filetype` | 30 types with a description, an icon name and extensions | the kernel shell, `fs::columnview` |
| K6 | kernel `fs::pinnedapps` | 4 default taskbar pins | the kernel shell |
| K7 | kernel `fs::startmenu` | 5 default favourites, 2 quick links | the kernel shell, `/proc/startmenu` |
| K8 | kernel `fs::applaunch` | 5 launcher entries with search words | the kernel shell |
| K9 | kernel `fs::openwith`, `fs::appstore`, `fs::contextmenu` | no built-in programs or types (an API, an empty catalogue, and menu actions) | -- |
| U1 | `gui/desktop` `launcher::builtin_app_database` | 10 programs (name, description, path, icon, start-menu folder, keywords) | the start menu, pins, the Run box |
| U2 | `apps/fileassoc` `add_default_apps` and `assign_default_associations` | 8 programs with the extensions each opens; 7 group defaults; 3 per-extension overrides | the File Associations program |
| U3 | `gui/desktop/src/default_apps.rs`, deleted in 57300b5b1 | 10 programs (id, name, description, path, icon, roles, extensions, MIME types); 12 roles | nothing (a panel no one could open) |
| U4 | `guitk::filetypes` | 114 extensions (MIME type, kind, description); 24 content signatures; 16 kinds | the file manager, thumbnails, File Associations, `gui/associations` |
| U5 | `gui/associations` | 3 groups (Music, Video, Images), each a view over a toolkit kind | File Associations, Settings, the file manager |

## 1. Programs

Every program any source named, by the program it really is. A path in the
second column that no crate builds is marked *no such program*.

| Program (this crate) | Also known as | From |
|---|---|---|
| **File Explorer** `/usr/bin/explorer` (`apps/explorer`) | "File Manager" `/usr/bin/file-manager` *no such program*, id `org.os.files`; "Files" `/usr/bin/files` *no such program* (K6, K8); "File Explorer" `org.slateos.explorer` | K1, K6, K7, K8, U1, U2, U3 |
| **Terminal** `/usr/bin/terminal` (`apps/terminal`) | `org.os.terminal`, `org.slateos.terminal` | K1, K6, K7, K8, U1, U3 |
| **Text Editor** `/usr/bin/editor` (`apps/editor`) | `/usr/bin/text-editor` *no such program* (K1, id `org.os.editor`); `/usr/bin/edit` *no such program* (K3); `org.slateos.editor` | K1, K3, K7, K8, U1, U2, U3 |
| **Settings** `/usr/bin/settings` (`apps/settings`) | `org.os.settings` | K1, K6, K7, K8, U1 |
| **Calculator** `/usr/bin/calculator` (`apps/calculator`) | `org.os.calculator`, `org.slateos.calculator` | K1, K7, U1, U3 |
| **System Information** `/usr/bin/sysinfo-app` (`apps/sysinfo`, whose package and binary are `sysinfo-app`) | "System Info" `/usr/bin/sysinfo` (U1) -- **the wrong program**: that is `userspace/sysinfo`, the command-line tool, so the start menu's row started a program with no window; `/usr/bin/system-info` *no such program*, id `org.os.sysinfo` (K1) | K1, U1 |
| **Process Explorer** `/usr/bin/procexplorer` (`apps/procexplorer`) | `/usr/bin/process-explorer` *no such program*, id `org.os.procexp` | K1, U1 |
| **Image Viewer** `/usr/bin/imageviewer` (`apps/imageviewer`) | `/usr/bin/image-viewer` *no such program*, id `org.os.viewer`; "Photo Viewer" `/usr/bin/viewer` *no such program*, id `org.slateos.viewer` (U3, K3) | K1, K3, U1, U2, U3 |
| **Music Player** `/usr/bin/musicplayer` (`apps/musicplayer`) | half of "Media Player" `/usr/bin/media-player` *no such program* (K1, `org.os.player`) and `/usr/bin/player` *no such program* (K3); `org.slateos.musicplayer` | K1, K3, U1, U2, U3 |
| **Video Player** `/usr/bin/videoplayer` (`apps/videoplayer`) | the other half of "Media Player" (K1, K3); `org.slateos.videoplayer` | K1, K3, U2, U3 |
| **Screenshot** `/usr/bin/screenshot` (`apps/screenshot`) | -- | U1 |
| **PDF Viewer** `/usr/bin/pdfviewer` (`apps/pdfviewer`) | `/usr/bin/pdfview` *no such program* (K3); "Document reader" `org.slateos.pdfviewer` | K3, U2, U3 |
| **Archive Manager** `/usr/bin/archivemanager` (`apps/archivemanager`) | `/usr/bin/archiver` *no such program* (K3); `archivemgr` (K2); `org.slateos.archivemanager` | K2, K3, U2, U3 |
| **Hex Editor** `/usr/bin/hexeditor` (`apps/hexeditor`) | -- | U2 |
| **Calendar** `/usr/bin/calendar` (`apps/calendar`) | `org.slateos.calendar` | U3 |

**Where they go:** each is a desktop entry, `gui/programs/applications/<id>.desktop`,
in the format the start menu already reads for installed programs
(`gui/desktopentry`). The id is `org.slateos.<Program>`. Each entry carries the
union of what the sources said about it -- every name a search should find it by,
every type it opens -- in the freedesktop vocabulary, so a program that later
ships its own entry replaces the built-in one field for field.

**Named, and no program:**

| Named | By | Why it is not carried |
|---|---|---|
| "Web Browser" `/usr/bin/browser` | K3 (for `text/html`), K6 (a pin), K8 | SlateOS has no browser. The *role* is carried (section 3); a pin or a default naming a program that does not exist would be a button that does nothing. |
| "Email Client" `emailclient` | K2 | as above: the role is carried, no program is invented |
| "Program Launcher" `/usr/bin/run` | K3 (for SlateOS executables) | an executable is started, not opened with something; the file manager starts it itself |
| "Library Inspector" `/usr/bin/ldd`, "Archive Tool" `/usr/bin/ar` | K3 (for SlateOS shared and static libraries) | `userspace/ldd` and `userspace/ar` are command-line tools; opening a file from the file manager starts a window, and these have none. Recorded here so a future "inspect library" action can use them. |

## 2. What each program is found by

The union carried into each desktop entry. Icon names are the icon theme's
(freedesktop); the kernel's `icon-files`-style names named no picture in any
theme and are not carried.

| Program | Keywords (union) | Menu categories (freedesktop) | Icon |
|---|---|---|---|
| File Explorer | explorer, finder, nautilus (K1); files, browse, folder, directory (U1) | Utility; System; FileManager | `system-file-manager` |
| Terminal | shell, console, command (K1); bash, cli (U1) | System; TerminalEmulator | `utilities-terminal` |
| Text Editor | notepad, edit, vim, nano (K1); code, write (U1); text (K8) | Utility; TextEditor | `accessories-text-editor` |
| Settings | preferences, config, control (K1); options, display, monitor, resolution, dpi, network, wifi, ethernet, vpn, internet, sound, audio, volume, speaker, microphone (U1) | Settings | `preferences-system` |
| Calculator | calc, math (K1); compute (U1) | Utility; Calculator | `accessories-calculator` |
| System Information | about, hardware, specs (K1); info (U1) | System | `computer` |
| Process Explorer | task, manager, top, htop (K1); processes, kill (U1) | System; Monitor | `utilities-system-monitor` |
| Image Viewer | photo, picture, gallery (K1, U1); png, jpg (U1) | Graphics; Viewer | `image-x-generic` |
| Music Player | music, vlc, mpv (K1); audio, song, mp3, media (U1) | AudioVideo; Audio; Player | `audio-x-generic` |
| Video Player | video, vlc, mpv (K1); media, movie | AudioVideo; Video; Player | `video-x-generic` |
| Screenshot | capture, snip, screen, grab (U1) | Utility | `applets-screenshooter` |
| PDF Viewer | document, reader, pdf | Office; Viewer | `x-office-document` |
| Archive Manager | zip, tar, compress, extract | Utility; Archiving; Compression | `package-x-generic` |
| Hex Editor | hex, binary, bytes | Development | `accessories-text-editor` |
| Calendar | events, reminders, schedule | Office; Calendar | `x-office-calendar` |

The last five rows' keywords are the sources' descriptions turned into words
("Open and create archives", "Calendar and event management"), because no
source gave them keywords of their own.

**The first category is the start-menu folder** (`desktopentry::menu::Folder::of`
takes the first main category), so it is chosen to keep each program in the
folder U1 put it in: File Explorer in Accessories, as the Aero reference has it,
though K1 filed it under System.

**How each is started** (`Exec=`) follows what the program accepts on its
command line today: `%F` (any number of files) for the text editor, the music
and video players, the PDF viewer and the hex editor; `%f` (one) for the file
explorer, the image viewer and the archive manager; nothing for the seven that
take no file.

**Descriptions** (U1's, else U3's, else K1's): each entry's `Comment`.

## 3. Roles and who fills them

A role is a job a program can be the default for. K2 and U3 each had a list.

| Role | Defined by (K2's types and schemes) | Default program | Sources |
|---|---|---|---|
| Web browser | `text/html`, `application/xhtml+xml`, `x-scheme-handler/http`, `x-scheme-handler/https` | none installed | K2, U3 |
| Email | `x-scheme-handler/mailto`, `message/rfc822` | none installed | K2, U3 |
| File manager | `inode/directory` (K2, U3; K1 wrote `application/x-directory`) | File Explorer | K2, U3 |
| Text editor | `text/plain` and the source types | Text Editor | K2, U3 |
| Terminal | the `TerminalEmulator` category (K2's `x-scheme-handler/terminal` is registered nowhere) | Terminal | K2, U3 |
| Image viewer | `image/*` | Image Viewer | K2, U3 |
| Video player | `video/*` | Video Player | K2, U3 |
| Music player | `audio/*` | Music Player | K2, U3 |
| Document reader | `application/pdf`, `application/epub+zip` (K2's "PDF viewer", U3's "Document reader") | PDF Viewer | K2, U3 |
| Archive manager | the archive types | Archive Manager | K2, U3 |
| Calculator | the `Calculator` category | Calculator | K2, U3 |
| Calendar | the `Calendar` category (K2 said `text/calendar`, but the calendar program does not open a file named on its command line, so no program opens that type yet) | Calendar | K2, U3 |
| Maps | `x-scheme-handler/geo` | none installed | K2 |
| System monitor | the `Monitor` category | Process Explorer | K2 |

**Where they go:** `programs::Role`, each defined by the types or the category
above, and filled by the program whose entry claims them. A role with no program
is kept and answers "none": the Settings page can say "No web browser is
installed" rather than hide the question.

## 4. What opens what, by default

Before a person chooses (`gui/associations` holds their choices), a type opens
with its built-in default. The sources' defaults, reconciled to the real
programs, **and only where the program can open the file it is handed** -- a
default that starts a program without the file, or one that cannot read it, is
a double-click that does nothing useful:

| Types | Opens with | From |
|---|---|---|
| `text/plain` and the text and source types: `text/x-rust`, `text/x-c`, `text/x-c++`, `text/x-python`, `text/javascript`, `text/typescript`, `text/css`, `text/markdown`, `application/x-shellscript` (K3 spelled it `text/x-shellscript`), `text/csv`, `application/json`, `application/toml`, `application/x-yaml`, `application/xml`, `application/sql`, `application/rtf` | Text Editor | K2, K3, U2 (Documents, Code and Other groups), U3 |
| `text/html` | Text Editor, until a browser is installed (K2 and K3 named the absent browser) | K2, K3, U2 |
| `image/png`, `image/jpeg`, `image/gif`, `image/bmp`, `image/webp`, `image/tiff`, `image/svg+xml`, `image/x-icon` | Image Viewer | K1, K2, K3, U2, U3 |
| `audio/mpeg`, `audio/wav`, `audio/ogg`, `audio/flac`, `audio/mp4`, `audio/aac`, `audio/opus` | Music Player | K2, K3 (as "Media Player"), U2, U3 |
| `video/mp4`, `video/x-matroska`, `video/webm`, `video/x-msvideo`, `video/quicktime`, `video/x-ms-wmv`, `video/x-flv` | Video Player | K2, K3 (as "Media Player"), U2, U3 |
| `application/zip`, `application/gzip`, `application/x-bzip2`, `application/x-xz`, `application/zstd`, `application/x-7z-compressed`, `application/vnd.rar` (K3's `application/x-rar-compressed`), `application/x-tar` | Archive Manager | K2, K3, U2, U3 |
| `application/pdf` | PDF Viewer | K2, K3, U2 (override), U3 |
| `inode/directory` | File Explorer | K2, U3 |

Fifty types in all. **Named by a source, and left without a default:**

| Type | Named by | Why no default |
|---|---|---|
| `application/octet-stream` (`.bin`) | U2 (Hex Editor) | it is also the type of every file nothing recognises, so a default would open all of them in the hex editor. The hex editor's entry lists it, so Open With offers it for any file. |
| `application/x-iso9660-image` | U2 (File Explorer) | the file explorer, handed a file, opens the folder that holds it; nothing reads the image's contents |
| `application/epub+zip` | K2, U3 (the document reader) | the PDF viewer does not read EPUB, and the e-book reader reads plain text |
| `text/calendar` | U3 (Calendar) | the calendar imports `.ics` from its own window but does not open one named on its command line (lane E) |
| the office documents (`.doc`, `.docx`, `.odt`, `.xls`, `.xlsx`, `.ppt`, ...) | U2 (Documents group: Text Editor) | they are zip archives or binary files; the text editor would show their bytes |
| `x-scheme-handler/http`, `https`, `mailto`, `geo`, `message/rfc822` | K2, K3 | no browser, mail program or map is installed; the roles remain (section 3) |

**Where they go:** each program's entry lists the types it opens (`MimeType=`),
and `gui/programs/defaults.list` names the default for each type, in the
freedesktop `mimeapps.list` format, so the same file can later be installed as
the system's `/usr/share/applications/mimeapps.list`.

## 5. File types

**Extensions:** U4 (the toolkit's table, `guitk::filetypes`) is where file types
belong, and it lacked 33 that K4 and K5 knew. **Twenty-three are carried**,
each with K4's type:

`a`, `bat`, `cc`, `cmd`, `cpio`, `cxx`, `diff`, `epub`, `gzip`, `htm`, `hxx`,
`jar`, `lib`, `markdown`, `mjs`, `o`, `patch`, `psm1`, `pyw`, `text`, `xsd`,
`xsl`, `zstd`.

**One, `oga` (Ogg audio), is held back** only because `apps/fileassoc`'s test of
applying a program to the Music group counts the toolkit's audio types by hand
("5 of 10"), and an eleventh would fail it; lane E is asked to derive the count
from the table (`requests/c-e-a-test-that-counts-the-toolkits-audio-types.md`),
and `oga` goes in when that lands.

**Nine wait on a decision the toolkit had already recorded** (`known-issues.md`,
the `filesearch` entry), and stay out, pinned by the toolkit's
`the_kernels_types_this_table_waits_to_decide_are_still_absent`: `exe`, `dll`,
`class`, `wasm`, `deb`, `rpm` -- programs and installers of other systems, which
this one cannot run; the toolkit's `foreign_executables_are_absent_on_purpose`
says to decide that first -- and `db`, `sqlite`, `sqlite3`, which need a
"database" kind the category enum does not have.

**Content signatures:** K4 recognised 20 kinds of file by their first bytes
that the toolkit did not. **Nine kinds are carried** into the toolkit's
signature table: ELF executables, TIFF (both byte orders), Zstandard, LZ4,
POSIX tar (`ustar` at 257), cpio (its three ASCII forms), `ar` archives
(static libraries), MIDI, and AVI. Not carried:

| Kind | Why |
|---|---|
| Windows executables (`MZ`), SQLite, WebAssembly, Debian packages | the extensions they identify wait on the decision above; a signature for a type the table does not hold would answer "unknown" |
| WebM | its signature is Matroska's (EBML); the two differ only in a document type deeper in the file than a fixed-offset pattern can see, and the toolkit already reads it as Matroska |
| Windows icons (`00 00 01 00`) | four bytes, three of them zero, match far too much to name a file by; `.ico` still identifies an icon |
| K4's five guesses from text (JSON, XML, HTML, plain text, `#!` scripts) | the toolkit's `is_text_file` makes the text-or-binary call, and "HTML" or "JSON" sniffed from a file's first bytes is wrong often enough that the extension should win |

**Two defects in the toolkit's own table were found and fixed on the way:** a
bare `RIFF` signature stood ahead of `WEBP` and answered WAV, so every WebP
picture and every AVI video sniffed as audio (RIFF files are now told apart by
their form type); and the ELF signature answered "unknown" though the table had
held `.elf` as one of this system's own executables since 2026-09-16. The
toolkit's extension enum (`FileExtension`, `parse_extension`), which lagged the
table by a dozen rows and which nothing outside the file used, is gone: the
signature table now names table rows directly.

**Where K4 and U4 disagreed on a type's name**, the toolkit's spelling is the one
the shared-mime-info database and IANA use, and it stays:

| Extension | K4 | Kept (U4) |
|---|---|---|
| aac | audio/mp4 | audio/aac |
| sh, bash | text/x-shellscript | application/x-shellscript |
| elf | application/x-elf | application/x-executable |
| java | text/x-java-source | text/x-java |
| m4v | video/mp4 | video/x-m4v |
| ps1 | text/x-powershell | application/x-powershell |
| rar | application/x-rar-compressed | application/vnd.rar |
| nx, dso, slib | application/x-nx-executable, -sharedlib, -staticlib | application/x-slateos-executable, -shared-library, -static-library (the OS's own name) |

**K5's descriptions and icons:** superseded. Every K5 extension is in U4 with a
description of its own, and a file's picture comes from the icon theme by its
MIME type (`image-png`, then `image-x-generic`), not from K5's `icon-png`
names, which no theme has.

## 6. Kinds, groups and categories

| Vocabulary | Where it stays | Note |
|---|---|---|
| K1's 14 program categories | freedesktop `Categories=` in each desktop entry | System, Office, Graphics, Development, Education, Science and Settings are the same words; Multimedia is `AudioVideo`, Internet `Network`, Games `Game`, Accessories `Utility`; Terminal and File Manager are the additional categories `TerminalEmulator` and `FileManager`; Other is no category |
| K4's `category()` (Text, Image, Audio, Video, Font, Archive, Document, Executable, ...) | U4's 16 kinds, which cover it | Font is the one K4 had and U4 does not name; U4 files fonts under `Data`. Not carried: nothing reads a font kind. |
| U1's start-menu folders | derived from `Categories=` by `gui/desktopentry` | unchanged |
| U2's 7 sidebar groups | `apps/fileassoc` | a view over U4's kinds; unchanged |
| U4's 16 kinds | `guitk::filetypes` | unchanged |
| U5's 3 groups | `gui/associations` | unchanged |

## 7. Pins, favourites, quick links and launcher entries

K6, K7 and K8 held defaults for things the shell now owns -- its taskbar pins
(`taskbar.yaml`), its start menu pins (`startmenu.yaml`) and its search. The
shell has no defaults today: a machine with neither file starts with nothing
pinned. The kernel's, minus the browser that does not exist, become the shell's
first-start pins:

| Kernel default | Programs | Becomes |
|---|---|---|
| K6 taskbar pins | Files, Browser, Terminal, Settings | the taskbar's first-start pins: File Explorer, Terminal, Settings |
| K7 favourites | Files, Terminal, Editor, Settings, Calculator | the start menu's first-start pins, the same five |
| K7 quick links | Settings, Terminal | the start menu's Settings and Terminal buttons, which the shell already has |
| K8 launcher entries | Files, Terminal, Browser, Settings, Text Editor, with search words | their words are in section 2's keywords |

## 8. Deliberately not carried

| Item | Source | Why |
|---|---|---|
| Nine invented paths (`/usr/bin/file-manager`, `text-editor`, `system-info`, `process-explorer`, `image-viewer`, `media-player`, `edit`, `viewer`, `player`, `archiver`, `pdfview`, `files`) | K1, K3, K6, K8, U3 | each is mapped to the real program in section 1; the path itself named nothing |
| Priorities 5 and 10 on K3's defaults | K3 | one default per type is what `defaults.list` states; a priority between a program and a program that does not exist decides nothing |
| `version: "1.0"` and install times | K1 | invented at registration, true of nothing |
| `show_in_menu`, `tray_icon`, `start_hidden` | K1 | the same for every program (true, false, false); `NoDisplay=` exists in the entry format for the first if a program ever needs it |
| `confirm_changes`, `reset_on_update` | U3 | preferences of a panel that no one could open |
| K5's `icon-*` names; K4's five text guesses | K4, K5 | section 5 |
| K2's `x-scheme-handler/terminal` | K2 | registered nowhere; the Terminal role is its category (section 3) |
