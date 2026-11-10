//! Drill files for each via span, the job file, paste for parts that are
//! not placed, and long references in the netlist.

use lamina_footprint::{Options, from_package};
use lamina_model::*;
use lamina_scene::{SceneLayer, build_2d_with};

fn board() -> ProjectData {
    let mut d = ProjectData::new("spans");
    d.board.stackup.copper_layers = 4;
    d.board.outline = Some(Contour::rect(0.0, 0.0, 30.0, 20.0));
    let fp = from_package("R_0603", &Options::default(), None).unwrap();
    d.footprints.insert(fp.id.clone(), fp);
    for (r, x) in [("R1", 10.0), ("LONGREF12", 20.0)] {
        d.parts.parts.insert(r.into(), Part::new("R_0603"));
        d.placement
            .placements
            .insert(r.into(), Placement::at(x, 10.0));
    }
    d.nets.nets.insert(
        "A".into(),
        Net {
            pads: vec!["R1.2".parse().unwrap(), "LONGREF12.1".parse().unwrap()],
            ..Default::default()
        },
    );
    let via = |id: &str, x: f64, from: Option<&str>, to: Option<&str>| Via {
        id: id.into(),
        net: "A".into(),
        at: Point::new(x, 15.0),
        diameter: 0.6,
        drill: 0.3,
        from: from.map(|l| l.parse().unwrap()),
        to: to.map(|l| l.parse().unwrap()),
    };
    d.routing.vias.push(via("v1", 5.0, None, None));
    d.routing
        .vias
        .push(via("v2", 10.0, Some("top"), Some("in1")));
    d.routing
        .vias
        .push(via("v3", 15.0, Some("in1"), Some("in2")));
    d
}

fn text(o: &lamina_ports::fab::Output) -> String {
    String::from_utf8(o.contents.clone()).unwrap()
}

#[test]
fn each_via_span_has_its_drill_file() {
    let d = board();
    let g = lamina_geom::build(&d);
    let files = lamina_export::drills(&g, "b");
    let names: Vec<&str> = files.iter().map(|f| f.name.as_str()).collect();
    assert_eq!(
        names,
        vec!["b-PTH.DRL", "b-L1-L2.DRL", "b-L2-L3.DRL", "b-NPTH.DRL"]
    );
    let by = |n: &str| text(files.iter().find(|f| f.name == n).unwrap());
    assert!(by("b-PTH.DRL").contains("TF.FileFunction,Plated,1,4,PTH"));
    assert!(by("b-PTH.DRL").contains("X5.0000Y15.0000"));
    assert!(!by("b-PTH.DRL").contains("X10.0000Y15.0000"));
    assert!(by("b-L1-L2.DRL").contains("TF.FileFunction,Plated,1,2,Blind"));
    assert!(by("b-L1-L2.DRL").contains("X10.0000Y15.0000"));
    assert!(by("b-L2-L3.DRL").contains("TF.FileFunction,Plated,2,3,Buried"));
    assert!(by("b-L2-L3.DRL").contains("X15.0000Y15.0000"));
}

#[test]
fn the_job_file_lists_the_gerber_files_and_the_board_data() {
    let d = board();
    let a = lamina_drc::analyze(&d);
    let s = lamina_scene::build_2d(&d, &a);
    let art = lamina_export::gerbers(&s, "b");
    let job = lamina_export::job(&d, &art, "b");
    assert_eq!(job.name, "b.gbrjob");
    let j: serde_json::Value = serde_json::from_slice(&job.contents).unwrap();
    assert_eq!(j["GeneralSpecs"]["LayerNumber"], 4);
    assert_eq!(j["GeneralSpecs"]["Size"]["X"], 30.0);
    assert_eq!(j["GeneralSpecs"]["Size"]["Y"], 20.0);
    let files = j["FilesAttributes"].as_array().unwrap();
    assert_eq!(files.len(), art.len());
    assert!(
        files
            .iter()
            .any(|f| f["Path"] == "b.GTL" && f["FileFunction"] == "Copper,L1,Top")
    );
    assert!(
        files
            .iter()
            .any(|f| f["Path"] == "b.GTS" && f["FilePolarity"] == "Negative")
    );
}

#[test]
fn a_part_that_is_not_placed_gets_paste_only_on_request() {
    let mut d = board();
    let a = lamina_drc::analyze(&d);
    let count = |d: &ProjectData, paste_for_dnp: bool| {
        build_2d_with(d, &a, &lamina_scene::Options { paste_for_dnp })
            .layer(SceneLayer::Paste(Side::Top))
            .map(|l| l.items.len())
            .unwrap_or(0)
    };
    assert_eq!(count(&d, false), 4);
    d.parts.parts.get_mut("R1").unwrap().dnp = true;
    assert_eq!(count(&d, false), 2);
    assert_eq!(count(&d, true), 4);
}

#[test]
fn a_long_reference_gets_an_alias_in_the_netlist() {
    let d = board();
    let out = text(&lamina_export::ipc356::write(&d, "b").unwrap());
    assert!(out.contains("C  REF   Z00001 LONGREF12"), "{out}");
    assert!(
        out.lines()
            .any(|l| l.starts_with("327") && l.contains("Z00001-1"))
    );
    assert!(
        !out.lines()
            .any(|l| l.starts_with("327") && l.contains("LONGRE"))
    );
    assert!(out.lines().all(|l| l.len() == 80));
}

#[test]
fn a_rule_area_is_a_line_on_the_keepout_layer_and_not_in_the_gerbers() {
    let mut d = board();
    d.routing.areas.push(RuleArea {
        id: "a1".into(),
        layers: vec![],
        outline: Contour::rect(2.0, 2.0, 5.0, 5.0),
    });
    let a = lamina_drc::analyze(&d);
    let s = lamina_scene::build_2d(&d, &a);
    let layer = s.layer(SceneLayer::Keepout).expect("the layer exists");
    assert_eq!(layer.items.len(), 1);
    assert!(matches!(
        layer.items[0],
        lamina_scene::Geom::Stroke { closed: true, .. }
    ));
    let before = lamina_export::gerbers(&lamina_scene::build_2d(&board(), &a), "b");
    let after = lamina_export::gerbers(&s, "b");
    for (x, y) in before.iter().zip(&after) {
        assert_eq!(x.contents, y.contents, "{}", x.name);
    }
}

#[test]
fn copper_gerbers_have_net_part_and_function_attributes() {
    let d = board();
    let a = lamina_drc::analyze(&d);
    let s = lamina_scene::build_2d(&d, &a);
    let files = lamina_export::gerbers(&s, "b");
    let top = text(files.iter().find(|f| f.name == "b.GTL").unwrap());
    assert!(top.contains("%TO.P,R1,2*%"), "{top}");
    assert!(top.contains("%TO.C,R1*%"));
    assert!(top.contains("%TO.N,A*%"));
    assert!(top.contains("%TO.N,N/C*%"), "a pad with no net");
    assert!(top.contains("%TA.AperFunction,ViaPad*%"));
    // An attribute is before the aperture that it is for, and ends after it.
    let via = top.find("%TA.AperFunction,ViaPad*%").unwrap();
    let rest = &top[via..];
    assert!(rest.find("%ADD").unwrap() < rest.find("%TD*%").unwrap());
    // The silkscreen has no net attributes.
    let silk = text(files.iter().find(|f| f.name == "b.GTO").unwrap());
    assert!(!silk.contains("%TO."));
}
