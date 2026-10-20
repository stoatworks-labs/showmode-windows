# AGENTS.md — Show Mode for Windows

Invariants for anyone (human or agent) changing this repo. The macOS app's AGENTS.md
(stoatworks-labs/showmode) is the parent of most of these; the Windows-specific ones are where
Windows behaves differently.

- **Journal before you change.** Every guard records the value it replaces in `journal.rs`
  *before* writing the new one. Restore must only undo what show mode changed, and a crash
  mid-engage must still be recoverable on the next launch. Each restore removes its own journal
  entry only once it has put the value back; what's left is retried by Finish Restoring and the
  next launch. A setting someone changed by hand mid-show stands: restore writes the old value
  only where show mode's value is still there.
- **Record once.** `journal::record` keeps the first value it saw, so engaging over a restore
  that never finished never journals show mode's own value as "previous".
- **Muting outlives the muter on Windows.** A per-app mute set through `ISimpleAudioVolume`
  stays after the process that set it dies, and the app comes back muted on its next launch
  (measured by recording in the bench spike, 25H2). The macOS tap is the opposite. So: journal
  each app before muting it, keep the watchdog (`--watchdog`, which waits on a process *handle*,
  never a pid) running for as long as anything is muted, and put apps that quit mid-show on the
  pending list so they're unmuted when they next play. Never mute without the watchdog up.
- **A loopback recording is an audio session too.** Muting it makes the recorder hear silence:
  the bench's own recorder had to be on the allow-list, and so would OBS capturing the output.
- **Never touch output volume or a device's mute.** A failure costs the protection, never the
  show.
- **Undocumented interfaces are looked up at runtime** (QuietHoursSettings) so a Windows that
  removes them degrades to "unavailable" rather than crashing. **Never write a binary blob you
  can't validate** (the Night light and CloudStore blobs): unlike a missing COM class, a
  mis-written blob fails silently by breaking Settings. nightlight-cli was withdrawn in May 2026
  after exactly that.
- **No COM inside a sent message.** WM_COPYDATA from another process is input-synchronous, and
  COM refuses outgoing calls during one (RPC_E_CANTCALLOUT_ININPUTSYNCCALL): Do Not Disturb and
  the wallpaper both reported "unavailable" on winlab. Post the command to the window and run it
  from the message loop.
- **WM_DISPLAYCHANGE is sent too.** Its handler only notes the time and sets a timer; the
  wallpaper and display-lock work runs on the timer, a second after the last change.
- **Display corrections are temporary.** `SetDisplayConfig` never gets `SDC_SAVE_TO_DATABASE`,
  so the layouts Windows remembers for each set of screens stay the user's. Keep both limits: four
  corrections inside twenty seconds (the macOS fight limit), and two in a row that fail or don't
  hold. Without the second, a correction that can't work was retried all show: the five-second
  check spaced the retries so the first limit never tripped (winlab, 2026-10-20).
- **A mirror is screens at the same origin, not a shared source.** Windows' Duplicate across two
  adapters put all three of winlab's screens on the virtual adapter's one source, with the other
  adapter's monitor under a made-up target id, and left no source free to split one off. Key
  monitors by device path, and when a split can't be built by hand, fall back to
  `SDC_TOPOLOGY_EXTEND` (unsaved), but never over a mirror that was there at the start.
- **Pre-show checks only read.** Night light's state is parsed from its CloudStore blob only after
  the blob's shape checks out; anything else reads as unknown and reports nothing.
- **Read back every restore.** SPI_SETWINARRANGING with the value in pvParam alone switched Snap
  off and never back on, and the restore logged success. The value goes in uiParam; restores now
  read the setting back and keep the journal entry if it didn't take.
- **Spotlight on 25H2** leaves no BackgroundType value; its pictures live under
  `...\Microsoft\IrisService\`. Never black out over Spotlight: it can't be put back.
- **Never take over someone else's showmode: handler.** A Parallels guest registers the Mac's URL
  schemes (`SharedHostApplication`); Show Mode only writes the scheme when it's absent or already
  points at a ShowMode.exe.
- **No admin in v0.1.** Everything is per user. An admin step, when one comes, goes through a
  signed helper in Program Files taking named steps, never a script or a command string, and
  never a service or a highest-privileges scheduled task.
- **No Ctrl+Alt+letter hotkeys.** AltGr arrives as Ctrl+Alt, so any such chord steals a
  character on some keyboard layout. The default is Ctrl+Alt+Shift+F11. F12 is the debugger's.
- **Restarts can't be refused.** `WM_QUERYENDSESSION` returning FALSE only puts Show Mode on
  Windows' list of apps holding up an ordinary restart; a forced update restart ignores it.
  Treat it as a courtesy, and catch a pending restart before the show instead.
- **The note is not a notification.** Show mode may just have switched notifications off, so
  warnings go in the menu and the note window by the notification area, which never takes the
  focus.
- **One version:** `Cargo.toml`. The app reads it with `env!("CARGO_PKG_VERSION")` and
  `scripts/build.sh` names the executables from it.
- **The icons are drawn** by `scripts/make-icons.swift`, never taken from SF Symbols or Segoe
  Fluent Icons; the PNGs in `Resources/tray` are embedded by `src/icons.rs`.
- **Verify on a real machine.** What has and hasn't been run lives in `docs/USER-GUIDE.md`'s
  status block. Update it when something is actually run on Windows, never because it compiles.
- **Testing on winlab** (the shared lab VM, `ssh winlab`): ask Allan first, take the box's lock
  (`mkdir ~/.local/state/tranche3-locks/winlab` plus an `owner` file) and give it back after.
  Run the app in the signed-in session through a one-shot scheduled task at `Limited` run level,
  which is the token a no-admin app gets (the spike's `ps/run-in-session.ps1` does this); an ssh
  session can't see the desktop, the tray or Do Not Disturb. winlab has no audio hardware: test
  muting on the Parallels guest, by recording a loopback capture with test tones (440 Hz not
  allowed, 1 kHz allowed) and a Goertzel filter, as the macOS AGENTS.md describes.
- **Driving a bench run:** the tray menu opens through UI Automation (the icon's InvokePattern,
  as a click; call it on the script's own thread, since a PowerShell script block on a thread-pool
  thread silently does nothing) and closes with Escape posted to the `#32768` menu window. Windows
  11 puts new tray icons in the overflow; `IsPromoted` on the icon's entry under
  `HKCU\Control Panel\NotifyIconSettings` shows it, and that entry belongs to the test copy, so
  delete it afterwards. On the Parallels guest, `prlctl exec "Windows 11" --current-user` runs in
  Allan's session and `Y:` is `~/Projects`; it's his own account, so switch off every guard the
  test doesn't need, play test tones at −40 dBFS rather than the spike's −12, and put back
  anything the run changed.
- **Build from the Mac** with `scripts/build.sh` (cargo-xwin). The CRT is linked statically
  (`.cargo/config.toml`) so a show machine needs no Visual C++ redistributable.
