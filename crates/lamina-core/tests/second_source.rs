//! `audit_footprints` with a second source: the footprint of the same part
//! from its provider.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use lamina_core::model::provider::{Bundle, Hit, PartInfo, Provider, ProviderInfo};
use lamina_core::model::*;
use lamina_core::*;

/// A provider with the id `lcsc`. It gives a SOT-23 footprint for each part.
struct Fake {
    fetches: AtomicUsize,
}

impl Provider for Fake {
    fn info(&self) -> ProviderInfo {
        ProviderInfo {
            id: "lcsc".into(),
            name: "Fake".into(),
            description: "test parts".into(),
            example: "lcsc:C1".into(),
            online: true,
        }
    }

    fn search(&self, _query: &str, _limit: usize) -> std::result::Result<Vec<Hit>, String> {
        Ok(Vec::new())
    }

    fn fetch(&self, id: &str, _model: bool) -> std::result::Result<Bundle, String> {
        self.fetches.fetch_add(1, Ordering::SeqCst);
        let opt = lamina_footprint::Options::default();
        let fp = lamina_footprint::from_package("SOT-23", &opt, Some(&format!("LCSC_{id}")))?;
        Ok(Bundle {
            footprint: fp,
            model: None,
            info: PartInfo::default(),
            notes: Vec::new(),
        })
    }
}

/// A board with one part. Its footprint has a name with no standard package,
/// and its pads are those of `package`.
fn board(t: &std::path::Path, fake: &Arc<Fake>, package: &str) -> Project {
    let ports = Ports {
        providers: vec![fake.clone()],
        ..Ports::default()
    };
    let mut p = Project::create_with(&t.join("b"), "demo", ports).unwrap();
    p.set_library(Library::at(&t.join("lib")));
    let opt = lamina_footprint::Options::default();
    let mut fp = lamina_footprint::from_package(package, &opt, Some("MYPART")).unwrap();
    fp.provenance = Provenance::Manual {
        note: "drawn by hand".into(),
    };
    let mut d = p.data.clone();
    d.footprints.insert("MYPART".into(), fp);
    let mut part = Part::new("MYPART");
    part.lcsc = Some("C1".into());
    d.parts.parts.insert("Q1".into(), part);
    p.commit(
        d,
        Touched::default().footprint("MYPART".to_string()),
        false,
        "setup",
    )
    .unwrap();
    p
}

#[test]
fn a_second_source_that_agrees_gives_evidence_only_with_a_source() {
    let t = tempfile::tempdir().unwrap();
    let fake = Arc::new(Fake {
        fetches: AtomicUsize::new(0),
    });
    let mut p = board(t.path(), &fake, "SOT-23");

    // With no network and nothing in the cache, there is no source.
    let a = p
        .audit_footprints(AuditFootprints { online: false })
        .unwrap();
    assert_eq!(a.rows[0].result, "no_source");
    assert_eq!(fake.fetches.load(Ordering::SeqCst), 0, "no network use");

    let a = p
        .audit_footprints(AuditFootprints { online: true })
        .unwrap();
    assert_eq!(a.rows[0].result, "verified", "{:?}", a.rows[0].findings);
    assert_eq!(a.rows[0].source.as_deref(), Some("lcsc:C1"));
    let e = p.data.footprints["MYPART"].current_evidence().unwrap();
    assert_eq!(e.method, EvidenceMethod::SecondSource);
    assert!(e.pass);
}

#[test]
fn a_second_source_that_differs_stores_nothing() {
    let t = tempfile::tempdir().unwrap();
    let fake = Arc::new(Fake {
        fetches: AtomicUsize::new(0),
    });
    // The footprint has six pads. The part of the provider has three.
    let mut p = board(t.path(), &fake, "SOT-23-6");
    let a = p
        .audit_footprints(AuditFootprints { online: true })
        .unwrap();
    assert_eq!(a.rows[0].result, "differs");
    assert!(!a.rows[0].findings.is_empty());
    assert!(p.data.footprints["MYPART"].current_evidence().is_none());
}

/// A project with the KiCad provider on the fixture libraries. Its one part
/// has the SOT-23 footprint as a KiCad board gives it: no library name.
fn kicad_board(t: &std::path::Path) -> Project {
    let libs = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../lamina-kicad/tests/fixtures/footprints");
    let kicad = lamina_kicad::provider::KicadProvider::new(
        Arc::new(lamina_kicad::DirSource {
            footprints: libs.clone(),
            models: t.join("models"),
        }),
        vec![libs.clone()],
        vec![],
    );
    let ports = Ports {
        providers: vec![Arc::new(kicad)],
        ..Ports::default()
    };
    let mut p = Project::create_with(&t.join("b"), "demo", ports).unwrap();
    p.set_library(Library::at(&t.join("lib")));
    let text =
        std::fs::read_to_string(libs.join("Package_TO_SOT_SMD.pretty/SOT-23.kicad_mod")).unwrap();
    let mut fp = lamina_kicad::parse_footprint(&text, "", "kicad_pcb")
        .unwrap()
        .footprint;
    fp.id = "BOARD_SOT".into();
    fp.provenance = Provenance::Kicad {
        library: String::new(),
        footprint: "SOT-23".into(),
        path: "kicad_pcb".into(),
    };
    let mut d = p.data.clone();
    d.footprints.insert("BOARD_SOT".into(), fp);
    d.parts.parts.insert("Q1".into(), Part::new("BOARD_SOT"));
    p.commit(
        d,
        Touched::default().footprint("BOARD_SOT".to_string()),
        false,
        "setup",
    )
    .unwrap();
    p
}

#[test]
fn a_board_footprint_is_compared_with_the_kicad_library() {
    let t = tempfile::tempdir().unwrap();
    let mut p = kicad_board(t.path());
    let a = p
        .audit_footprints(AuditFootprints { online: false })
        .unwrap();
    assert_eq!(a.rows[0].result, "verified", "{:?}", a.rows[0]);
    assert_eq!(
        a.rows[0].source.as_deref(),
        Some("kicad:Package_TO_SOT_SMD:SOT-23")
    );
    let e = p.data.footprints["BOARD_SOT"].current_evidence().unwrap();
    assert_eq!(e.method, EvidenceMethod::SecondSource);
    assert!(e.document.contains("was not compared with a datasheet"));
}

#[test]
fn a_pad_that_was_moved_on_the_board_is_found() {
    let t = tempfile::tempdir().unwrap();
    let mut p = kicad_board(t.path());
    let mut d = p.data.clone();
    d.footprints.get_mut("BOARD_SOT").unwrap().pads[0].at.x += 0.08;
    p.commit(
        d,
        Touched::default().footprint("BOARD_SOT".to_string()),
        false,
        "move a pad",
    )
    .unwrap();
    let a = p
        .audit_footprints(AuditFootprints { online: false })
        .unwrap();
    assert_eq!(a.rows[0].result, "differs", "{:?}", a.rows[0]);
    assert!(p.data.footprints["BOARD_SOT"].current_evidence().is_none());
}
