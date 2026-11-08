use lamina_model::*;
use lamina_ports::format::BoardFormat;
use std::path::Path;

#[test]
fn escaped_library_names_do_not_collide_or_exceed_file_limits() {
    let mut d = ProjectData::new("names");
    let long = "a".repeat(300);
    for id in ["x/y", "x_2fy", "CON", long.as_str()] {
        d.footprints.insert(
            id.into(),
            Footprint::new(
                id,
                Provenance::Manual {
                    note: "test".into(),
                },
            ),
        );
    }
    let files = lamina_kicad::KicadFormat.companions(&d, Path::new("out/board.kicad_pcb"));
    let names: std::collections::BTreeSet<_> = files
        .iter()
        .filter(|(p, _)| p.extension().is_some_and(|e| e == "kicad_mod"))
        .map(|(p, _)| p.file_name().unwrap().to_string_lossy().into_owned())
        .collect();
    assert_eq!(names.len(), 4);
    assert!(
        names
            .iter()
            .all(|n| n.len() < 200 && !n.starts_with("CON."))
    );
}

#[test]
fn companions_supply_every_board_footprint_with_safe_names() {
    let mut d = ProjectData::new("library");
    let id = "provider:part/with\\bad?name";
    let mut fp = Footprint::new(
        id,
        Provenance::Manual {
            note: "test".into(),
        },
    );
    fp.pads.push(Pad::smd(
        "1",
        Point::new(1.0, 2.0),
        PadShape::Rect {
            width: 0.8,
            height: 1.2,
        },
    ));
    d.footprints.insert(id.into(), fp);
    d.parts.parts.insert("U1".into(), Part::new(id));
    let mut placement = Placement::at(10.0, 20.0);
    placement.side = Side::Bottom;
    placement.rotation = 90.0;
    d.placement.placements.insert("U1".into(), placement);
    d.board.holes.push(Hole {
        id: "mount".into(),
        at: Point::new(3.0, 4.0),
        drill: Drill::Round { diameter: 2.0 },
        plated: false,
        pad_diameter: None,
        net: None,
        rotation: 0.0,
    });
    let files = lamina_kicad::KicadFormat.companions(&d, Path::new("output/board.kicad_pcb"));
    let table = files
        .iter()
        .find(|(path, _)| path.ends_with("fp-lib-table"))
        .expect("library table");
    assert!(table.1.contains("${KIPRJMOD}/board.pretty"));
    let board = sexpr::parse(&lamina_kicad::export_board(&d)).unwrap();
    for fp in board.children("footprint") {
        let name = fp.arg(0).unwrap().strip_prefix("lamina:").unwrap();
        assert!(!name.chars().any(|c| ":/\\?*<>|\"".contains(c)));
        let path = Path::new("output/board.pretty").join(format!("{name}.kicad_mod"));
        let (_, text) = files
            .iter()
            .find(|(p, _)| p == &path)
            .expect("library footprint");
        let library = sexpr::parse(text).unwrap();
        assert_eq!(library.arg(0), Some(name));
        assert!(library.child("at").is_none());
        assert_eq!(library.children("pad").count(), 1);
    }
}
