//! IPC-D-356A electrical test netlist, fixed-width ASCII and decimils.
//!
//! A test record is 80 columns. These are the 1-based columns:
//!
//! | Columns | Field                                          |
//! |---------|------------------------------------------------|
//! | 1-3     | Record code: `317`, `327`, `367`, `307`, `027` |
//! | 4-17    | Net, as an `N0001` alias, or `N/C`             |
//! | 21-26   | Reference designator                           |
//! | 27      | `-` when a pin follows                         |
//! | 28-31   | Pin, or a `P001` alias for a longer pad name   |
//! | 32      | `M` for a mid-point (via or hole)              |
//! | 33-38   | Drill `D####` and `P` or `U`                   |
//! | 39-41   | Access side `A##`                              |
//! | 42-57   | Location `X+######Y+######`                    |
//! | 58-71   | Size and rotation `X####Y####R###`             |
//! | 73-74   | Solder mask `S#`                               |
//! | 75-80   | Layer span `L##L##` of a blind or buried via   |
//!
//! `P  NNAME` parameters map net aliases to net names. `C  PIN` comments map
//! pin aliases to pad names.
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

/// The width of the pin field, columns 28 to 31.
pub const PIN_WIDTH: usize = 4;

/// The most aliases that one part can have. They are `P001` to `P999`.
const MAX_PIN_ALIASES: usize = 999;

/// Maps each pad name of one part to the text of its pin field.
///
/// A name that fits the pin field is kept. Longer names get `P001`, `P002` and
/// so on, in sorted order. An alias that a short name of the part already uses
/// is skipped, with any letter case. The same name always gets the same alias,
/// and the order of the input does not change the result. Each test record
/// also has the reference designator, so two parts can use the same aliases.
pub fn pin_aliases<'a>(
    names: impl IntoIterator<Item = &'a str>,
) -> Result<BTreeMap<String, String>, String> {
    let names: BTreeSet<&str> = names.into_iter().collect();
    let mut taken = BTreeSet::new();
    let mut aliases = BTreeMap::new();
    let mut long = Vec::new();
    for name in names {
        let text = ascii(name);
        if text.len() > PIN_WIDTH {
            long.push(name);
        } else {
            taken.insert(text.to_ascii_uppercase());
            aliases.insert(name.to_string(), text);
        }
    }
    let mut free = (1..=MAX_PIN_ALIASES)
        .map(|i| format!("P{i:03}"))
        .filter(|a| !taken.contains(a));
    for name in long {
        let alias = free.next().ok_or_else(|| {
            format!(
                "IPC-D-356 has room for at most {MAX_PIN_ALIASES} aliases of pad names \
                 longer than {PIN_WIDTH} characters on one part."
            )
        })?;
        aliases.insert(name.to_string(), alias);
    }
    Ok(aliases)
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
            field(&self.pin, PIN_WIDTH)?,
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
    let mut pin_comments = Vec::new();
    for (reference, pl) in &d.placement.placements {
        let Some(fp) = d
            .parts
            .parts
            .get(reference)
            .and_then(|p| d.footprints.get(&p.footprint))
        else {
            continue;
        };
        let pins = pin_aliases(fp.pads.iter().map(|p| p.number.as_str()))
            .map_err(|e| format!("{reference}: {e}"))?;
        for (name, alias) in &pins {
            if ascii(name) != *alias {
                pin_comments.push((reference.clone(), alias.clone(), name.clone()));
            }
        }
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
                pin: pins[&pad.number].clone(),
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
    for (reference, alias, name) in &pin_comments {
        let head = format!("C  PIN   {}-{alias} ", field(reference, 6)?.trim_end());
        let display: String = ascii(name).chars().take(80 - head.len()).collect();
        line(&mut out, &format!("{head}{display}"))?;
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

#[cfg(test)]
mod tests {
    use super::*;

    fn aliases(names: &[&str]) -> BTreeMap<String, String> {
        pin_aliases(names.iter().copied()).unwrap()
    }

    #[test]
    fn short_pin_names_are_kept() {
        let map = aliases(&["1", "A1", "A12", "GND1", "EP", ""]);
        for (name, alias) in &map {
            assert_eq!(name, alias);
        }
        assert_eq!(map.len(), 6);
    }

    #[test]
    fn pin_names_of_exactly_four_characters_fit() {
        let map = aliases(&["A12B", "B12A"]);
        assert_eq!(map["A12B"], "A12B");
        assert_eq!(map["B12A"], "B12A");
    }

    #[test]
    fn long_pin_names_get_aliases_in_sorted_order() {
        let map = aliases(&["B4A9", "A1B12", "B1A12", "A4B9", "A5"]);
        assert_eq!(map["A1B12"], "P001");
        assert_eq!(map["B1A12"], "P002");
        assert_eq!(map["A4B9"], "A4B9");
        assert_eq!(map["B4A9"], "B4A9");
        assert_eq!(map["A5"], "A5");
    }

    #[test]
    fn names_that_shorten_alike_get_distinct_aliases() {
        let map = aliases(&["A1B12", "A1B13", "A1B1", "A1B10"]);
        assert_eq!(map["A1B1"], "A1B1");
        assert_eq!(map["A1B10"], "P001");
        assert_eq!(map["A1B12"], "P002");
        assert_eq!(map["A1B13"], "P003");
    }

    #[test]
    fn dozens_of_long_names_on_one_part_are_unique_and_fit() {
        let names: Vec<String> = (0..80).map(|i| format!("PIN_{i:03}")).collect();
        let mut all: Vec<&str> = names.iter().map(String::as_str).collect();
        all.extend(["1", "2", "P003", "p010"]);
        let map = pin_aliases(all.iter().copied()).unwrap();
        assert_eq!(map.len(), all.len());
        let values: BTreeSet<_> = map.values().map(|a| a.to_ascii_uppercase()).collect();
        assert_eq!(values.len(), map.len(), "aliases collide: {map:?}");
        for alias in map.values() {
            assert!(alias.len() <= PIN_WIDTH && alias.is_ascii());
            assert!(field(alias, PIN_WIDTH).is_ok());
        }
    }

    #[test]
    fn aliases_skip_names_that_the_part_already_uses() {
        let map = aliases(&["P001", "p002", "LONGNAME", "LONGERNAME"]);
        assert_eq!(map["P001"], "P001");
        assert_eq!(map["p002"], "p002");
        assert_eq!(map["LONGERNAME"], "P003");
        assert_eq!(map["LONGNAME"], "P004");
    }

    #[test]
    fn aliases_are_deterministic_and_ignore_input_order() {
        let forward = ["SHIELD", "A1B12", "1", "B1A12", "VBUS1"];
        let mut reverse = forward;
        reverse.reverse();
        let first = aliases(&forward);
        for _ in 0..10 {
            assert_eq!(aliases(&forward), first);
        }
        assert_eq!(aliases(&reverse), first);
    }

    #[test]
    fn repeated_pad_names_share_one_alias() {
        let map = aliases(&["SHIELD", "SHIELD", "SHIELD", "SHIELD", "1"]);
        assert_eq!(map.len(), 2);
        assert_eq!(map["SHIELD"], "P001");
    }

    #[test]
    fn distinct_parts_reuse_the_same_aliases() {
        // Each test record has the reference designator in columns 21-26, so
        // the pin only has to be unique within its part.
        let j1 = aliases(&["A1B12", "B1A12"]);
        let j2 = aliases(&["A1B12", "B1A12"]);
        assert_eq!(j1, j2);
        assert_eq!(j1["A1B12"], "P001");
    }

    #[test]
    fn non_ascii_names_are_measured_after_replacement() {
        let map = aliases(&["\u{3b1}1", "\u{3b1}1234"]);
        assert_eq!(map["\u{3b1}1"], "_1");
        assert_eq!(map["\u{3b1}1234"], "P001");
    }

    #[test]
    fn too_many_long_names_on_one_part_are_refused() {
        let names: Vec<String> = (0..MAX_PIN_ALIASES)
            .map(|i| format!("LONG{i:04}"))
            .collect();
        assert!(pin_aliases(names.iter().map(String::as_str)).is_ok());
        let mut more = names.clone();
        more.push("LONG9999".into());
        let err = pin_aliases(more.iter().map(String::as_str)).unwrap_err();
        assert!(err.contains("at most 999"), "{err}");
        // A short name that takes an alias leaves one fewer.
        let mut taken: Vec<&str> = names.iter().map(String::as_str).collect();
        taken.push("P500");
        assert!(pin_aliases(taken).is_err());
    }
}
