# Agent guide: push this repo and get a green APK build

(خلاصه فارسی: پروژه را روی گیت‌هاب پوش کن، اکشن APK می‌سازد. این کد بدون امکان بیلد نوشته شده،
پس احتمالاً چند خطای کوچک API در اولین بیلد می‌آید. طبق جدول پایین فقط همان‌ها را اصلاح کن.)

## What this is
100% Rust Android app using the crate `liquid-rust` (al1h3n/liquid-rust, wgpu 30).
Packaged with a minimal Gradle project (no Kotlin/Java code, `hasCode=false`, NativeActivity).
Do NOT replace liquid-rust with anything else and do NOT add Kotlin/Compose.

IMPORTANT: the code was written WITHOUT being able to compile it (no network in the authoring
environment). The liquid-rust API calls were taken from docs.rs, but the wgpu 30 / winit
boilerplate was written from older wgpu knowledge. Expect a few compile errors on the first run.
Best reference for correct wgpu 30 + winit usage: `examples/demo.rs` in
https://github.com/al1h3n/liquid-rust (open it on GitHub when fixing).

## Push steps
1. `cd LiquidRustDemo && git init && git add . && git commit -m "Initial commit"`
2. `git branch -M main`
3. Create an empty GitHub repo, `git remote add origin <url>`, `git push -u origin main`
   (or `gh repo create LiquidRustDemo --public --source=. --push`).
4. The push triggers the workflow "Build APK". Watch it: `gh run watch`.
5. Download: `gh run download -n LiquidRustDemo-debug-apk`.
6. After the first green build, commit the generated `Cargo.lock` (`git add Cargo.lock`) so builds are reproducible.

## Fix loop
Get the error with `gh run view --log-failed`. Make the smallest possible fix, one commit per fix.

| Error | Fix |
|---|---|
| `package requires rustc 1.99` / edition 2024 unsupported | In the workflow use `dtolnay/rust-toolchain@beta` (or `nightly`). |
| `no variant CurrentSurfaceTexture` / mismatched types on `get_current_texture` | wgpu 30 API differs. Copy the pattern from liquid-rust `examples/demo.rs` (Result vs enum). Same function in `src/lib.rs`, `Gfx::render`. |
| `no field multiview_mask` / `unknown field multiview` | Rename it (`multiview` <-> `multiview_mask`) in the two `RenderPipelineDescriptor`s and two `RenderPassDescriptor`s. |
| `push_constant_ranges` unknown | Replace with `immediate_size: 0` in `PipelineLayoutDescriptor`. |
| `bind_group_layouts` type mismatch | Use `&[Some(&bgl)]`. |
| `Instance::default()` / `create_surface` errors | Use the instance/surface creation from demo.rs (newer wgpu may need an `InstanceDescriptor` or a display handle). |
| `request_device` signature | Match demo.rs. |
| liquid_rust `Frame`/`Glass`/`Scene` method not found | Check https://docs.rs/liquid-rust/latest/liquid_rust/ . Methods used: `Scene::{new, add, add_container, set_material, set_shape, set_frame, set_container_spacing, pointer_down, pointer_move, pointer_up, update}`, `Glass::{new, shape, material, interactive, container}`, `Material::{regular, clear, clear_bar, tinted}` + public fields, `Color::{WHITE, BLUE, with_alpha}`, `Rect::new`, `Vec2::new`, `Shape::Rounded`, `Frame {content, target, size, scale, content_changed}`, `Renderer::{new, render}`. |
| `cargo ndk` / NDK not found | `echo $ANDROID_NDK_HOME` in the workflow; install NDK with `sdkmanager "ndk;27.2.12479018"` and export `ANDROID_NDK_HOME`. |
| Gradle/AGP errors | Keep AGP 8.13.2 with Gradle 8.14.3 (or bump both together). The Gradle project only packages the .so. |
| App crashes on start (black screen) | `adb logcat -s RustStdoutStderr liquid_demo` (panic hook logs `PANIC: ...`). Most likely surface format / present mode; try `PresentMode::AutoVsync` or the first format in `caps.formats`. |

## Done when
Run is green and the artifact `LiquidRustDemo-debug-apk` downloads. Install on an Android 8+ phone
with Vulkan. Expected: animated background, glass buttons you can drag and fuse, sliders that
change blur/refraction/etc. live.
