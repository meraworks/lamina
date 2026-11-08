use lamina_model::*;
use std::collections::BTreeMap;

pub fn check(d: &ProjectData) -> Vec<Violation> {
    let lengths = d.net_lengths();
    let mut out = Vec::new();
    let mut groups: BTreeMap<&str, Vec<(&str, &Net, f64)>> = BTreeMap::new();
    for (name, net) in &d.nets.nets {
        let length = lengths[name].length;
        if net.length_min.is_some_and(|min| length + 1e-6 < min)
            || net.length_max.is_some_and(|max| length > max + 1e-6)
        {
            out.push(Violation::warning("length_out_of_range", format!(
                "Net {name} has {length:.3} mm of tracks; minimum {}, maximum {} mm (total tracks, not pad-to-pad path).",
                net.length_min.map_or("unset".into(), |v| format!("{v:.3}")),
                net.length_max.map_or("unset".into(), |v| format!("{v:.3}"))
            )).items([name.clone()]));
        }
        if let Some(group) = &net.skew_group {
            groups.entry(group).or_default().push((name, net, length));
        }
    }
    for (group, members) in groups {
        let Some(limit) = members
            .iter()
            .filter_map(|(_, n, _)| n.max_skew)
            .reduce(f64::min)
        else {
            continue;
        };
        let min = members
            .iter()
            .map(|(_, _, l)| *l)
            .fold(f64::INFINITY, f64::min);
        let max = members.iter().map(|(_, _, l)| *l).fold(0.0, f64::max);
        if max - min > limit + 1e-6 {
            out.push(Violation::warning("skew_out_of_range", format!(
                "Group {group} has {:.3} mm track-length spread, maximum {limit:.3} mm (total tracks, not pad-to-pad path).", max - min
            )).items(members.iter().map(|(name, _, _)| name.to_string())));
        }
    }
    out
}
