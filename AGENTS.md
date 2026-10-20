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
- **Never touch output volume or a device's mute.** A failure costs the protection, never the
  show.
- **Undocumented interfaces are looked up at runtime** (QuietHoursSettings) so a Windows that
  removes them degrades to "unavailable" rather than crashing. **Never write a binary blob you
  can't validate** (the Night light and CloudStore blobs): unlike a missing COM class, a
  mis-written blob fails silently by breaking Settings. nightlight-cli was withdrawn in May 2026
  after exactly that.
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
- **Build from the Mac** with `scripts/build.sh` (cargo-xwin). The CRT is linked statically
  (`.cargo/config.toml`) so a show machine needs no Visual C++ redistributable.
