//! Colours for the layers and for the physical appearance of the board.

use crate::SceneLayer;
use lamina_model::{Copper, Side};

/// sRGB colour with alpha, 0..255.
pub type Rgba = [u8; 4];

pub const BACKGROUND: Rgba = [14, 16, 21, 255];
pub const BOARD_2D: Rgba = [24, 28, 36, 255];

/// Pour fills in the technical view are drawn at this share of the layer
/// colour, so that pads and tracks on the same layer stand out.
pub const POUR_ALPHA: f32 = 0.3;

/// Layer colours for the technical 2D view.
pub fn layer_color(l: SceneLayer, copper_layers: u8) -> Rgba {
    match l {
        SceneLayer::Copper(i) => match Copper::from_index(i, copper_layers) {
            Copper::Top => [226, 140, 58, 235],
            Copper::Bottom => [64, 140, 230, 220],
            Copper::Inner(1) => [92, 196, 120, 200],
            Copper::Inner(2) => [196, 110, 220, 200],
            Copper::Inner(n) => {
                let k = (n as usize * 47) % 160;
                [80 + k as u8, 180, 200 - k as u8 / 2, 200]
            }
        },
        SceneLayer::Mask(_) => [70, 200, 140, 60],
        SceneLayer::Paste(_) => [180, 180, 190, 90],
        SceneLayer::Silk(Side::Top) => [242, 242, 236, 255],
        SceneLayer::Silk(Side::Bottom) => [200, 200, 160, 200],
        SceneLayer::Courtyard(_) => [255, 92, 200, 160],
        SceneLayer::Fab(_) => [140, 156, 178, 170],
        SceneLayer::Keepout => [255, 80, 80, 40],
        SceneLayer::Outline => [255, 214, 79, 255],
        SceneLayer::Drills => [10, 12, 16, 255],
        SceneLayer::Ratsnest => [150, 170, 255, 230],
        SceneLayer::Drc => [255, 64, 100, 255],
        SceneLayer::DrcWarning => [255, 184, 64, 255],
    }
}

/// Solder mask colour by name.
pub fn mask(name: &str) -> Rgba {
    match name.to_ascii_lowercase().as_str() {
        "black" => [24, 24, 26, 255],
        "blue" => [22, 64, 140, 255],
        "red" => [150, 28, 30, 255],
        "white" => [235, 235, 232, 255],
        "yellow" => [214, 180, 30, 255],
        "purple" => [86, 40, 120, 255],
        "matte black" => [32, 32, 34, 255],
        _ => [22, 94, 52, 255],
    }
}

pub fn silk(name: &str) -> Rgba {
    match name.to_ascii_lowercase().as_str() {
        "black" => [20, 20, 20, 255],
        _ => [245, 245, 240, 255],
    }
}

/// The colour of exposed copper for a surface finish.
pub fn finish(name: &str) -> Rgba {
    let n = name.to_ascii_lowercase();
    if n.contains("enig") || n.contains("gold") {
        [222, 180, 82, 255]
    } else {
        [205, 207, 210, 255]
    }
}

/// Bare laminate.
pub const FR4: Rgba = [196, 176, 120, 255];
