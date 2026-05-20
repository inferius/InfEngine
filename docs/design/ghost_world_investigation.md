# Ghost World DPI Artifact Investigation (W34.4)

## Symptom
User reported a "ghost world in upper-left" - a small duplicate rendering in the
upper-left corner of the window. First observed after W28.x; a W28 retrospective
audit ruled out shadow pass and debug overlays as causes.

## What Was Checked

### 1. Swapchain configuration (infengine_render/src/lib.rs)
- `present_mode: PresentMode::AutoVsync` -- standard, no known ghost-frame behavior.
- `desired_maximum_frame_latency: 2` -- standard double-buffering.
- Resize path: `Renderer::resize(w, h)` calls `surface.configure(&device, &config)`
  with updated width/height. No stale state detected.

### 2. Resize event handling (infengine_window/src/lib.rs)
- `WindowEvent::Resized(size)` fires with **physical** pixel dimensions. The handler
  calls `on_resize(size.width, size.height)` which correctly passes physical pixels to
  `Renderer::resize`.
- **`ScaleFactorChanged` is not handled.** On Windows 11, a DPI change fires
  `ScaleFactorChanged` followed by `Resized`. The `Resized` handler should cover the
  swapchain reconfigure, but the intermediate state between the two events could leave
  a mismatched swapchain if the OS delivers them in a specific order.

### 3. Initial renderer size (on_start)
- Window is created with `LogicalSize::new(1280, 720)` (infengine_window/src/lib.rs).
- `Renderer::new` calls `window.size()` -> `inner.inner_size()` (physical pixels) to
  initialize the swapchain.
- **On Windows with wgpu DX12 backend**: `inner_size()` called synchronously in
  `resumed()` has been observed to return logical pixels (pre-DPI) in some winit
  0.30 configurations before the OS compositor scales the window. If the user's
  display is 2x DPI (e.g. 4K @ 200%), `inner_size()` could return 1280x720 while
  the actual surface is 2560x1440. The renderer would configure a 1280x720 swapchain
  in a 2560x1440 surface slot.

### 4. Viewport mismatch
- The 3D camera and depth texture are created with the same `(width, height)` as the
  swapchain. If the swapchain was initialized at the wrong size, the viewport/scissor
  would also be wrong.
- wgpu presents the backbuffer to the OS. If the backbuffer is smaller than the native
  surface, the compositor stretches (or letter-boxes) it - on DirectX 12 on Windows 11
  the window might show a stretched version of the small render in the full window,
  OR it might show the raw backbuffer in the upper-left with undefined content elsewhere.
  The latter matches the "ghost in upper-left" description.

### 5. egui pixels_per_point
- `EditorUi::new` passes `window.scale_factor() as f32` to `egui_winit::State::new`.
  This is correct - egui reads the actual DPI at construction time.
- Subsequent DPI changes are forwarded via `on_window_event` -> `winit_state.on_window_event`.
  egui handles scale-factor changes internally.

### 6. Shadow pass
- Shadow pass renders to an offscreen depth texture (no color attachment). Unlikely
  source of visible duplicate rendering.

## Most Likely Cause

**Windows DPI + wgpu DX12 initial swapchain size mismatch.**

On a Windows 11 machine with a >100% DPI scale (common on 4K laptops), the sequence is:

1. `on_start` runs during `resumed()` (before the window is shown).
2. `inner_size()` returns the **logical** size (1280x720) because the OS has not yet
   applied the DPI scale to the window.
3. `Renderer::new` configures the swapchain at 1280x720.
4. The window becomes visible; the OS realizes the surface should be 2560x1440.
5. wgpu presents the 1280x720 backbuffer. The compositor stretches it to fit the
   2560x1440 surface, but due to DirectX 12 swapchain semantics, the top-left
   1280x720 pixels may be rendered at 1:1 while the remaining area is filled with
   stale GPU memory -- which shows as a "ghost world" (the previous frame's render,
   uncleared, at the smaller resolution) in the upper-left corner.

## Suggested Mitigation

**Short-term (safe, no refactor required):**

Add a `WindowEvent::ScaleFactorChanged` handler in `infengine_window/src/lib.rs`
that calls `on_resize` with the new physical size:

```rust
WindowEvent::ScaleFactorChanged { scale_factor: _, inner_size_writer } => {
    // winit 0.30: new_inner_size is provided via inner_size_writer.
    // We can also re-read inner_size() after the event.
    // Call on_resize so the renderer reconfigures the swapchain.
    // (The subsequent Resized event would also catch this, but the
    // ScaleFactorChanged fires first and the gap between the two
    // events is where the mismatch window exists.)
}
```

Actually for winit 0.30, `ScaleFactorChanged` arrives as:
```rust
WindowEvent::ScaleFactorChanged { scale_factor, inner_size_writer: _ }
```
followed by a `Resized` event. The `Resized` handler already reconfigures the
swapchain so adding a ScaleFactorChanged handler is not strictly necessary, but
it eliminates the one-frame gap where the swapchain is the wrong size.

**Better fix (one line in on_start):**

After `Renderer::new(window)` succeeds, immediately call `renderer.resize` with
the window's current physical size to ensure the swapchain matches even if
`Renderer::new` used a stale size:

```rust
// In on_start, after Renderer::new:
if let Some(r) = self.renderer.as_mut() {
    let (w, h) = window.size();
    r.resize(w, h);
}
```

This is a two-line idempotent fix. `resize` reconfigures the swapchain; if the
size was already correct it's a no-op (same config is re-applied). It guarantees
the swapchain is sized to the physical pixel count the OS reports at the time the
window is fully initialized.

## Debug Instrumentation (if unresolved)

Add logging to the `on_resize` callback and `Renderer::new`:

```rust
// In Renderer::new:
info!("renderer init: inner_size = {}x{}", width, height);
info!("renderer init: scale_factor = {}", window_scale_factor);

// In on_resize:
info!("on_resize: {}x{}", w, h);
```

Then run on the affected machine and compare the `inner_size` at `renderer init` vs
the first `on_resize` event. If they differ, the mismatch hypothesis is confirmed.

## Status
- No code change applied (research task per W34.4 spec).
- Suggested mitigations are low-risk and can be applied in the next wave.
