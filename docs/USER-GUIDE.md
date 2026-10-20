# Show Mode for Windows user guide

Show Mode is a notification-area app that locks a Windows PC down for a show, then puts every
setting back how you had it. One click (or **Ctrl+Alt+Shift+F11**) stops the PC sleeping, stops
notifications landing on an output, blacks out the wallpaper so your desktop never appears on the
projector, and can mute every app that isn't the show.

> **Status (2026-10-20): not released. Bench-tested on two Windows 11 25H2 machines, not on a
> show.**
>
> **Checked on winlab** (Windows 11 Pro 25H2, build 26200, x64 VM, one display, run with the
> token a standard user gets): the app starting in the signed-in session, the tray icon in both
> states on a light taskbar, the menu (opened through the icon), one copy only, and
> `showmode://on` and `off` handed over from a second copy. Starting a show took the keep-awake
> request, switched to a copy of the power plan, turned Snap off, turned Do Not Disturb on
> (priority only, and Windows' own reading agreed), hid crash dialogs, and left the wallpaper alone
> because it was Windows Spotlight, saying so in the note. Ending it put every one back exactly as
> found: the original plan active and the copy deleted, Snap on, Do Not Disturb off, the crash
> setting gone. Killing the app outright mid-show left everything changed, as it should, and the
> next launch found the journal, put it all back and said so. Ctrl+Alt+Shift+F11 registered (the
> menu shows it), but nobody pressed it.
>
> **Checked on a Parallels guest** (Windows 11 25H2, ARM64), recording the output with test tones
> at −40 dBFS: with muting on, the allowed app's 1 kHz tone stayed at −40 dB while the 440 Hz app
> dropped below −120 dB, and so did a second 440 Hz app opened mid-show. Ending the show brought
> both back and the watchdog went with it. Killing Show Mode mid-show unmuted the running app at
> once. An app that had quit while muted came back muted when relaunched (Windows had kept the
> mute), and was unmuted the moment Show Mode was relaunched.
>
> **Fixed during that bench:** a command handed over by a showmode: URL couldn't reach Do Not
> Disturb or the wallpaper; Snap didn't come back on at the end (it did once that was fixed, and
> the restore now reads the setting back); Spotlight wasn't recognised on 25H2; and Show Mode took
> over a showmode: handler that belonged to something else.
>
> **Not checked yet:** turning off a screen saver that is actually on (winlab has none); the
> wallpaper blackout itself and putting a picture or slideshow back; pressing the hotkey; Launch at
> Login; Finish Restoring; a restart or sign-out mid-show; more than one display; Windows 11 24H2;
> and a real show.
>
> This codebase was created with AI assistance, directed and reviewed by a human author.

---

## Install

There's no release yet. To build it, see the README.

Show Mode needs Windows 11. It lives in the notification area as a pair of theatre masks:
outlined while show mode is off, filled while it's on. Windows may tuck it under the ^ arrow;
drag it onto the taskbar to keep it in view.

## Starting and ending a show

Click the masks and choose **Start Show Mode**, or press **Ctrl+Alt+Shift+F11** anywhere.
**End Show Mode** (or the same keys again) puts everything back.

If something couldn't be applied, a short note appears by the notification area for a few
seconds and stays listed in the menu, marked ⚠, until the show ends. It isn't a Windows
notification, since show mode may just have switched those off, and it appears on the main
display, not the projector.

While a show is running, an ordinary restart or sign-out stops at Windows' list of apps holding
it up, with Show Mode's reason. That's a courtesy for a slipped click, not a guard: a restart
forced by Windows Update doesn't ask. Check for a waiting update before doors.

## What it turns off

Untick any guard in **What Show Mode Disables**. A change there takes effect at the next start.

| Guard | What it does |
|---|---|
| Prevent display & system sleep | Holds a power request, the way a video player does. It goes with the app, so a crash can't leave a PC that never sleeps. Windows still ends it on a lid close, the power button, Start > Sleep and the lock screen, which is what the next guard is for. |
| Show power plan | Switches to a copy of your active power plan with the lid set to do nothing and no display, sleep or hibernate timeouts, so a laptop driving a projector keeps going with its lid shut. Your own plan is never edited; at the end it's switched back and the copy deleted. If you pick another plan mid-show, your choice stands. |
| Turn off the screen saver | Only if it's on. |
| Do Not Disturb | Turns on Do Not Disturb, so no notification banner lands on an output. Whatever it lets through (alarms, reminders and the apps you've set as priority in Settings > System > Notifications) still gets through, so check that list on the show machine. |
| Turn off Snap | Windows stop snapping to the screen edges, the Snap layouts box stops appearing over the maximise button, and dragging a window to the top no longer maximises it. |
| Hide app crash dialogs | When an app crashes, Windows can put a "stopped working" window over everything. This keeps it away; the crash report is still saved. |
| Black out desktop wallpaper | Paints the desktop black on every screen, including one plugged in mid-show, so an extended desktop never shows your wallpaper on the projector. Your pictures, their position and a running slideshow come back at the end. With Windows Spotlight as the background it's left alone, because Spotlight can't be put back exactly. |
| Mute apps not allowed to play | **Off by default.** See below. |

## Keeping other apps off the PA

When the PC's output feeds a PA, Outlook's new-mail chime or a browser tab is one stray click
away from the audience. With **Mute apps not allowed to play** ticked, every app is silent
during the show except the ones ticked under **Allowed to Play** in the menu: your show app, and
anything else that should be heard. The menu lists the apps with sound now, marks the ones
playing, and remembers your choices.

- **It catches sound that's already playing**, as well as anything that starts later. That's one
  thing Windows does better than the Mac here.
- **Apps that play through ASIO or exclusive mode** (a lot of show and audio software) bypass
  Windows' mixer, so they can't be muted, and they're never muted by accident either.
- **Apps that record or stream the output are muted too** unless they're ticked. A recording of
  the output (OBS's desktop audio, say) is an audio session like any other, and a muted one
  records silence. Tick it under **Allowed to Play** if it should hear the show.
- **Nothing is turned down.** The output's volume and mute are never touched.
- **Windows remembers a mute.** An app muted by Show Mode would stay muted, even after a crash and
  even the next time it opens. So a small watchdog runs alongside Show Mode while apps are muted
  and unmutes them the moment Show Mode goes. An app that was muted and then quit mid-show is
  unmuted the next time it plays, as long as Show Mode is running then (turn on **Launch at
  Login**).

## Driving it from somewhere else

Show Mode answers a URL scheme:

```
ShowMode.exe showmode://on
```

The others are `showmode://off` and `showmode://toggle`. Companion's shell action can run that
on the same PC. A copy of Show Mode started with a command hands it to the running one and
leaves.

If another app already handles `showmode:` (a Parallels guest passes it to the Mac's Show Mode,
for one), Show Mode leaves that alone, and `ShowMode.exe showmode://on` still works.

## Putting things back

Before Show Mode changes anything it writes down what was there, in
`%LOCALAPPDATA%\ShowMode\journal.json`. Ending the show restores those values and nothing else,
so a setting you'd already switched off yourself stays off. The same happens when you quit, and
if the app crashes or the PC restarts mid-show the next launch finds the journal, restores it and
tells you it did. **Launch at Login** in the menu is worth turning on for a show machine, so a
restart mid-show is followed by a clean restore.

If something can't be put back, the menu says so and offers **Finish Restoring**, and the next
launch tries again.

## What it did

Show Mode keeps a plain log of what it changes in `%LOCALAPPDATA%\ShowMode\ShowMode.log`.
**Open Log** in the menu opens it.

`ShowMode.exe --status` prints what it can see (the active power plan, the screen saver, Snap,
Do Not Disturb, the wallpaper, apps with sound, the journal) without changing anything.
`--status --out <file>` writes it to a file instead.
