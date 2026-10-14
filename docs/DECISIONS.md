# Decisions

Notes on how Reyer is built and why, including the places where my original spec could not be met literally and what was done instead.

## Stack

Rust 2021: winit (window), glutin (OpenGL 3.3 context), glow (GL calls), fontdue (text), tiny-skia + svgtypes (icons), lofty (tags and cover art), rfd (open dialog), arboard (clipboard), ureq (ComfyUI client), windows-sys (native window code). libmpv 0.41 is loaded at run time with `libloading` through a small binding in `src/mpv.rs`.

Versions 1.x were written in Python with PySide6/QML. 2.0 is a rewrite in Rust with the same behavior, visuals and design tokens (`assets/design-tokens.json`); the release is a 4 MB executable plus libmpv instead of a 345 MB Qt bundle.

- **Immediate-mode UI.** The interface is drawn every frame by a small immediate-mode toolkit (`src/ui`) on a custom OpenGL renderer (`src/gfx`). Persistent widget state (springs, focus, drags) lives in `UiState`. Frames are only requested while something animates or a timer is due, so an idle window costs no CPU.
- **Shaders** are GLSL 330 in `assets/shaders` and are embedded in the executable, as are the design tokens and the window icon. Only libmpv and the `assets/backgrounds` and `assets/art` images sit next to `Reyer.exe`.

## Video inside the scene

- mpv renders through the render API into an offscreen texture (`Gfx::video`), so the frame is a GPU texture that the glass and the background extension can sample.
- **Files load only after the render context exists.** If `loadfile` runs before `mpv_render_context_create`, mpv's `vo=libmpv` fails with "No render context set" and plays without video. `Player::start` creates the player and its render context together, before any file is opened.
- mpv draws top-down and the texture is sampled bottom-up (OpenGL convention), so it renders with `flip_y = 1`.
- `mpv_render_context_render` runs with `block_for_target_time = 0`; the UI thread never sleeps inside mpv.
- **Shutdown.** The render context is freed (on the UI thread, GL context current) before mpv terminates. Settings and the resume position are saved first.
- libmpv loads after the first frame is on screen, so the window appears before the 120 MB DLL is mapped, and a missing DLL gives a clear dialog instead of a crash.
- The built-in mpv Lua scripts (stats, console, OSC, …) are disabled. Reyer draws its own UI, and this keeps startup lean.
- Empty lists and maps from mpv can arrive with null pointers; the node conversion handles that before building slices.

## Window

- The window is undecorated with the DWM shadow kept (`with_undecorated_shadow`) and Windows 11 rounded corners (`DWMWCP_ROUND`, off when maximized or full screen). A window subclass answers `WM_NCHITTEST` with the 6 px resize edges. Moving uses `drag_window` when you drag the video, so Aero Snap works.
- **Geometry memory.** Only the normal (windowed) rectangle is saved, 400 ms after the last move or resize and again on close. Reopening after closing maximized or full screen therefore restores the last normal rectangle; the window never starts maximized or full screen. `restore_rect` (tested) clamps the rectangle to the work area it overlaps most and re-centers it on the primary screen if its monitor is gone.
- Reyer never touches the window size when a video loads; mpv letterboxes inside it.
- **Always on top** uses `SetWindowPos(HWND_TOPMOST)`, so the window and its GL context are never recreated.
- **Full screen** is borderless on the current monitor, and leaving it returns to the previous state. A top-level window's geometry can't be animated smoothly on Windows (each step re-allocates the swap chain), so the transition happens inside the scene: the content layer springs from 0.96 to 1.0.
- The Windows 11 snap-layouts flyout on the maximize button needs that button to answer `HTMAXBUTTON` hit tests, which would take its hover and press away from the UI. Snap works by dragging and with Win+arrows; the flyout is not supported.
- Only the title pill and the window buttons block clicks at the top; the rest of the top band still moves the window and toggles play/pause.

## Liquid Glass

- **Layers.** Content (video, Now Playing, empty-state background) is drawn into its own texture. A `Backdrop` copies it at quarter resolution and blurs it progressively: two Gaussian iterations at 1/4 give `fine` (≈9 px, Clear glass), then more at 1/8 and 1/16 give `coarse` (≈30 px, Regular glass). Every pass samples neighbouring texels (spread 1). An earlier version spread a 9-tap kernel over gaps of several texels to save passes, and every color edge showed up several times as stripes. The extra small passes cost very little. Glass elements sample these textures and are never part of the content, so there's no feedback loop and no glass sampling glass.
- **One container pass.** `Renderer::glass` draws up to six rounded shapes in one pass over their bounding box. The dock's five capsules are one pass, as are the title pill and window buttons. Shapes are combined with a polynomial smooth-union (`smoothUnion` token), so capsules that come close (during folding or the speed nudge) melt together.
- **Optics** (all tokens in `design-tokens.json`):
  - Refraction offsets the backdrop lookup along the SDF gradient, squared towards the rim (10 px Regular, 12 px Clear), with a faint red-channel dispersion at the edge.
  - Then saturation (1.35–1.5×), a luminosity lift, the variant's tint, and the dimming layer.
  - A 1 px specular rim runs 70% → 5% white with the light from the top-left, plus a faint glint opposite, a soft top sheen and a hairline border.
  - A two-part shadow (large and soft, plus a contact shadow) makes the panels float.
  - The light direction follows the pointer slightly.
- **Clear dimming rule.** Every 250 ms while a video plays and the chrome is visible, the coarse backdrop is read back and the mean luminance behind the dock and behind the top cluster is measured. The thresholds use hysteresis (on above 0.58, off below 0.44), and the 35% dim eases in and out. Nothing is sampled while paused, so the dim stays stable. With Increase Contrast the dim is always on.
- **Glass style.** In Auto, Clear is used only for the dock and window buttons (and Regular for everything text-heavy). Regular forces Regular everywhere. Clear also makes the title pill clear but keeps menus, toasts and the settings popover Regular, because Clear text menus aren't legible over video.
- **Text on Clear glass is always light-on-dark,** whatever the Windows theme. The dimming rule is what keeps it legible over bright frames. Regular glass follows the light or dark theme. Light colors are defined for all of it.
- **Glass over glass.** Popovers sit over the dock area, so they get `overlap = 1`, which raises their tint opacity (`overlapOpacityBoost`) and keeps stacked text legible.
- **Slider knobs.** At rest a knob is a flat white dot. While hovered, focused or dragged it grows (12 → 24 px) into a lens: a small shader magnifies and barrel-distorts the track under it (described analytically, no extra texture), with a rim highlight. On release it settles back.
- **Edge effect.** Top and bottom bands mix from the sharp backdrop to the coarse one, with a dim that's strongest at the window edge. That gives a progressive blur, not a gradient scrim. It's shown only over video and only with the chrome.
- **Background extension** is drawn by Reyer, not by mpv. mpv's own `--border-background=blur` smeared the edge rows into vertical streaks in top and bottom bars (they showed through the clear dock as stripes) and barely lit side bars. Now mpv clears its bars to transparent (`background-color=#00000000`). Behind the video, the content layer draws a 24-pixel-wide copy of the current frame (from `osd-dimensions`), blurred into soft light, dimmed, and aspect-filled to the window. It costs almost nothing, and the glass samples smooth light instead of streaks. The setting turns it off (black bars).
- **Content-layer material.** The empty-state card uses the same pass as a standard material: blur and tint only, with no lensing and a faint rim. The Now Playing background is a 64 px copy of the cover, desaturated, darkened and blurred, cross-fading between tracks.
- **Fallbacks.** Without OpenGL 3.3 Reyer shows a message and exits. Reduce Transparency uses the frosted color inside the glass shader with refraction off.

## Motion

- **Springs.** `src/spring.rs` uses the exact closed-form solution of a damped oscillator (mass 1, `ω = 2π/response`, `k = ω²`, `c = 2ζω`). Steps are frame-rate independent (tested: 60 Hz, 120 Hz and one big step all match), and position and velocity carry through any retarget. With Reduce Motion, springs jump to their target and only short cross-fades (120 ms) remain.
- **Drags.** Sliders follow the pointer 1:1, and past the ends they rubber-band (unit-tested). On release the knob settles with the measured pointer velocity.
- Opacity and tint changes use 160–240 ms ease-out curves, never linear.
- **Popovers.** A popover's shape interpolates position, size and corner radius from the button that opened it (SNAPPY), and its content travels and is clipped with the shape. Content fades in once the shape is about 30% open and only accepts input from 70%; menu rows fade with a 4% progress offset each. These are progress-based stand-ins for the 60 ms and 15 ms timings: they stay correct when interrupted and reversed. Dismissing reverses into the button, and asking for the open popover again closes it.
- **Toasts** grow out of the title pill, hold 1.3 s and shrink back. A new toast cross-fades its text instead of stacking. Mute and track toasts describe the state mpv is switching to, not its delayed echo.
- **Auto-hide.** The chrome fades and drifts 8 px, and comes back on the next movement. The cursor hides with it, and hidden controls don't take clicks.
- **Dock shape.** By default the dock is one continuous pill, with hairline dividers between the groups. Settings → Dock Shape → Grouped brings back separate capsules. Switching animates: the transport capsule grows into the pill while the others sink into it (and the reverse).
- **Dock folding.** When the window is narrow, the time labels give their room to the seek bar (the hover bubble still shows the time). After that, the tools capsule folds into a "…" button with a glass popover, then the volume slider stops unfolding and the volume group stays icon-only. Each step is a width spring, so the capsules morph.
- **Empty state.** Four generated backgrounds drift slowly and cross-fade every 18 s; they pause while the window is inactive or on battery saver.

## Playback

- The `gpu-hq` profile is applied (it's `high-quality` on newer mpv; both names are tried), with `hwdec=auto-safe` (`REYER_HWDEC` overrides it), tone mapping set to `auto` with peak detection for HDR → SDR, `sub-auto=fuzzy`, `keep-open` with auto-advance, and `audio-pitch-correction` (scaletempo2) at every speed.
- **Volume curve.** mpv's volume is cubic: its 50% is only 12.5% amplitude (−18 dB), so half the slider sounded almost silent. Reyer's percentage is linear in amplitude like most players (50% = −6 dB). Past 100% it boosts up to 2× amplitude (+6 dB) at 150%, for quiet sources. Keys and the wheel show a volume toast.
- Seeks are keyframe seeks while dragging and exact on release. Chapters show as ticks.
- **Speed stepping** keeps the requested speed itself. mpv echoes speed changes asynchronously, so three quick presses must give 1.75× rather than stepping from a stale 1.0×.
- **Resume.** Positions are remembered only for files longer than 60 s, played more than 10 s in and with more than 10 s left. The newest 300 are kept.
- **DRM.** `.m4p` files get a toast without trying to play. Any other file that fails to open is scanned at both ends (the `moov` box can be at either) for FairPlay sample entries (`drms`/`drmi`), so the toast says why. The scan doesn't run before every load, because an accidental 4-byte match in compressed data would block a playable file.
- **Audio files.** `audio-display=no`: mpv doesn't show the cover as video, and the Now Playing screen draws it instead. Tags come from mpv, cover art from `lofty`; files without a cover get one of twelve generated tiles, tinted by a hash of the title. There is no visualizer: the spec allows one only if it's driven by real audio data, and leaving it out keeps the content calm and the CPU idle.

## Not done, or simplified

- **Seek-bar thumbnail previews** aren't done. They need a second decoder instance; the time bubble is implemented.
- **Single instance.** "Open with" on a second file starts a second window. Forwarding to the running instance can be added if wanted.
- **Chrome blur while fading** and the previous cover scaling away on a track change aren't done; the chrome fades and drifts, the new cover scales in.
- **Focus rings** appear only while the keyboard is being used to move around (Tab or arrows), and disappear on the next mouse click, like CSS `:focus-visible`.
- **File information** (Settings → File Info, or I) is read from mpv on demand: file, container, video (codec, resolution, frame rate, color, HDR, which hardware decoder is in use), audio, tags and tracks. Clicking a value copies it.
- **Surround passthrough** (Settings, off by default) sends AC-3, E-AC-3, DTS/DTS-HD and TrueHD to an AV receiver or soundbar untouched (`audio-spdif`, WASAPI exclusive mode). That is how Dolby Atmos and DTS:X reach a receiver: their object metadata travels inside those bitstreams, and decoding it on the PC needs Dolby's and DTS's licensed decoders, which no open player has.
  - mpv doesn't fall back when the output device refuses the bitstream: audio and then playback stall. So Reyer watches mpv's audio-output errors, switches to normal decoding and shows a toast.
  - Audio output is pinned to WASAPI; falling through to OpenAL is what stalled.
- **Not supported, on purpose or by necessity:**
  - Dolby AC-4: FFmpeg has no AC-4 decoder, so there's nothing to build on.
  - DRM-protected media (FairPlay `.m4p`, streaming services): removing copy protection is circumvention, which I won't build. Apple has sold music DRM-free since 2009, so purchases can be re-downloaded unprotected.
- **Subtitles** come from the file itself (embedded tracks) or from a subtitle file next to it with a matching name, which loads automatically (`sub-auto=fuzzy`). "Add Subtitle File…" in the subtitles menu loads any other file. Reyer doesn't generate or download subtitles.
- **Settings entry point.** The spec doesn't place a Settings button. It's a sliders icon in the Tools group, and Ctrl+, opens it as well.

## Assets

Made with my local ComfyUI (Z-Image, 8 steps). Jobs go through the studio server on :8189, which proxies ComfyUI on :8188 and starts it on demand, and whatever was started is stopped again so the GPU is freed. Two to four candidates were made per prompt and each was reviewed by eye before picking. Prompts and seeds are in `assets/PROMPTS.md`. The app icon is composed from two generated layers (background, glass glyph) plus a drawn accent glow, rim highlight and rounded-square mask, exported as a multi-size `.ico` with PNG entries.
