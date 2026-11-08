use crate::project::{OpResult, Project, Touched};
use lamina_model::*;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct TuneLength {
    pub net: String,
    pub track: String,
    /// Zero-based segment index within the track's points.
    pub segment: usize,
    /// Target sum of all tracks on the net in mm, not pad-to-pad length.
    pub target: f64,
    /// Maximum meander height in mm, left of the segment direction.
    pub amplitude: f64,
    /// Centre spacing between successive meander legs in mm.
    pub spacing: f64,
}

impl Project {
    pub fn tune_length(&mut self, p: TuneLength) -> Result<OpResult, Error> {
        for (name, value) in [
            ("target", p.target),
            ("amplitude", p.amplitude),
            ("spacing", p.spacing),
        ] {
            if !value.is_finite() || value <= 0.0 {
                return Err(Error::invalid(format!(
                    "{name} must be finite and more than 0 mm."
                )));
            }
        }
        let track_index = self
            .data
            .routing
            .tracks
            .iter()
            .position(|t| t.id == p.track)
            .ok_or_else(|| Error::invalid(format!("Track `{}` does not exist.", p.track)))?;
        let track = &self.data.routing.tracks[track_index];
        if track.net != p.net || !self.data.nets.nets.contains_key(&p.net) {
            return Err(Error::invalid(
                "The selected track must belong to the specified existing net.",
            ));
        }
        if p.segment >= track.points.len().saturating_sub(1) {
            return Err(Error::invalid(
                "The segment index is outside the track. Use a zero-based index.",
            ));
        }
        let class = self.data.nets.nets[&p.net].class.as_deref();
        let clearance = self
            .data
            .board
            .rules
            .resolve(Some(&p.net), class, Some(track.layer))
            .clearance;
        if p.spacing + 1e-9 < track.width + clearance {
            return Err(Error::invalid(format!(
                "spacing must be at least track width plus clearance: {:.3} mm.",
                track.width + clearance
            )));
        }
        let current = self.data.net_lengths()[&p.net].length;
        let extra = p.target - current;
        if extra <= 1e-6 {
            return Err(Error::invalid(format!(
                "Net {} is already {current:.6} mm long. Give a longer target.",
                p.net
            )));
        }
        let (a, b) = (track.points[p.segment], track.points[p.segment + 1]);
        let length = a.dist(b);
        let bumps = (extra / (2.0 * p.amplitude)).ceil().max(1.0);
        let run = (2.0 * bumps - 1.0) * p.spacing;
        if !bumps.is_finite() || bumps > 10000.0 || run + 2.0 * p.spacing > length {
            return Err(Error::invalid(
                "The selected segment has no room for this target, amplitude and spacing.",
            ));
        }
        let height = extra / (2.0 * bumps);
        let u = (b - a) * (1.0 / length);
        let n = Point::new(-u.y, u.x);
        let at = |along, up| a + u * along + n * up;
        let start = (length - run) / 2.0;
        let mut points = vec![a];
        for i in 0..bumps as usize {
            let x = start + 2.0 * p.spacing * i as f64;
            points.extend([
                at(x, 0.0),
                at(x, height),
                at(x + p.spacing, height),
                at(x + p.spacing, 0.0),
            ]);
        }
        points.push(b);
        if points.iter().any(|p| !p.is_finite()) {
            return Err(Error::invalid(
                "The requested geometry exceeds the coordinate range.",
            ));
        }
        let mut d = self.data.clone();
        d.routing.tracks[track_index]
            .points
            .splice(p.segment..=p.segment + 1, points);
        self.commit(
            d,
            Touched::file(FileKind::Routing),
            true,
            format!(
                "Tuned net {} to {:.6} mm total track length",
                p.net, p.target
            ),
        )
    }
}
