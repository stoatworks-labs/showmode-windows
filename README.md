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

> **Preview.** v0.1.0 is out for testing, and unsigned. It has been bench-tested on two Windows 11
> 25H2 machines (an x64 VM and an ARM64 Parallels guest): starting and ending a show, a crash
> mid-show and the next launch putting everything back, muting other apps measured by recording
> the output, and the display lock with two virtual screens. It hasn't done a show, and the
> wallpaper blackout hasn't been run yet, so try it on your own show PC before a show depends on
> it. The guards added since (system sounds, the Windows key, the cursor fence, the taskbar
> corner, AutoPlay, accessibility shortcuts, Energy saver, f.lux, Syncthing and sync apps) are on
> main and come with the next release, bench-tested the same way. The
> [user guide](docs/USER-GUIDE.md) says exactly what has and hasn't been checked.

<!-- downloads:start -->

## Download

**[v0.1.0](https://github.com/stoatworks-labs/showmode-windows/releases/tag/v0.1.0)** — prebuilt for Windows. Pick your platform:

<details>
<summary><b>Windows</b> — x64, ARM64</summary>

| Build | Download | Size |
| --- | --- | --- |
| x64 · .zip archive | [`ShowMode-0.1.0-windows-x64.zip`](https://github.com/stoatworks-labs/showmode-windows/releases/download/v0.1.0/ShowMode-0.1.0-windows-x64.zip) | 432 KB |
| ARM64 · .zip archive | [`ShowMode-0.1.0-windows-arm64.zip`](https://github.com/stoatworks-labs/showmode-windows/releases/download/v0.1.0/ShowMode-0.1.0-windows-arm64.zip) | 390 KB |

</details>

All builds, checksums and release notes: [github.com/stoatworks-labs/showmode-windows/releases](https://github.com/stoatworks-labs/showmode-windows/releases).

The Windows builds are unsigned, so SmartScreen warns once — see [SmartScreen and Smart App Control](#smartscreen-and-smart-app-control) for the one-time click-through.

<!-- downloads:end -->

## What it does

| Guard | How |
|---|---|
| Display & system sleep | a power request (`PowerSetRequest`, display and system), which dies with the process |
| Show power plan | a temporary copy of your active plan with the lid set to do nothing and no display, sleep or hibernate timeouts; your own plan is never edited, and the copy is deleted afterwards |
| Energy saver | its charge level set to 0 on that same copy, on battery and on mains, which turns Always use energy saver off too |
| Screen saver | `SPI_SETSCREENSAVEACTIVE`, only if it's on |
| Do Not Disturb | the shell's own QuietHoursSettings COM server, the one Settings uses (undocumented, looked up at runtime, so a Windows without it just reports "unavailable") |
| System sounds | the No Sounds scheme, written the way the Sound control panel writes it; each event's own sound is journalled and put back |
| App crash dialogs | Windows Error Reporting `DontShowUI` (per user); reports are still saved |
| AutoPlay | `DisableAutoplay`, the Settings switch, so plugging in a drive or phone asks nothing |
| Snap | `SPI_SETWINARRANGING`: windows stop snapping to edges, Snap layouts and drag-to-top maximising |
| Show Desktop corner, window shake | `TaskbarSd` and `DisallowShaking`, the Settings switches, then the broadcast Settings sends |
| Windows key | a low-level keyboard hook on a thread of its own swallows the Windows keys and anything pressed with them, Ctrl+Esc and Print Screen; it writes nothing and goes with the process |
| Accessibility shortcuts | the shortcut flags for Sticky Keys, Filter Keys, Toggle Keys and Mouse Keys (`SystemParametersInfo`), never the features themselves |
| Cursor fence | a low-level mouse hook stops the cursor at the edge of the screens ticked under **Keep Cursor Off**, backed by a 10 ms check; it stands down rather than leave the cursor nowhere to go, and Ctrl+Alt+Shift+F10 pauses it |
| Desktop wallpaper | black on every screen through `IDesktopWallpaper`, including screens plugged in mid-show; pictures, position, colour and slideshow are journalled and put back. With Windows Spotlight as the background it's left alone, since Spotlight can't be put back exactly |
| Display lock | the display that was main at the start stays main, and a screen that starts mirroring mid-show is split back out to extended, through the CCD API (`SetDisplayConfig`). Every correction is temporary, never saved to Windows' display database; a layout something keeps reverting is let go after four corrections inside twenty seconds |
| f.lux and LightBulb | asked to quit (WM_CLOSE, never killed) and opened again afterwards |
| Other apps' sound *(off by default)* | every app not ticked under **Allowed to Play** is muted, session by session, on every output, including sound that's already playing. Windows remembers a per-app mute even after the app that set it has died, so a watchdog process unmutes everything the moment Show Mode does |
| Syncthing | its local REST API pauses the folders that are syncing and resumes only those |
| Sync apps *(off by default)* | OneDrive (its own `/shutdown`), Dropbox, Google Drive, Box Drive, MEGA and Resilio Sync asked to quit and opened again afterwards |

Before changing anything it writes down what was there, in `%LOCALAPPDATA%\ShowMode\journal.json`.
Ending the show puts back those values and nothing else. Quitting does the same, and if the app
crashes or the PC restarts mid-show, the next launch finds the journal and finishes the job.
Nothing in v0.1 needs admin.

**Pre-show checks** list what Show Mode can't switch off, only point out: above all a waiting
Windows restart (Windows Update restarts on its own deadline, and no app can refuse it), then
battery, Energy saver, Night light, the taskbar on every display, Windows minimising windows when
a monitor drops out, a Public network (which hides NDI and Dante), a Remote Desktop session, and
remote-access, backup and sync tools that are running. They only read.

Not on Windows yet: pausing Windows Update and Search indexing, which need admin (Show Mode
doesn't ask for it), and the Mac's fan guards.

## Installing

Download the zip for your PC from the
[releases page](https://github.com/stoatworks-labs/showmode-windows/releases): x64 for nearly
every PC, ARM64 for a Snapdragon laptop or Windows on a Mac. Unzip it somewhere it can stay, such
as `%LOCALAPPDATA%\Programs\ShowMode`, and run `ShowMode.exe`. There's no installer. Settings and
the journal live in `%LOCALAPPDATA%\ShowMode`, and the `showmode:` handler follows the program
wherever it's started from. Launch at Login remembers where it was when you ticked it, so tick it
again after moving it.

### SmartScreen and Smart App Control

The builds aren't code-signed yet, so the first time you run one, SmartScreen says "Windows
protected your PC". Click **More info**, then **Run anyway**, and it won't ask again for that
copy. With Smart App Control switched on, Windows blocks an unsigned app it doesn't know and
offers no Run anyway, so Show Mode won't start on that PC until it's signed. I haven't had a PC
with it on to try.

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

<!-- attributions:start -->
This project is built on other people's work — see [ATTRIBUTIONS.md](ATTRIBUTIONS.md).
<!-- attributions:end -->

## Licence

MIT. See [LICENSE](LICENSE).
