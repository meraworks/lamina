//! IPC-D-356A electrical test netlist, fixed-width ASCII and decimils.
use crate::Output;
use lamina_model::*;
use std::collections::{BTreeMap, BTreeSet};

struct Record {
    net: Option<String>,
    reference: String,
    pin: String,
    code: u16,
    at: Point,
    width: f64,
    height: f64,
    rotation: f64,
    drill: Option<(f64, bool)>,
    access: u8,
    mask: u8,
    span: Option<(u8, u8)>,
}

fn ascii(text: &str) -> String {
    text.chars()
        .map(|c| if c.is_ascii_graphic() { c } else { '_' })
        .collect()
}

fn field(text: &str, width: usize) -> Result<String, String> {
    let text = ascii(text);
    if text.len() > width {
        return Err(format!(
            "IPC-D-356 field `{text}` exceeds {width} characters."
        ));
    }
    Ok(format!("{text:<width$}"))
}

fn dimension(mm: f64, limit: i64) -> Result<i64, String> {
    let value = (mm / 0.00254).round();
    if !value.is_finite() || value.abs() > limit as f64 {
        return Err(format!(
            "IPC-D-356 dimension {mm} mm is outside its fixed-width range."
        ));
    }
    Ok(value as i64)
}

fn line(out: &mut String, value: &str) -> Result<(), String> {
    if !value.is_ascii() || value.len() > 80 {
        return Err("IPC-D-356 record exceeds 80 ASCII columns.".into());
    }
    out.push_str(&format!("{value:<80}\n"));
    Ok(())
}

impl Record {
    fn component(&self, code: u16, net: &str, midpoint: bool) -> Result<String, String> {
        Ok(format!(
            "{code:03}{}   {}{}{}{}",
            field(net, 14)?,
            field(&self.reference, 6)?,
            if self.pin.is_empty() { ' ' } else { '-' },
            field(&self.pin, 4)?,
            if midpoint { 'M' } else { ' ' }
        ))
    }
    fn location(&self) -> Result<String, String> {
        Ok(format!(
            "A{:02}X{:+07}Y{:+07}",
            self.access,
            dimension(self.at.x, 999999)?,
            dimension(self.at.y, 999999)?
        ))
    }
    fn size(&self) -> Result<String, String> {
        Ok(format!(
            "X{:04}Y{:04}R{:03}",
            dimension(self.width, 9999)?,
            dimension(self.height, 9999)?,
            self.rotation.round().rem_euclid(360.0) as u16
        ))
    }
    fn write(&self, out: &mut String, net: &str, layers: u8) -> Result<(), String> {
        let mut record = self.component(self.code, net, self.pin.is_empty())?;
        record.push_str(&match self.drill {
            Some((size, plated)) => format!(
                "D{:04}{}",
                dimension(size, 9999)?,
                if plated { 'P' } else { 'U' }
            ),
            None => "      ".into(),
        });
        record.push_str(&self.location()?);
        record.push_str(&if self.span.is_some() {
            "              ".into()
        } else {
            self.size()?
        });
        record.push_str(&format!(" S{}", self.mask));
        if let Some((start, end)) = self.span {
            record.push_str(&format!("L{start:02}L{end:02}"));
        }
        line(out, &record)?;
        if self.span.is_some_and(|(a, b)| a == 1 || b == layers) {
            line(
                out,
                &format!(
                    "{}      {}{} S{}",
                    self.component(27, net, false)?,
                    self.location()?,
                    self.size()?,
                    self.mask
                ),
            )?;
        }
        Ok(())
    }
}

pub fn write(d: &ProjectData, name: &str) -> Result<Output, String> {
    let layers = d.board.stackup.copper_layers;
    let pad_nets = d.nets.pad_map();
    let mut records = Vec::new();
    for (reference, pl) in &d.placement.placements {
        let Some(fp) = d
            .parts
            .parts
            .get(reference)
            .and_then(|p| d.footprints.get(&p.footprint))
        else {
            continue;
        };
        for pad in &fp.pads {
            let (width, height) = pad.shape.size();
            let (code, drill, access) = match pad.kind {
                PadKind::Smd => (327, None, if pl.side == Side::Top { 1 } else { layers }),
                PadKind::Through { drill, plated } => (
                    if plated { 317 } else { 367 },
                    Some((drill.min_size(), plated)),
                    0,
                ),
            };
            let mask = if !pad.mask {
                3
            } else if access == 0 {
                0
            } else if pl.side == Side::Top {
                2
            } else {
                1
            };
            records.push(Record {
                net: pad_nets
                    .get(&PadRef {
                        part: reference.clone(),
                        pad: pad.number.clone(),
                    })
                    .cloned(),
                reference: reference.clone(),
                pin: pad.number.clone(),
                code,
                at: pl.transform(pad.at),
                width,
                height: if matches!(pad.shape, PadShape::Circle { .. }) {
                    0.0
                } else {
                    height
                },
                rotation: -(pl.transform_angle(pad.rotation)
                    + if pl.side == Side::Bottom { 180.0 } else { 0.0 }),
                drill,
                access,
                mask,
                span: None,
            });
        }
    }
    for via in &d.routing.vias {
        let a = via
            .from
            .unwrap_or(Layer::TOP)
            .copper()
            .ok_or("Via start must be copper")?
            .index(layers)
            + 1;
        let b = via
            .to
            .unwrap_or(Layer::BOTTOM)
            .copper()
            .ok_or("Via end must be copper")?
            .index(layers)
            + 1;
        let (start, end) = (a.min(b), a.max(b));
        let through = start == 1 && end == layers;
        records.push(Record {
            net: Some(via.net.clone()),
            reference: "VIA".into(),
            pin: String::new(),
            code: if through { 317 } else { 307 },
            at: via.at,
            width: via.diameter,
            height: 0.0,
            rotation: 0.0,
            drill: Some((via.drill, true)),
            access: if through {
                0
            } else if start == 1 {
                1
            } else if end == layers {
                layers
            } else {
                start
            },
            mask: 3,
            span: if through { None } else { Some((start, end)) },
        });
    }
    for hole in &d.board.holes {
        let (w, h) = match hole.drill {
            Drill::Round { diameter } => (diameter, 0.0),
            Drill::Slot { width, height } => (width, height),
        };
        records.push(Record {
            net: hole.net.clone(),
            reference: "HOLE".into(),
            pin: String::new(),
            code: if hole.plated { 317 } else { 367 },
            at: hole.at,
            width: hole.pad_diameter.unwrap_or(w),
            height: h,
            rotation: -hole.rotation,
            drill: Some((hole.drill.min_size(), hole.plated)),
            access: 0,
            mask: 3,
            span: None,
        });
    }
    let names: BTreeSet<_> = records.iter().filter_map(|r| r.net.clone()).collect();
    if names.len() > 9999 {
        return Err("IPC-D-356 export supports at most 9999 named nets.".into());
    }
    let aliases: BTreeMap<_, _> = names
        .into_iter()
        .enumerate()
        .map(|(i, n)| (n, format!("N{:04}", i + 1)))
        .collect();
    let mut out = String::new();
    for header in [
        format!(
            "P  JOB   {}",
            ascii(name).chars().take(63).collect::<String>()
        ),
        "P  CODE  00".into(),
        "P  UNITS CUST 0".into(),
        "P  VER   IPC-D-356A".into(),
        "P  IMAGE PRIMARY".into(),
    ] {
        line(&mut out, &header)?;
    }
    for (name, alias) in &aliases {
        let display: String = ascii(name).chars().take(58).collect();
        line(&mut out, &format!("P  NNAME{alias} {display}"))?;
    }
    records.sort_by(|a, b| {
        a.net
            .cmp(&b.net)
            .then(a.reference.cmp(&b.reference))
            .then(a.pin.cmp(&b.pin))
    });
    for record in &records {
        record.write(
            &mut out,
            record.net.as_ref().map_or("N/C", |n| aliases[n].as_str()),
            layers,
        )?;
    }
    line(&mut out, "999")?;
    Ok(Output {
        name: format!("{name}.d356"),
        contents: out.into_bytes(),
    })
}
