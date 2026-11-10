//! The distance that the voltage between two conductors needs.

use serde::Deserialize;

use crate::{NetsFile, PinRole, pin_role};

#[derive(Debug, Clone, Copy, Deserialize)]
struct Spacing {
    internal: f64,
    external_bare: f64,
    external_coated: f64,
}

#[derive(Debug, Clone, Deserialize)]
struct SpacingRow {
    up_to: f64,
    #[serde(flatten)]
    spacing: Spacing,
}

#[derive(Debug, Clone, Deserialize)]
struct SpacingTable {
    rows: Vec<SpacingRow>,
    above_500_per_volt: Spacing,
}

/// Where two conductors are, for the spacing table.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Surface {
    /// An inner layer.
    Internal,
    /// An outer layer, and one of the two items has no solder mask (a pad).
    ExternalBare,
    /// An outer layer below solder mask.
    ExternalCoated,
}

impl Surface {
    /// The surface for a copper layer index of a board with `layers` layers.
    /// `bare` is true if one of the two items is a pad.
    pub fn of(layer: u8, layers: u8, bare: bool) -> Surface {
        if layer != 0 && layer + 1 != layers {
            Surface::Internal
        } else if bare {
            Surface::ExternalBare
        } else {
            Surface::ExternalCoated
        }
    }
}

fn table() -> &'static SpacingTable {
    static TABLE: std::sync::OnceLock<SpacingTable> = std::sync::OnceLock::new();
    TABLE.get_or_init(|| {
        serde_json::from_str(include_str!("ipc2221_spacing.json"))
            .expect("the spacing table is valid JSON")
    })
}

/// The minimum distance in mm between two conductors with `volts` between
/// them (IPC-2221C, Table 6-1).
pub fn voltage_spacing(volts: f64, surface: Surface) -> f64 {
    let t = table();
    let pick = |s: &Spacing| match surface {
        Surface::Internal => s.internal,
        Surface::ExternalBare => s.external_bare,
        Surface::ExternalCoated => s.external_coated,
    };
    let v = volts.abs();
    for row in &t.rows {
        if v <= row.up_to {
            return pick(&row.spacing);
        }
    }
    let last = t.rows.last().expect("the table has rows");
    pick(&last.spacing) + (v - last.up_to) * pick(&t.above_500_per_volt)
}

impl NetsFile {
    /// True if a net of the board has a voltage.
    pub fn has_voltages(&self) -> bool {
        self.nets.values().any(|n| n.voltage.is_some())
    }

    /// The voltage of a net: its `voltage`, or 0 for a ground net.
    pub fn volts(&self, net: &str) -> Option<f64> {
        let n = self.nets.get(net)?;
        n.voltage
            .or_else(|| (pin_role(net) == PinRole::Ground).then_some(0.0))
    }

    /// The distance that the voltage between two nets needs. It is 0 if the
    /// voltage of one of them is not known, or if no net has a voltage.
    pub fn voltage_distance(&self, a: Option<&str>, b: Option<&str>, surface: Surface) -> f64 {
        if !self.has_voltages() {
            return 0.0;
        }
        match (a.and_then(|n| self.volts(n)), b.and_then(|n| self.volts(n))) {
            (Some(x), Some(y)) if a != b => voltage_spacing(x - y, surface),
            _ => 0.0,
        }
    }

    /// The largest distance that the voltage of `net` needs to another net.
    pub fn voltage_floor(&self, net: &str, surface: Surface) -> f64 {
        if !self.has_voltages() {
            return 0.0;
        }
        self.nets
            .keys()
            .map(|other| self.voltage_distance(Some(net), Some(other), surface))
            .fold(0.0, f64::max)
    }
}
