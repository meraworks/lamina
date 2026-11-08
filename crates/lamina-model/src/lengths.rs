use std::collections::BTreeMap;

use crate::ProjectData;
use schemars::JsonSchema;
use serde::Serialize;

#[derive(Debug, Clone, Default, PartialEq, Serialize, JsonSchema)]
pub struct NetLength {
    /// Sum of all track segments in mm, including branches. Excludes via depth.
    pub length: f64,
    pub via_count: usize,
}

impl ProjectData {
    pub fn net_lengths(&self) -> BTreeMap<String, NetLength> {
        let mut lengths: BTreeMap<String, NetLength> = self
            .nets
            .nets
            .keys()
            .map(|name| (name.clone(), NetLength::default()))
            .collect();
        for track in &self.routing.tracks {
            lengths.entry(track.net.clone()).or_default().length += track
                .points
                .windows(2)
                .map(|p| p[0].dist(p[1]))
                .sum::<f64>();
        }
        for via in &self.routing.vias {
            lengths.entry(via.net.clone()).or_default().via_count += 1;
        }
        lengths
    }
}
