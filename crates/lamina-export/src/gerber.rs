//! Gerber X2 (RS-274X with attributes) writer.

use std::collections::BTreeMap;
use std::fmt::Write;

use lamina_model::Point;
use lamina_scene::{Geom, Tag, TagFunction};

/// The Gerber file function attribute, for example `Copper,L1,Top`.
pub struct Function {
    pub function: String,
    /// `Positive` or `Negative`.
    pub polarity: &'static str,
}

fn coord(v: f64) -> i64 {
    (v * 1e6).round() as i64
}

pub struct Writer {
    body: String,
    /// An aperture for each diameter and function.
    apertures: BTreeMap<(i64, Option<TagFunction>), u32>,
    next: u32,
    current: Option<u32>,
    dark: bool,
    /// The function for the apertures of the item in work.
    function: Option<TagFunction>,
    /// The object attributes that are in effect now.
    object: Option<Tag>,
}

/// A name for an attribute field. The characters that have a function in a
/// Gerber file become `_`.
fn field(s: &str) -> String {
    s.chars()
        .map(|c| {
            if c.is_ascii_graphic() && !matches!(c, ',' | '*' | '%' | '\\') {
                c
            } else {
                '_'
            }
        })
        .collect()
}

fn function_name(f: TagFunction) -> &'static str {
    match f {
        TagFunction::Conductor => "Conductor",
        TagFunction::SmdPad => "SMDPad,CuDef",
        TagFunction::ComponentPad => "ComponentPad",
        TagFunction::ViaPad => "ViaPad",
    }
}

impl Default for Writer {
    fn default() -> Self {
        Self::new()
    }
}

impl Writer {
    pub fn new() -> Writer {
        Writer {
            body: String::new(),
            apertures: BTreeMap::new(),
            next: 10,
            current: None,
            dark: true,
            function: None,
            object: None,
        }
    }

    fn circle(&mut self, d: f64) -> u32 {
        let key = (coord(d.max(0.001)), self.function);
        let next = &mut self.next;
        *self.apertures.entry(key).or_insert_with(|| {
            let n = *next;
            *next += 1;
            n
        })
    }

    fn select(&mut self, ap: u32) {
        if self.current != Some(ap) {
            let _ = writeln!(self.body, "D{ap}*");
            self.current = Some(ap);
        }
    }

    fn polarity(&mut self, dark: bool) {
        if self.dark != dark {
            self.body
                .push_str(if dark { "%LPD*%\n" } else { "%LPC*%\n" });
            self.dark = dark;
        }
    }

    fn mv(&mut self, p: Point, op: &str) {
        let _ = writeln!(self.body, "X{}Y{}{op}*", coord(p.x), coord(p.y));
    }

    fn region(&mut self, ring: &[Point]) {
        if ring.len() < 3 {
            return;
        }
        self.body.push_str("G36*\n");
        self.mv(ring[0], "D02");
        for p in &ring[1..] {
            self.mv(*p, "D01");
        }
        self.mv(ring[0], "D01");
        self.body.push_str("G37*\n");
    }

    /// Add a copper item with its attributes: the net, the part and the
    /// pad for the object, and the function for its aperture (Gerber X2).
    pub fn add_tagged(&mut self, g: &Geom, tag: Option<&Tag>) {
        if self.object.as_ref() != tag {
            if self.object.is_some() {
                self.body.push_str("%TD*%\n");
            }
            if let Some(t) = tag {
                if let (Some(part), Some(pad)) = (&t.part, &t.pad) {
                    let _ = writeln!(self.body, "%TO.P,{},{}*%", field(part), field(pad));
                    let _ = writeln!(self.body, "%TO.C,{}*%", field(part));
                }
                match &t.net {
                    Some(net) => {
                        let _ = writeln!(self.body, "%TO.N,{}*%", field(net));
                    }
                    // A pad with no net.
                    None if t.pad.is_some() => self.body.push_str("%TO.N,N/C*%\n"),
                    None => {}
                }
            }
            self.object = tag.cloned();
        }
        self.function = tag.map(|t| t.function);
        self.add(g);
        self.function = None;
    }

    /// Add a shape in dark polarity. Then clear the polygon holes.
    pub fn add(&mut self, g: &Geom) {
        match g {
            Geom::Disc { c, r } => {
                let ap = self.circle(2.0 * r);
                self.polarity(true);
                self.select(ap);
                self.mv(*c, "D03");
            }
            Geom::Stroke { pts, width, closed } => {
                if pts.is_empty() {
                    return;
                }
                let ap = self.circle(*width);
                self.polarity(true);
                self.select(ap);
                if pts.len() == 1 {
                    self.mv(pts[0], "D03");
                    return;
                }
                self.mv(pts[0], "D02");
                for p in &pts[1..] {
                    self.mv(*p, "D01");
                }
                if *closed {
                    self.mv(pts[0], "D01");
                }
            }
            Geom::Poly { outer, holes } => {
                self.polarity(true);
                self.body.push_str("G01*\n");
                self.region(outer);
                if !holes.is_empty() {
                    self.polarity(false);
                    for h in holes {
                        self.region(h);
                    }
                    self.polarity(true);
                }
            }
        }
    }

    pub fn finish(self, f: &Function) -> String {
        let mut out = String::new();
        out.push_str("%TF.GenerationSoftware,Lamina,lamina,");
        out.push_str(env!("CARGO_PKG_VERSION"));
        out.push_str("*%\n");
        out.push_str("%TF.SameCoordinates,Original*%\n");
        let _ = writeln!(out, "%TF.FileFunction,{}*%", f.function);
        let _ = writeln!(out, "%TF.FilePolarity,{}*%", f.polarity);
        out.push_str("%FSLAX46Y46*%\n%MOMM*%\n%LPD*%\nG01*\n");
        for ((size, function), n) in &self.apertures {
            if let Some(f) = function {
                let _ = writeln!(out, "%TA.AperFunction,{}*%", function_name(*f));
            }
            let _ = writeln!(out, "%ADD{n}C,{:.6}*%", *size as f64 / 1e6);
            if function.is_some() {
                out.push_str("%TD*%\n");
            }
        }
        out.push_str(&self.body);
        out.push_str("M02*\n");
        out
    }
}
