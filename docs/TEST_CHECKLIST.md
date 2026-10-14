# Test checklist

Run `cargo test` for the logic. Everything below is checked by hand on the real app (`dist\Reyer\Reyer.exe`).

## Window

- [ ] First launch (delete `%APPDATA%\Reyer\settings.json`): 960×540, centered.
- [ ] Resize to about 1100×650, move it somewhere, close, reopen: same size and position, windowed.
- [ ] Maximize, close, reopen: the last normal size and position, not maximized.
- [ ] Go fullscreen (F), close with Alt+F4, reopen: windowed at the last normal rectangle.
- [ ] Save a position on a second monitor, unplug it, reopen: centered on the primary monitor at the same size.
- [ ] Drag the video to move the window, then drag it to a screen edge: Aero Snap works. Win+↑/↓ work.
- [ ] Resize from every edge and corner. The window stops at 520×300 and the dock folds as it narrows (tools → "…", then the volume stays icon-only).
- [ ] Windows 11: rounded corners when windowed, square when maximized or fullscreen. The taskbar button minimizes and restores.
- [ ] Pin (or T) keeps the window on top, and video keeps playing without a hitch while toggling.
- [ ] At 125%, 150% and 200% scaling, text and icons are crisp, and moving the window between monitors with different scaling works.

## Opening files

- [ ] O and Ctrl+O open the dialog with the "All files" filter selected, and the Video and Audio filters are there.
- [ ] Each of these plays: `.mov` (HEVC), `.mov` (ProRes), `.mp4`, `.m4v`, `.mkv`, `.avi`, `.webm`, `.ts`.
- [ ] Each of these plays: `.m4a` (AAC and ALAC), `.m4b`, `.caf`, `.aiff`, `.flac`, `.mp3`, `.wav`, `.opus`, `.wma`, `.ape`.
- [ ] The window size never changes when a video opens; bars are filled by the background extension.
- [ ] Drop one file: its folder is queued in natural order (track2 before track10).
- [ ] Drop a folder, then several files: everything is queued, and the dock breathes once.
- [ ] `Reyer.exe file.mkv` and "Open with" Reyer open the file.
- [ ] Ctrl+V with a copied path or URL plays it.
- [ ] A renamed file with an odd extension still plays.
- [ ] A broken file shows "Can't play …" and Reyer stays alive. An `.m4p` shows the DRM toast.
- [ ] Remove `libmpv-2.dll`: a dialog explains how to get it.

## Audio

- [ ] An `.m4a` with a cover: the cover grows out of the title pill on first open, with a reflection below and a blurred background in the cover's colors. Title, artist and album come from the tags, and the title pill shows "artist — album".
- [ ] A `.flac` without a cover: a generated tile tinted from the title, and the same title always gives the same tile.
- [ ] Next track: the old cover scales away, the new one rises, and the background cross-fades.
- [ ] Optional: turn on "Generate Artwork for Tracks Without Cover" with ComfyUI present. Playback continues, the generated cover fades in when it's ready, and it's cached for next time.

## Playlist and transport

- [ ] Playback advances at the end of each file. The last file stops at its end.
- [ ] Previous restarts the track when more than 4 s in, and goes to the previous track otherwise.
- [ ] Click the video: play/pause after a short delay. Double-click: fullscreen, without a pause toggle.

## Speed

- [ ] Pressing "+" three times gives 1.75×, with a toast each time, and the number rolls up.
- [ ] Clicking the number gives 1.0×, with a pulse and a toast.
- [ ] Holding or right-clicking the number opens the presets popover, which grows out of it.
- [ ] `]`, `[` and Backspace work.
- [ ] At 0.5× and 2× voices keep a natural pitch, and lips stay in sync.

## Glass

- [ ] Over moving video the dock blurs the frame and its edges bend it; the rim is bright at the top-left.
- [ ] Over a bright scene the dock and window buttons get the 35% dim; over a dark scene they don't. There's no flicker at the threshold, and nothing changes while paused.
- [ ] Settings → Dock Shape: Single is one pill with dividers, and Grouped is five capsules. Switching morphs between them.
- [ ] Settings → Glass Style: Regular and Clear change the look as described in DECISIONS.
- [ ] Popovers (tracks, speed, settings, "…") grow out of their buttons and collapse back. Clicking twice quickly reverses smoothly.
- [ ] Seek and volume knobs become lenses only while hovered or dragged. Dragging past the end rubber-bands, and releasing settles with momentum.
- [ ] The seek bubble follows the pointer with a little lag, and chapters show as ticks.
- [ ] Toasts come out of the title pill, and a new toast replaces the old one instead of stacking.

## Auto-hide

- [ ] While playing, the UI and cursor hide after 2.5 s without movement, and moving the mouse brings them back.
- [ ] The UI stays visible while paused, while a popover is open, while dragging a slider, and while the pointer is over the dock or top bar.

## Accessibility

- [ ] With Windows "Transparency effects" off (or Settings → Reduce Transparency: On), all glass becomes opaque frosted panels with the same shapes, and all text is legible.
- [ ] With Windows high contrast on (or Increase Contrast: On), borders are stronger, text is brighter, the accent is brighter, and the clear glass is always dimmed.
- [ ] With "Animation effects" off (or Reduce Motion: On), there are no springs or morphs, only short fades.
- [ ] Keyboard only: Tab reaches every control (window buttons, transport, volume, seek, speed, tools, settings rows) with a visible focus ring. Space and Enter press, arrows adjust sliders and menus, and Esc closes.
- [ ] A screen reader (Narrator) names every control.

## Performance

- [ ] Starting from cold, the window is visible in under 1 s.
- [ ] Paused with the UI hidden, Task Manager shows about 0% CPU.
- [ ] A 4K HEVC file plays without dropped frames while hovering, opening popovers and dragging the seek bar (check with mpv stats or by watching).
- [ ] Closing during playback leaves no `Reyer.exe` behind.
