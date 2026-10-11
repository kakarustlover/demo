//! Quality tiers = the fallback ladder. Tier 0 is the full Liquid Glass look, the last tier
//! does not call the glass shader at all (flat translucent rects), so it can never be slow.

#[derive(Clone, Copy, Debug)]
pub struct Tier {
    pub name: &'static str,
    /// Render scale of the glass scene (the result is upscaled to the screen).
    pub scale: f32,
    /// Frame cap while something moves.
    pub fps: f32,
    /// Chromatic dispersion (3x the blur taps).
    pub disp: bool,
    /// Bright rims + rim shade (3 distance-field evaluations per pixel).
    pub rim: bool,
    /// Inner hairlines (2 evaluations).
    pub hairline: bool,
    /// Drop shadow (evaluated on every pixel around the glass).
    pub shadow: bool,
    /// Dark outer hairline.
    pub edge: bool,
    /// Multiplier on the blur radius.
    pub frost: f32,
    /// false = liquid-rust is not called, glass is drawn as flat rects.
    pub glass: bool,
}

pub const TIERS: [Tier; 6] = [
    Tier { name: "ULTRA",    scale: 1.00, fps: 60.0, disp: true,  rim: true,  hairline: true,  shadow: true,  edge: true,  frost: 1.0, glass: true },
    Tier { name: "HIGH",     scale: 0.85, fps: 60.0, disp: false, rim: true,  hairline: true,  shadow: true,  edge: true,  frost: 1.0, glass: true },
    Tier { name: "BALANCED", scale: 0.75, fps: 30.0, disp: false, rim: false, hairline: true,  shadow: true,  edge: true,  frost: 1.0, glass: true },
    Tier { name: "LITE",     scale: 0.60, fps: 30.0, disp: false, rim: false, hairline: false, shadow: false, edge: true,  frost: 0.8, glass: true },
    Tier { name: "MINI",     scale: 0.45, fps: 30.0, disp: false, rim: false, hairline: false, shadow: false, edge: false, frost: 0.6, glass: true },
    Tier { name: "SAFE",     scale: 0.50, fps: 30.0, disp: false, rim: false, hairline: false, shadow: false, edge: false, frost: 0.0, glass: false },
];

pub const LAST: usize = TIERS.len() - 1;
