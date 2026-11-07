//! The pad number and the pin name on each pad, fitted to the pad as it
//! is on the screen. The text runs along the long side of the pad and is
//! never upside down. If the full pin name does not fit, the label uses
//! the first part of the name (`P1.14` of `P1.14/AIN7`), then the pad
//! number only, then nothing.

use std::f32::consts::{FRAC_PI_2, PI};

/// The smallest font size for a pin name, in points.
pub const MIN_NAME: f32 = 8.0;
/// The smallest font size for a pad number, in points.
pub const MIN_NUMBER: f32 = 7.0;
/// The largest font size of a label, in points.
pub const MAX_SIZE: f32 = 15.0;
/// The line height of the label font, as a factor of the font size.
pub const LINE: f32 = 1.15;

/// How the number and the name sit on the pad.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Arrange {
    /// The number on one line and the name on the line below.
    Stacked,
    /// The number and the name on one line.
    Inline,
    /// The number only.
    Number,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Fit {
    pub arrange: Arrange,
    /// The name to show. It can be shorter than the pin name.
    pub name: Option<String>,
    /// The font size in points.
    pub size: f32,
    /// The text angle in radians, clockwise on the screen.
    pub angle: f32,
}

/// A pad as it is on the screen.
#[derive(Debug, Clone, Copy)]
pub struct PadView {
    /// The size along the local X axis of the pad, in points.
    pub along_x: f32,
    /// The size along the local Y axis of the pad, in points.
    pub along_y: f32,
    /// The direction of the local X axis, in radians clockwise on the screen.
    pub x_angle: f32,
}

/// The labels that a pad can show, the best first: the full pin name, the
/// first part of the name, the pad number only. Each one has the largest
/// size that fits, not more than `MAX_SIZE`. `advance` is the width of one
/// character of the monospace font, as a factor of the font size.
fn choices(pad: PadView, number: &str, name: Option<&str>, advance: f32) -> Vec<Fit> {
    let (long, short, axis) = if pad.along_y > pad.along_x {
        (pad.along_y, pad.along_x, pad.x_angle + FRAC_PI_2)
    } else {
        (pad.along_x, pad.along_y, pad.x_angle)
    };
    // The round ends of a pad take some of its length.
    let room_len = (long - 0.35 * short).max(0.0) * 0.92;
    let room_wide = short * 0.8;
    let chars = |s: &str| s.chars().count() as f32;
    let fits = |lines: f32, n: f32| {
        (room_wide / (lines * LINE))
            .min(room_len / (n * advance))
            .min(MAX_SIZE)
    };
    let angle = upright(axis);
    let num = chars(number);
    let mut out = Vec::new();
    if let Some(name) = name.filter(|n| !n.is_empty() && *n != number) {
        let head = name.split('/').next().unwrap_or(name);
        for text in [name, head] {
            if out.iter().any(|f: &Fit| f.name.as_deref() == Some(text)) {
                continue;
            }
            let stacked = fits(2.0, num.max(chars(text)));
            let inline = fits(1.0, num + 1.0 + chars(text));
            let (arrange, size) = if stacked >= 0.8 * inline {
                (Arrange::Stacked, stacked)
            } else {
                (Arrange::Inline, inline)
            };
            if size >= MIN_NAME {
                out.push(Fit {
                    arrange,
                    name: Some(text.to_string()),
                    size,
                    angle,
                });
            }
        }
    }
    let size = fits(1.0, num);
    if size >= MIN_NUMBER {
        out.push(Fit {
            arrange: Arrange::Number,
            name: None,
            size,
            angle,
        });
    }
    out
}

/// The largest label size at which the pad shows a pin name, or its
/// number if no name fits. Give the smallest of these sizes to `fit` for
/// all the pads of the same size, so that their labels match.
pub fn comfortable(pad: PadView, number: &str, name: Option<&str>, advance: f32) -> Option<f32> {
    let c = choices(pad, number, name, advance);
    let named = c.iter().filter(|f| f.name.is_some()).map(|f| f.size);
    named.reduce(f32::max).or_else(|| c.last().map(|f| f.size))
}

/// The label of a pad at the font size `size`: the best label that fits
/// at this size. If none fits, the best label that fits at a smaller size.
pub fn fit(pad: PadView, number: &str, name: Option<&str>, advance: f32, size: f32) -> Option<Fit> {
    let c = choices(pad, number, name, advance);
    let size = size.min(MAX_SIZE);
    let pick = c
        .iter()
        .find(|f| f.size >= size * 0.98)
        .or_else(|| c.first())?;
    Some(Fit {
        size: pick.size.min(size),
        ..pick.clone()
    })
}

/// The same direction, turned so that the text is not upside down. A
/// vertical label reads from the bottom to the top.
pub fn upright(angle: f32) -> f32 {
    let mut a = (angle + PI).rem_euclid(2.0 * PI) - PI;
    // Snap angles that are almost square, so labels do not tilt by a hair.
    for q in [-PI, -FRAC_PI_2, 0.0, FRAC_PI_2, PI] {
        if (a - q).abs() < 1e-3 {
            a = q;
        }
    }
    if a >= FRAC_PI_2 {
        a -= PI;
    } else if a < -FRAC_PI_2 {
        a += PI;
    }
    a
}

#[cfg(test)]
mod tests {
    use super::*;

    const ADV: f32 = 0.6;

    /// The label at the comfortable size, as the viewer draws it.
    fn best(p: PadView, number: &str, name: Option<&str>) -> Option<Fit> {
        fit(p, number, name, ADV, comfortable(p, number, name, ADV)?)
    }

    fn pad(x: f32, y: f32) -> PadView {
        PadView {
            along_x: x,
            along_y: y,
            x_angle: 0.0,
        }
    }

    #[test]
    fn a_tall_pad_reads_from_the_bottom_up() {
        let f = best(pad(40.0, 160.0), "42", Some("P1.14/AIN7")).unwrap();
        assert!((f.angle + FRAC_PI_2).abs() < 1e-6, "{}", f.angle);
        assert_eq!(f.name.as_deref(), Some("P1.14/AIN7"));
    }

    #[test]
    fn a_wide_pad_reads_level() {
        let f = best(pad(160.0, 40.0), "1", Some("P1.00/XL1")).unwrap();
        assert_eq!(f.angle, 0.0);
    }

    #[test]
    fn the_label_stays_inside_the_pad() {
        for (w, h) in [(30.0, 120.0), (60.0, 60.0), (120.0, 25.0), (20.0, 300.0)] {
            let Some(f) = best(pad(w, h), "42", Some("P1.14/AIN7")) else {
                continue;
            };
            let (long, short) = if h > w { (h, w) } else { (w, h) };
            let text = match f.arrange {
                Arrange::Stacked => f.name.as_ref().unwrap().len().max(2) as f32,
                Arrange::Inline => 3.0 + f.name.as_ref().unwrap().len() as f32,
                Arrange::Number => 2.0,
            };
            assert!(text * ADV * f.size <= long, "{w}x{h}: {f:?}");
            let lines = if f.arrange == Arrange::Stacked {
                2.0
            } else {
                1.0
            };
            assert!(lines * LINE * f.size <= short, "{w}x{h}: {f:?}");
        }
    }

    #[test]
    fn a_narrow_pad_drops_the_second_name() {
        // Room for `P1.14`, not for `P1.14/AIN7`.
        let f = best(pad(14.0, 60.0), "42", Some("P1.14/AIN7")).unwrap();
        assert_eq!(f.name.as_deref(), Some("P1.14"), "{f:?}");
    }

    #[test]
    fn a_small_pad_shows_the_number_only() {
        let f = best(pad(12.0, 30.0), "42", Some("P1.14/AIN7")).unwrap();
        assert_eq!(f.arrange, Arrange::Number);
        assert!(best(pad(5.0, 5.0), "42", None).is_none());
    }

    #[test]
    fn a_big_pad_does_not_get_huge_text() {
        let f = best(pad(400.0, 400.0), "49", Some("VSS")).unwrap();
        assert_eq!(f.size, MAX_SIZE);
    }

    #[test]
    fn pads_of_one_size_share_a_size() {
        let p = pad(20.0, 100.0);
        let long = comfortable(p, "42", Some("P1.14/AIN7"), ADV).unwrap();
        let short = comfortable(p, "45", Some("DECD"), ADV).unwrap();
        let size = long.min(short);
        let a = fit(p, "45", Some("DECD"), ADV, size).unwrap();
        let b = fit(p, "42", Some("P1.14/AIN7"), ADV, size).unwrap();
        assert_eq!(a.size, b.size);
        assert_eq!(a.name.as_deref(), Some("DECD"));
    }

    #[test]
    fn a_long_name_gives_way_to_its_first_part() {
        // At the size of its neighbours, `P1.00/XL1` does not fit; `P1.00` does.
        let p = pad(18.0, 75.0);
        let size = comfortable(p, "9", Some("VDD"), ADV).unwrap();
        let f = fit(p, "1", Some("P1.00/XL1"), ADV, size).unwrap();
        assert_eq!(f.name.as_deref(), Some("P1.00"), "{f:?}");
        assert_eq!(f.size, size);
    }

    #[test]
    fn upright_angles() {
        assert_eq!(upright(0.0), 0.0);
        assert_eq!(upright(PI), 0.0);
        assert_eq!(upright(FRAC_PI_2), -FRAC_PI_2);
        assert_eq!(upright(-FRAC_PI_2), -FRAC_PI_2);
        assert!((upright(3.0 * PI / 4.0) + PI / 4.0).abs() < 1e-6);
    }
}
