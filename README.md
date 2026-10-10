# Liquid Rust Demo (Android)

Android demo of **liquid-rust** (https://github.com/al1h3n/liquid-rust, crate `liquid-rust`):
Apple-style Liquid Glass for `wgpu`. The whole app is Rust (NativeActivity + winit + wgpu).

What you can do in the app
- Switch between 7 animated backgrounds with the arrows at the top.
- Drag the glass buttons around. The three round ones are in one `Container`, so they fuse
  like liquid when they get close.
- Pills show the presets `Material::regular / tinted / clear / clear_bar`.
- Sliders edit `Material` fields live: frost (blur), refraction, depth, dispersion, specular,
  tint opacity, container merge distance, corner radius.

Code map
- `src/ui.rs`   : builds the `liquid_rust::Scene` (Glass, Container, Material) and handles touch.
- `src/lib.rs`  : wgpu setup, content pass, `Renderer::render`, text overlay.
- `src/shader.wgsl` : background shaders and the tiny quad shader (panel + text).
- `app/`        : Gradle wrapper project that only packages `libliquid_demo.so` into an APK.

CI: `.github/workflows/android.yml` builds the APK (artifact `LiquidRustDemo-debug-apk`).
Needs a GPU with Vulkan on the phone (any recent Android phone).
