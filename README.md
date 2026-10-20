# Show Mode for Windows

> **AI-assisted project.** This codebase was created with [Claude](https://claude.com/claude-code)
> (Anthropic), directed and reviewed by a human author.

A notification-area app for the Windows PC that's running your slides or playback. It stops the
PC doing anything clever during the show (going to sleep, popping up notifications, putting your
holiday photos on the projector, letting Outlook chime through the PA), then puts every setting
back how you had it.

It's the Windows sibling of [Show Mode for the Mac](https://github.com/stoatworks-labs/showmode),
and a separate app: Windows needed different answers to nearly every question.

**Ctrl+Alt+Shift+F11** starts/ends show mode · or click the masks in the notification area

> **Not released.** v0.1 has been bench-tested on two Windows 11 25H2 machines (an x64 VM and an
> ARM64 Parallels guest): starting and ending a show, a crash mid-show and the next launch putting
> everything back, muting other apps measured by recording the output, and the display lock with
> two virtual screens. It hasn't done a show, and the wallpaper blackout hasn't been run yet. The
> [user guide](docs/USER-GUIDE.md) says exactly what has and hasn't been checked.

## What it does

| Guard | How |
|---|---|
| Display & system sleep | a power request (`PowerSetRequest`, display and system), which dies with the process |
| Show power plan | a temporary copy of your active plan with the lid set to do nothing and no display, sleep or hibernate timeouts; your own plan is never edited, and the copy is deleted afterwards |
| Screen saver | `SPI_SETSCREENSAVEACTIVE`, only if it's on |
| Do Not Disturb | the shell's own QuietHoursSettings COM server, the one Settings uses (undocumented, looked up at runtime, so a Windows without it just reports "unavailable") |
| Snap | `SPI_SETWINARRANGING`: windows stop snapping to edges, Snap layouts and drag-to-top maximising |
| App crash dialogs | Windows Error Reporting `DontShowUI` (per user); reports are still saved |
| Desktop wallpaper | black on every screen through `IDesktopWallpaper`, including screens plugged in mid-show; pictures, position, colour and slideshow are journalled and put back. With Windows Spotlight as the background it's left alone, since Spotlight can't be put back exactly |
| Display lock | the display that was main at the start stays main, and a screen that starts mirroring mid-show is split back out to extended, through the CCD API (`SetDisplayConfig`). Every correction is temporary, never saved to Windows' display database; a layout something keeps reverting is let go after four corrections inside twenty seconds |
| Other apps' sound *(off by default)* | every app not ticked under **Allowed to Play** is muted, session by session, on every output, including sound that's already playing. Windows remembers a per-app mute even after the app that set it has died, so a watchdog process unmutes everything the moment Show Mode does |

Before changing anything it writes down what was there, in `%LOCALAPPDATA%\ShowMode\journal.json`.
Ending the show puts back those values and nothing else. Quitting does the same, and if the app
crashes or the PC restarts mid-show, the next launch finds the journal and finishes the job.
Nothing in v0.1 needs admin.

**Pre-show checks** list what Show Mode can't switch off, only point out: above all a waiting
Windows restart (Windows Update restarts on its own deadline, and no app can refuse it), then
battery, Energy saver, Night light, the taskbar on every display, Windows minimising windows when
a monitor drops out, a Public network (which hides NDI and Dante) and a Remote Desktop session.
They only read.

Not in v0.1, and on the list: keeping Windows-key shortcuts and the cursor off the audience's
screens, accessibility hotkeys, and the background-work guards.

## Driving it from somewhere else

```
ShowMode.exe showmode://on
```

`showmode://off` and `showmode://toggle` too. The scheme is registered for your user at every
launch, so Companion (with its shell action on the same PC), a show-control script or a
terminal can drive it. A copy started with a command hands it to the running app and leaves.

`ShowMode.exe --status` prints what it can see without changing anything.

## Building

From a Mac (or Linux) with [cargo-xwin](https://github.com/rust-cross/cargo-xwin):

```bash
scripts/build.sh
```

That writes x64 and ARM64 executables to `dist/`. On Windows, plain `cargo build --release`
does it. `swift scripts/make-icons.swift` redraws the icons (it needs a Mac).

## Licence

MIT. See [LICENSE](LICENSE).
