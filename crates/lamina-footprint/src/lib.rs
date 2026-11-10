//! Footprint generation: IPC-7351 packages and datasheet pad lists.

pub mod build;
pub mod compare;
pub mod ipc;
pub mod presets;

pub use build::{Options, PackageSpec, generate};
pub use ipc::{Density, Dim};

use lamina_model::*;

/// A file-safe id for a package name and density, for example `R_0603_B`.
pub fn default_id(package: &str, density: Density) -> String {
    let mut id: String = package
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '.' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect();
    if density != Density::Nominal {
        id.push('_');
        id.push_str(density.letter());
    }
    id
}

/// Generates a footprint for a named package (see [`presets::EXAMPLES`]).
pub fn from_package(package: &str, opt: &Options, id: Option<&str>) -> Result<Footprint, String> {
    let spec = presets::resolve(package)?;
    let id = id
        .map(str::to_string)
        .unwrap_or_else(|| default_id(package, opt.density));
    generate(&id, package, &spec, opt)
}

/// Completes a footprint typed in from a datasheet. It checks the pads. If
/// there is no courtyard, it adds a courtyard and silkscreen.
pub fn from_pads(
    mut fp: Footprint,
    body: Option<(f64, f64)>,
    opt: &Options,
) -> Result<Footprint, String> {
    if !valid_id(&fp.id) {
        return Err(format!(
            "The footprint id `{}` is not valid. Use only letters, digits, . _ -",
            fp.id
        ));
    }
    if fp.pads.is_empty() {
        return Err("The footprint has no pads.".into());
    }
    for p in &fp.pads {
        p.shape
            .check()
            .map_err(|e| format!("pad {}: {e}", p.number))?;
        if !p.at.is_finite() || !p.rotation.is_finite() {
            return Err(format!("pad {}: the position is not finite.", p.number));
        }
        if let PadKind::Through { drill, .. } = &p.kind {
            drill
                .check()
                .map_err(|e| format!("pad {}: {e}", p.number))?;
            let (w, h) = p.shape.size();
            let need = match drill {
                Drill::Round { diameter } => *diameter,
                Drill::Slot { width, height } => width.min(*height),
            };
            if w.min(h) <= need {
                return Err(format!(
                    "pad {}: the copper is not larger than the drill. Make the pad larger or the drill smaller.",
                    p.number
                ));
            }
        }
    }
    let has_courtyard = fp.graphics.iter().any(|g| g.layer == FpLayer::Courtyard);
    if !has_courtyard {
        let pads = fp.pads_bbox().unwrap();
        let body = match body {
            Some((w, h)) => Rect {
                min: Point::new(-w / 2.0, -h / 2.0),
                max: Point::new(w / 2.0, h / 2.0),
            },
            None => pads,
        };
        let height = fp.height.unwrap_or(1.0);
        let keep_texts = !fp.texts.is_empty();
        let texts = std::mem::take(&mut fp.texts);
        build::decorate(&mut fp, body, height, None, opt);
        if keep_texts {
            fp.texts = texts;
        }
    }
    Ok(fp)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pad<'a>(fp: &'a Footprint, n: &str) -> &'a Pad {
        fp.pad(n).unwrap()
    }

    #[test]
    fn r0603_nominal() {
        let fp = from_package("R_0603", &Options::default(), None).unwrap();
        assert_eq!(fp.id, "R_0603");
        assert_eq!(fp.pads.len(), 2);
        let (w, h) = pad(&fp, "1").shape.size();
        assert!((w - 0.82).abs() < 0.06, "{w}");
        assert!((h - 0.96).abs() < 0.03, "{h}");
        assert!(
            (pad(&fp, "2").at.x - 0.82).abs() < 0.04,
            "{:?}",
            pad(&fp, "2").at
        );
        assert!(fp.courtyard_bbox().unwrap().width() > 2.4);
    }

    #[test]
    fn soic8_layout() {
        let fp = from_package("SOIC-8", &Options::default(), None).unwrap();
        assert_eq!(fp.pads.len(), 8);
        // Pin 1 is top-left, pin 4 bottom-left, pin 5 bottom-right, pin 8 top-right.
        let p1 = pad(&fp, "1").at;
        let p4 = pad(&fp, "4").at;
        let p5 = pad(&fp, "5").at;
        let p8 = pad(&fp, "8").at;
        assert!(p1.x < 0.0 && p1.y > 0.0);
        assert!((p1.y - 1.905).abs() < 1e-9 && (p4.y + 1.905).abs() < 1e-9);
        assert!(p5.x > 0.0 && p5.y < 0.0 && p8.y > 0.0);
        // A usual SOIC-8 land: 1.95 x 0.6 mm pads, with centres 2.475 mm from the axis.
        let (len, wid) = pad(&fp, "1").shape.size();
        assert!((1.4..2.1).contains(&len), "{len}");
        assert!((0.55..0.7).contains(&wid), "{wid}");
        assert!((2.4..2.6).contains(&p8.x), "{}", p8.x);
    }

    #[test]
    fn tssop28_fine_pitch_gap() {
        let fp = from_package("TSSOP-28", &Options::default(), None).unwrap();
        assert_eq!(fp.pads.len(), 28);
        let (_, w) = pad(&fp, "1").shape.size();
        assert!(
            w <= 0.45 + 1e-9,
            "The pad width {w} gives a gap of less than 0.2 mm."
        );
        let p14 = pad(&fp, "14").at;
        let p15 = pad(&fp, "15").at;
        assert!(
            (p14.y - p15.y).abs() < 1e-9,
            "14 and 15 must be opposite each other."
        );
    }

    #[test]
    fn sot23_and_sot223() {
        let fp = from_package("SOT-23", &Options::default(), None).unwrap();
        let p3 = pad(&fp, "3").at;
        assert!(p3.x > 0.0 && p3.y.abs() < 1e-9);
        let fp = from_package("SOT-223", &Options::default(), None).unwrap();
        let (_, tab) = pad(&fp, "4").shape.size();
        let (_, lead) = pad(&fp, "1").shape.size();
        assert!(tab > 3.0 && lead < 1.2, "{tab} {lead}");
    }

    #[test]
    fn qfn_with_ep() {
        let fp = from_package("QFN-32_5x5_P0.5_EP3.3x3.3", &Options::default(), None).unwrap();
        assert_eq!(fp.pads.len(), 33);
        let ep = pad(&fp, "33");
        assert_eq!(ep.at, Point::new(0.0, 0.0));
        assert!(ep.paste_margin.unwrap() < 0.0);
        // Pin 1 is on the left side, at the top. Pin 9 is on the bottom side, at the left.
        assert!(pad(&fp, "1").at.x < -2.0 && pad(&fp, "1").at.y > 1.5);
        assert!(pad(&fp, "9").at.y < -2.0 && pad(&fp, "9").at.x < -1.5);
    }

    #[test]
    fn lqfp48() {
        let fp = from_package("LQFP-48_7x7_P0.5", &Options::default(), None).unwrap();
        assert_eq!(fp.pads.len(), 48);
        let span = pad(&fp, "37").at.y - pad(&fp, "13").at.y;
        assert!((span - pad(&fp, "36").at.x * 2.0).abs() < 0.2);
        assert!(span > 8.0 && span < 9.0, "{span}");
    }

    #[test]
    fn pin_header() {
        let fp = from_package("PinHeader_1x03_P2.54", &Options::default(), None).unwrap();
        assert_eq!(fp.pads.len(), 3);
        assert!(pad(&fp, "1").at.y > pad(&fp, "2").at.y);
        assert!(matches!(pad(&fp, "1").kind, PadKind::Through { .. }));
        let fp = from_package("PinHeader_2x05_P1.27", &Options::default(), None).unwrap();
        assert_eq!(fp.pads.len(), 10);
        assert!(pad(&fp, "2").at.x > pad(&fp, "1").at.x);
        assert!((pad(&fp, "2").at.y - pad(&fp, "1").at.y).abs() < 1e-9);
    }

    #[test]
    fn silk_clear_of_pads() {
        for name in ["R_0603", "SOIC-8", "TSSOP-28", "QFN-32_5x5_P0.5", "SOT-23"] {
            let fp = from_package(name, &Options::default(), None).unwrap();
            for g in fp.graphics.iter().filter(|g| g.layer == FpLayer::Silk) {
                if let Shape::Line { a, b } = g.shape {
                    for p in &fp.pads {
                        let r = p.bbox();
                        let n = 20;
                        for k in 0..=n {
                            let t = k as f64 / n as f64;
                            let q = Point::new(a.x + (b.x - a.x) * t, a.y + (b.y - a.y) * t);
                            let inside = q.x > r.min.x - 0.15
                                && q.x < r.max.x + 0.15
                                && q.y > r.min.y - 0.15
                                && q.y < r.max.y + 0.15;
                            assert!(!inside, "{name}: the silk touches pad {}", p.number);
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn all_examples_generate() {
        for name in presets::EXAMPLES {
            let fp = from_package(name, &Options::default(), None)
                .unwrap_or_else(|e| panic!("{name}: {e}"));
            assert!(valid_id(&fp.id), "{}", fp.id);
            for p in &fp.pads {
                p.shape.check().unwrap();
            }
        }
        assert!(from_package("XYZ-9", &Options::default(), None).is_err());
    }

    #[test]
    fn datasheet_pads() {
        let mut fp = Footprint::new(
            "KAGA_TEST",
            Provenance::Datasheet {
                document: "test".into(),
                page: Some(22),
                url: None,
            },
        );
        fp.pads.push(Pad::smd(
            "1",
            Point::new(-1.0, 0.0),
            PadShape::Rect {
                width: 0.5,
                height: 0.9,
            },
        ));
        fp.pads.push(Pad::smd(
            "2",
            Point::new(1.0, 0.0),
            PadShape::Rect {
                width: 0.5,
                height: 0.9,
            },
        ));
        let fp = from_pads(fp, Some((3.0, 2.0)), &Options::default()).unwrap();
        assert!(fp.graphics.iter().any(|g| g.layer == FpLayer::Courtyard));

        let mut bad = Footprint::new(
            "B",
            Provenance::Manual {
                note: String::new(),
            },
        );
        bad.pads.push(Pad::through(
            "1",
            Point::new(0.0, 0.0),
            PadShape::Circle { diameter: 0.8 },
            Drill::Round { diameter: 1.0 },
        ));
        assert!(from_pads(bad, None, &Options::default()).is_err());
    }
}
