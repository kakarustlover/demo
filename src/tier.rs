//! Quality tiers = the fallback ladder. The user picks ULTRA (0), OPTIMIZED (2) or AURORA (5);
//! the governor walks down the ladder when the device is too slow.

#[derive(Clone, Copy, Debug)]
pub struct Tier {
    pub name: &'static str,
    /// Render scale of the glass scene (the result is upscaled to the screen).
    pub scale: f32,
    /// Frame cap while something moves.
    pub fps: f32,
    /// Below this many fps (while moving) the governor drops to the next tier.
    pub min_fps: f32,
    pub disp: bool,
    pub rim: bool,
    pub hairline: bool,
    pub shadow: bool,
    pub edge: bool,
    pub frost: f32,
    /// false = Aurora UI: liquid-rust is not called, glass is drawn as flat translucent cards.
    pub glass: bool,
}

pub const TIERS: [Tier; 6] = [
    Tier { name: "ULTRA",     scale: 1.00, fps: 60.0, min_fps: 40.0, disp: true,  rim: true,  hairline: true,  shadow: true,  edge: true,  frost: 1.0, glass: true },
    Tier { name: "HIGH",      scale: 0.85, fps: 60.0, min_fps: 40.0, disp: false, rim: true,  hairline: true,  shadow: true,  edge: true,  frost: 1.0, glass: true },
    Tier { name: "OPTIMIZED", scale: 0.75, fps: 30.0, min_fps: 18.0, disp: false, rim: false, hairline: true,  shadow: true,  edge: true,  frost: 1.0, glass: true },
    Tier { name: "LITE",      scale: 0.60, fps: 30.0, min_fps: 18.0, disp: false, rim: false, hairline: false, shadow: false, edge: true,  frost: 0.8, glass: true },
    Tier { name: "MINI",      scale: 0.45, fps: 30.0, min_fps: 15.0, disp: false, rim: false, hairline: false, shadow: false, edge: false, frost: 0.6, glass: true },
    Tier { name: "AURORA",    scale: 0.50, fps: 30.0, min_fps: 1.0,  disp: false, rim: false, hairline: false, shadow: false, edge: false, frost: 0.0, glass: false },
];

pub const LAST: usize = TIERS.len() - 1;
