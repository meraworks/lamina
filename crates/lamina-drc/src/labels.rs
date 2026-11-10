//! Checks of what the silkscreen says: a label far from its part, pin
//! labels in the wrong position, a polarity mark at the wrong pad, and a
//! text that a part covers. These checks guess, so they are warnings.

use lamina_geom::Prim;
use lamina_model::*;

use crate::r3;

struct PlacedPad {
    number: String,
    net: Option<String>,
    at: Point,
}

struct PlacedPart<'a> {
    refdes: &'a str,
    side: Side,
    value: &'a str,
    courtyard: Option<Vec<Point>>,
    body: Option<Vec<Point>>,
    pads: Vec<PlacedPad>,
    through: bool,
}

/// A word of a text: the word and the board position of its centre.
struct Word {
    text: String,
    at: Point,
}

fn poly(p: &[Point]) -> Prim {
    Prim::Poly {
        outer: p.to_vec(),
        holes: vec![],
    }
}

/// The board position of a point on the baseline of a text, `x` mm from its centre.
fn on_baseline(t: &BoardText, x: f64) -> Point {
    let p = Point::new(x, 0.0);
    if t.layer.side() == Some(Side::Bottom) {
        let q = p.rotated(-t.rotation);
        Point::new(t.at.x - q.x, t.at.y + q.y)
    } else {
        p.rotated(t.rotation).add(t.at)
    }
}

/// The font has one width for all characters: 5/6 of the size.
fn words(t: &BoardText) -> Vec<Word> {
    let s = t.size / 6.0;
    let chars: Vec<char> = t.text.chars().collect();
    let width = (chars.len() as f64 * 5.0 - 1.0) * s;
    let mut out = Vec::new();
    let mut start: Option<usize> = None;
    for i in 0..=chars.len() {
        let space = i == chars.len() || chars[i].is_whitespace();
        match (start, space) {
            (None, false) => start = Some(i),
            (Some(a), true) => {
                let centre = ((a + i - 1) as f64 / 2.0 * 5.0 + 2.0) * s - width / 2.0;
                out.push(Word {
                    text: chars[a..i].iter().collect(),
                    at: on_baseline(t, centre),
                });
                start = None;
            }
            _ => {}
        }
    }
    out
}

fn text_box(t: &BoardText) -> Prim {
    let half = lamina_geom::font::text_width(&t.text, t.size) / 2.0;
    Prim::Capsule {
        a: on_baseline(t, -half),
        b: on_baseline(t, half),
        r: t.size / 2.0,
    }
}

fn is_connector(refdes: &str) -> bool {
    let letters: String = refdes
        .chars()
        .take_while(|c| c.is_ascii_alphabetic())
        .collect();
    matches!(letters.as_str(), "J" | "P" | "CN" | "X" | "TB" | "BT")
}

/// +1 for a positive net, -1 for a negative net, 0 if the name does not say.
fn polarity(net: &str) -> i8 {
    let up = net.to_uppercase();
    if up.ends_with('+') || up.ends_with("_P") {
        return 1;
    }
    if up.ends_with('-') || up.ends_with("_N") {
        return -1;
    }
    match pin_role(net) {
        PinRole::Power => 1,
        PinRole::Ground => -1,
        _ => 0,
    }
}

/// The net of the part that a word names, if it names one.
fn net_for<'a>(word: &str, part: &'a PlacedPart) -> Option<&'a str> {
    let w = word.to_uppercase();
    if matches!(w.as_str(), "TX" | "RX" | "MISO" | "MOSI" | "TXD" | "RXD") {
        return None;
    }
    let nets: Vec<&str> = part.pads.iter().filter_map(|p| p.net.as_deref()).collect();
    // A short pin name can be the end of a net name: SDA for I2C_SDA. A
    // long word at the end of a net name is a function, not a pin: SERVO.
    let suffix = w.chars().count() <= 4;
    if let Some(n) = nets
        .iter()
        .find(|n| n.to_uppercase() == w || (suffix && n.to_uppercase().ends_with(&format!("_{w}"))))
    {
        return Some(n);
    }
    let only = |role: PinRole| {
        let mut found: Vec<&str> = nets
            .iter()
            .copied()
            .filter(|n| pin_role(n) == role)
            .collect();
        found.sort();
        found.dedup();
        (found.len() == 1).then(|| found[0])
    };
    match w.as_str() {
        "VCC" | "VDD" | "VIN" => only(PinRole::Power),
        "GND" | "VSS" => only(PinRole::Ground),
        _ => None,
    }
}

fn pitch(part: &PlacedPart) -> f64 {
    let mut best = f64::INFINITY;
    for (i, a) in part.pads.iter().enumerate() {
        for b in &part.pads[i + 1..] {
            let d = a.at.dist(b.at);
            if d > 1e-6 {
                best = best.min(d);
            }
        }
    }
    if best.is_finite() { best } else { 2.54 }
}

pub(crate) fn check(d: &ProjectData) -> Vec<Violation> {
    let nets = d.nets.pad_map();
    let layers = d.board.stackup.copper_layers;
    let mut parts: Vec<PlacedPart> = Vec::new();
    for (refdes, pl) in &d.placement.placements {
        let Some(part) = d.parts.parts.get(refdes) else {
            continue;
        };
        let Some(fp) = d.footprints.get(&part.footprint) else {
            continue;
        };
        let _ = layers;
        parts.push(PlacedPart {
            refdes,
            side: pl.side,
            value: part.value.as_deref().unwrap_or(""),
            courtyard: lamina_geom::courtyard(fp, pl),
            body: fp.body_bbox().map(|b| {
                [
                    b.min,
                    Point::new(b.max.x, b.min.y),
                    b.max,
                    Point::new(b.min.x, b.max.y),
                ]
                .iter()
                .map(|p| pl.transform(*p))
                .collect()
            }),
            pads: fp
                .pads
                .iter()
                .filter(|p| !p.number.is_empty())
                .map(|p| PlacedPad {
                    number: p.number.clone(),
                    net: nets
                        .get(&PadRef::new(refdes.clone(), p.number.clone()))
                        .cloned(),
                    at: pl.transform(p.at),
                })
                .collect(),
            through: fp.pads.iter().any(|p| !p.is_smd()),
        });
    }
    let max = d.board.rules.label_max_distance;
    let mut v = Vec::new();
    for t in &d.routing.texts {
        let Layer::Silk(side) = t.layer else { continue };
        let tbox = text_box(t);
        let all = words(t);
        let layer = t.layer;
        // A part can have a label on its side, and a through-hole part on the two sides.
        let dist_to = |p: &PlacedPart| {
            p.courtyard
                .as_ref()
                .map(|c| tbox.distance(&poly(c)))
                .unwrap_or(f64::INFINITY)
        };

        // A text that a part covers.
        if let Some(p) = parts.iter().find(|p| {
            p.side == side
                && p.body
                    .as_ref()
                    .is_some_and(|b| lamina_geom::prim::ring_contains(b, t.at))
        }) {
            v.push(
                Violation::warning(
                    "silk_under_part",
                    format!(
                        "The text \"{}\" is below the body of {}. The part hides it",
                        t.text, p.refdes
                    ),
                )
                .at(t.at)
                .on(layer)
                .items([t.id.clone(), p.refdes.to_string()]),
            );
        }

        // A label that names a part.
        if let Some(first) = all.first() {
            let name = first.text.trim_end_matches(':');
            if let Some(own) = parts.iter().find(|p| p.refdes == name) {
                let d_own = dist_to(own);
                let other = parts
                    .iter()
                    .filter(|p| p.refdes != own.refdes)
                    .map(|p| (dist_to(p), p))
                    .min_by(|a, b| a.0.total_cmp(&b.0));
                let inside = other.as_ref().is_some_and(|(dd, _)| *dd <= 1e-6) && d_own > 1e-6;
                let nearer = other
                    .as_ref()
                    .is_some_and(|(dd, _)| d_own > 2.0 && dd + 1.5 < d_own);
                if d_own > max || inside || nearer {
                    let near = other.map(|(_, p)| p.refdes).unwrap_or("no part");
                    v.push(
                        Violation::warning(
                            "label_far",
                            format!(
                                "The text \"{}\" names {}, but it is {} mm from that part and nearer to {near}",
                                t.text,
                                own.refdes,
                                r3(d_own)
                            ),
                        )
                        .at(t.at)
                        .on(layer)
                        .items([t.id.clone(), own.refdes.to_string()])
                        .measured(d_own, max),
                    );
                }
            }
        }

        // The nearest part with two or more pads, for the pin and polarity checks.
        // A connector wins: its labels are frequently nearer to a small
        // part beside it than to its own outline.
        let pick = |connector: bool, reach: f64| {
            parts
                .iter()
                .filter(|p| {
                    p.pads.len() >= 2
                        && (p.side == side || p.through)
                        && is_connector(p.refdes) == connector
                })
                .map(|p| (dist_to(p), p))
                .filter(|(dd, _)| *dd <= reach)
                .min_by(|a, b| a.0.total_cmp(&b.0))
                .map(|(_, p)| p)
        };
        let near = pick(true, 3.5).or_else(|| pick(false, 2.5));
        let Some(part) = near else { continue };
        let step = pitch(part);
        // A label is beside the pad row, not on it. Measure along the row:
        // the distance across the row counts for one fifth.
        let row = {
            let (a, b) = (part.pads[0].at, part.pads[part.pads.len() - 1].at);
            let v = b.sub(a);
            let len = v.x.hypot(v.y);
            if len > 1e-6 {
                Point::new(v.x / len, v.y / len)
            } else {
                Point::new(1.0, 0.0)
            }
        };
        let row_dist = move |at: Point, pad: Point| {
            let v = at.sub(pad);
            let along = v.x * row.x + v.y * row.y;
            let across = -v.x * row.y + v.y * row.x;
            along.hypot(0.2 * across)
        };
        let nearest_pad = |at: Point, keep: &dyn Fn(&PlacedPad) -> bool| {
            part.pads
                .iter()
                .filter(|p| keep(p))
                .map(|p| (row_dist(at, p.at), p))
                .min_by(|a, b| a.0.total_cmp(&b.0))
        };

        // Polarity marks.
        let marks: Vec<(&Word, i8)> = all
            .iter()
            .filter_map(|w| {
                if w.text.ends_with('+') {
                    Some((w, 1))
                } else if w.text.ends_with('-') && w.text != "--" {
                    Some((w, -1))
                } else {
                    None
                }
            })
            .collect();
        let has_plus = marks.iter().any(|(_, s)| *s == 1);
        // A text with two or more numbers is a legend, not a mark at a pad.
        let numbers = all.iter().filter(|w| w.text.parse::<u32>().is_ok()).count();
        let marks = if numbers >= 2 { Vec::new() } else { marks };
        let polar = |p: &PlacedPad| p.net.as_deref().is_some_and(|n| polarity(n) != 0);
        for (w, sign) in &marks {
            // A `-` alone can be a dash.
            if *sign == -1 && !has_plus {
                continue;
            }
            let Some((d_near, pad)) = nearest_pad(w.at, &polar) else {
                continue;
            };
            let found = polarity(pad.net.as_deref().unwrap_or(""));
            if found == *sign {
                continue;
            }
            let good = nearest_pad(w.at, &|p: &PlacedPad| {
                p.net.as_deref().is_some_and(|n| polarity(n) == *sign)
            });
            if let Some((d_good, right)) = good {
                if d_good > d_near + 0.25 * step {
                    v.push(
                        Violation::warning(
                            "polarity_mark",
                            format!(
                                "The mark \"{}\" in the text \"{}\" is nearest to {}.{} ({}), but the pad for it is {}.{} ({})",
                                w.text,
                                t.text,
                                part.refdes,
                                pad.number,
                                pad.net.as_deref().unwrap_or(""),
                                part.refdes,
                                right.number,
                                right.net.as_deref().unwrap_or("")
                            ),
                        )
                        .at(w.at)
                        .on(layer)
                        .items([t.id.clone(), part.refdes.to_string()]),
                    );
                }
            }
        }

        // Pin labels, for connectors only.
        if !is_connector(part.refdes) {
            continue;
        }
        let named: Vec<&Word> = all
            .iter()
            .filter(|w| {
                w.text.chars().count() > 1
                    && w.text.parse::<f64>().is_err()
                    && !w.text.eq_ignore_ascii_case(part.value)
            })
            .collect();
        // A legend: a pad number before each name ("1 VCC 2 GND").
        let mut legend: Vec<(&str, &Word)> = Vec::new();
        for pair in all.windows(2) {
            if part.pads.iter().any(|p| p.number == pair[0].text)
                && pair[0].text.parse::<u32>().is_ok()
                && pair[1].text.parse::<f64>().is_err()
            {
                legend.push((pair[0].text.as_str(), &pair[1]));
            }
        }
        if legend.len() >= 2 {
            for (number, w) in legend {
                let Some(net) = net_for(&w.text, part) else {
                    continue;
                };
                let pad_net = part
                    .pads
                    .iter()
                    .find(|p| p.number == number)
                    .and_then(|p| p.net.as_deref());
                if pad_net != Some(net) {
                    v.push(
                        Violation::warning(
                            "label_pin",
                            format!(
                                "The text \"{}\" gives {} for pad {number} of {}, but that pad is on {}",
                                t.text,
                                w.text,
                                part.refdes,
                                pad_net.unwrap_or("no net")
                            ),
                        )
                        .at(w.at)
                        .on(layer)
                        .items([t.id.clone(), part.refdes.to_string()]),
                    );
                }
            }
            continue;
        }
        let matched: Vec<(&Word, &str)> = named
            .iter()
            .filter_map(|w| net_for(&w.text, part).map(|n| (*w, n)))
            .collect();
        // The middle of the pads of a net.
        let centre_of = |net: &str| {
            let pts: Vec<Point> = part
                .pads
                .iter()
                .filter(|p| p.net.as_deref() == Some(net))
                .map(|p| p.at)
                .collect();
            let n = pts.len().max(1) as f64;
            Point::new(
                pts.iter().map(|p| p.x).sum::<f64>() / n,
                pts.iter().map(|p| p.y).sum::<f64>() / n,
            )
        };
        if matched.len() >= 2 {
            // The order of the words along the text against the order of their pads.
            let axis = matched[matched.len() - 1].0.at.sub(matched[0].0.at);
            let len = axis.x.hypot(axis.y);
            if len < 1e-6 {
                continue;
            }
            let along = |p: Point| (p.x * axis.x + p.y * axis.y) / len;
            let (mut same, mut reverse) = (0, 0);
            for (i, a) in matched.iter().enumerate() {
                for b in &matched[i + 1..] {
                    if a.1 == b.1 {
                        continue;
                    }
                    let words = along(b.0.at) - along(a.0.at);
                    let pads = along(centre_of(b.1)) - along(centre_of(a.1));
                    if pads.abs() < 0.25 * step {
                        continue;
                    }
                    if words * pads > 0.0 {
                        same += 1;
                    } else {
                        reverse += 1;
                    }
                }
            }
            if reverse > 0 && same == 0 {
                v.push(
                    Violation::warning(
                        "label_pin",
                        format!(
                            "The labels in the text \"{}\" are in the reverse order of the pins of {}",
                            t.text, part.refdes
                        ),
                    )
                    .at(t.at)
                    .on(layer)
                    .items([t.id.clone(), part.refdes.to_string()])
                    .fix(Fix::TurnText {
                        id: t.id.clone(),
                        by: 180.0,
                    }),
                );
            }
        } else if let [(w, net)] = matched[..] {
            let Some((d_near, pad)) = nearest_pad(w.at, &|_| true) else {
                continue;
            };
            if pad.net.as_deref() == Some(net) {
                continue;
            }
            let good = nearest_pad(w.at, &|p: &PlacedPad| p.net.as_deref() == Some(net));
            if let Some((d_good, right)) = good {
                if d_good > d_near + 0.5 * step {
                    v.push(
                        Violation::warning(
                            "label_pin",
                            format!(
                                "The label \"{}\" is at pad {}.{} ({}), but the pad of that net is {}.{}",
                                w.text,
                                part.refdes,
                                pad.number,
                                pad.net.as_deref().unwrap_or("no net"),
                                part.refdes,
                                right.number
                            ),
                        )
                        .at(w.at)
                        .on(layer)
                        .items([t.id.clone(), part.refdes.to_string()]),
                    );
                }
            }
        }
    }
    v
}
