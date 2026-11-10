//! Missing 3D models in `cache/models`: the status report, the copy from the
//! part library, the new download from a provider, and `restore_models`.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use lamina_core::model::provider::{Bundle, Hit, ModelFile, PartInfo, Provider, ProviderInfo};
use lamina_core::model::*;
use lamina_core::*;

/// A small OBJ model: one triangle, 2 mm high.
const OBJ: &[u8] = b"v -0.5 -0.5 0\nv 0.5 -0.5 0\nv 0 0.5 2\nf 1 2 3\n";

/// A provider in memory. Each part is a resistor footprint with one model.
/// It counts the fetches and can be switched off like a computer without a network.
struct MemProvider {
    fetches: AtomicUsize,
    offline: AtomicBool,
    /// The model name that the provider gives now.
    model_name: std::sync::Mutex<String>,
}

impl MemProvider {
    fn new() -> Arc<MemProvider> {
        Arc::new(MemProvider {
            fetches: AtomicUsize::new(0),
            offline: AtomicBool::new(false),
            model_name: std::sync::Mutex::new("body.obj".into()),
        })
    }

    fn fetches(&self) -> usize {
        self.fetches.load(Ordering::SeqCst)
    }

    fn set_offline(&self, v: bool) {
        self.offline.store(v, Ordering::SeqCst);
    }
}

impl Provider for MemProvider {
    fn info(&self) -> ProviderInfo {
        ProviderInfo {
            id: "mem".into(),
            name: "Memory".into(),
            description: "test parts".into(),
            example: "mem:R1K".into(),
            online: true,
        }
    }

    fn search(&self, _query: &str, _limit: usize) -> std::result::Result<Vec<Hit>, String> {
        Ok(Vec::new())
    }

    fn fetch(&self, id: &str, model: bool) -> std::result::Result<Bundle, String> {
        self.fetches.fetch_add(1, Ordering::SeqCst);
        if self.offline.load(Ordering::SeqCst) {
            return Err("no network".into());
        }
        let opt = lamina_footprint::Options::default();
        let mut fp = lamina_footprint::from_package("R_0603", &opt, Some(&format!("MEM_{id}")))?;
        fp.provenance = Provenance::Manual {
            note: format!("mem {id}"),
        };
        let model = model.then(|| ModelFile {
            name: self.model_name.lock().unwrap().clone(),
            bytes: OBJ.to_vec(),
            offset: [0.0; 3],
            rotation: [0.0; 3],
            scale: [1.0; 3],
            height: Some(2.0),
        });
        Ok(Bundle {
            footprint: fp,
            model,
            info: PartInfo {
                mpn: Some(format!("MPN-{id}")),
                ..Default::default()
            },
            notes: Vec::new(),
        })
    }
}

/// A renderer that reports the 3D scene instead of drawing it: the number of
/// parts with a model and the number of parts.
struct SceneRenderer;

impl lamina_core::ports::render::Renderer for SceneRenderer {
    fn snapshot(
        &self,
        _s2: &lamina_scene::Scene2D,
        s3: &lamina_scene::Scene3D,
        _kind: lamina_core::ports::render::Snapshot,
        _width: u32,
        _height: u32,
        _opts2d: Option<&lamina_core::ports::render::Options2D>,
    ) -> std::result::Result<(Vec<u8>, Vec<String>), String> {
        let with = s3.instances.iter().filter(|i| i.model.is_some()).count();
        let text = format!("{with}/{}", s3.instances.len());
        Ok((text.into_bytes(), Vec::new()))
    }
}

fn ports(providers: Vec<Arc<dyn Provider>>) -> Ports {
    Ports {
        providers,
        meshes: Some(Arc::new(lamina_step::FileMeshLoader)),
        renderer: Some(Arc::new(SceneRenderer)),
        ..Ports::default()
    }
}

/// A board with the mem provider and its own library. R1 and R2 use
/// `mem:R1K` and are placed. R3 uses `mem:R10K` and is not placed.
fn board(t: &Path, mem: &Arc<MemProvider>) -> Project {
    let mut p = Project::create_with(&t.join("b"), "demo", ports(vec![mem.clone()])).unwrap();
    p.set_library(Library::at(&t.join("lib")));
    p.set_outline(SetOutline {
        rect: Some(RectSpec {
            x: 0.0,
            y: 0.0,
            width: 20.0,
            height: 10.0,
            corner_radius: 0.0,
        }),
        contour: None,
        cutouts: None,
        strict: false,
    })
    .unwrap();
    for (part, r) in [("mem:R1K", "R1"), ("mem:R1K", "R2"), ("mem:R10K", "R3")] {
        import(&mut p, part, Some(r)).unwrap();
    }
    for (r, x) in [("R1", 5.0), ("R2", 10.0)] {
        p.place(Place {
            refdes: r.into(),
            x,
            y: 5.0,
            rotation: Some(90.0),
            side: None,
            locked: None,
            strict: false,
        })
        .unwrap();
    }
    p
}

fn import(p: &mut Project, part: &str, refdes: Option<&str>) -> Result<OpResult> {
    p.import_part(ImportPart {
        part: part.into(),
        refdes: refdes.map(String::from),
        value: None,
        footprint_id: None,
        refresh: false,
        model: true,
        replace: false,
        strict: false,
    })
}

fn models(p: &Project) -> PathBuf {
    p.root().join(MODEL_DIR)
}

/// The board files that a restore must not change, with their bytes.
fn snapshot(p: &Project) -> Vec<(PathBuf, Vec<u8>)> {
    let mut out = Vec::new();
    for f in ["board.json", "parts.json", "placement.json", "nets.json"] {
        let path = p.root().join(f);
        out.push((path.clone(), std::fs::read(&path).unwrap()));
    }
    let mut fps: Vec<PathBuf> = std::fs::read_dir(p.root().join(FOOTPRINT_DIR))
        .unwrap()
        .flatten()
        .map(|e| e.path())
        .collect();
    fps.sort();
    for path in fps {
        out.push((path.clone(), std::fs::read(&path).unwrap()));
    }
    out
}

fn assert_same(before: &[(PathBuf, Vec<u8>)], p: &Project) {
    let after = snapshot(p);
    assert_eq!(before.len(), after.len());
    for ((a, x), (_, y)) in before.iter().zip(&after) {
        assert!(x == y, "{} changed", a.display());
    }
}

fn render_3d(p: &Project) -> String {
    let r = p
        .render(Render {
            view: RenderView::View3D,
            name: Some("t".into()),
            ..Default::default()
        })
        .unwrap();
    String::from_utf8(r.png).unwrap()
}

fn missing_files(p: &Project) -> Vec<String> {
    p.status()
        .missing_models
        .into_iter()
        .map(|m| format!("{}={}", m.footprint, m.file))
        .collect()
}

#[test]
fn import_stores_the_model_in_the_cache_and_the_library() {
    let t = tempfile::tempdir().unwrap();
    let mem = MemProvider::new();
    let p = board(t.path(), &mem);
    assert_eq!(mem.fetches(), 2);
    assert_eq!(
        p.data.footprints["MEM_R1K"].model.as_ref().unwrap().file,
        "mem/body.obj"
    );
    assert!(models(&p).join("mem/body.obj").is_file());
    assert!(t.path().join("lib/models/mem/body.obj").is_file());
    assert!(p.status().missing_models.is_empty());
    assert_eq!(render_3d(&p), "2/2");
}

#[test]
fn status_lists_missing_models_and_the_library_restores_them() {
    let t = tempfile::tempdir().unwrap();
    let mem = MemProvider::new();
    let p = board(t.path(), &mem);
    std::fs::remove_dir_all(models(&p)).unwrap();
    mem.set_offline(true);
    let before = snapshot(&p);

    assert_eq!(
        missing_files(&p),
        vec!["MEM_R10K=mem/body.obj", "MEM_R1K=mem/body.obj"]
    );
    let json = serde_json::to_value(p.status()).unwrap();
    assert_eq!(json["missing_models"][0]["file"], "mem/body.obj");

    let fetches = mem.fetches();
    let r = p.restore_models(RestoreModels::default()).unwrap();
    assert_eq!(r.from_library, vec!["mem/body.obj (MEM_R10K, MEM_R1K)"]);
    assert!(r.downloaded.is_empty() && r.missing.is_empty(), "{r:?}");
    assert_eq!(r.present, 0);
    // The library had the file, so the provider was not asked.
    assert_eq!(mem.fetches(), fetches);
    assert_eq!(std::fs::read(models(&p).join("mem/body.obj")).unwrap(), OBJ);
    assert!(p.status().missing_models.is_empty());
    assert!(
        !serde_json::to_string(&p.status())
            .unwrap()
            .contains("missing_models")
    );
    assert_same(&before, &p);

    // The 3D scene has the mesh again.
    let s3 = lamina_scene::build_3d(&p.data, p.root());
    let r1 = s3.instances.iter().find(|i| i.refdes == "R1").unwrap();
    assert_eq!(
        r1.model.as_deref(),
        Some(models(&p).join("mem/body.obj").as_path())
    );
    assert_eq!(render_3d(&p), "2/2");
}

#[test]
fn restore_downloads_from_the_provider_when_the_library_has_no_copy() {
    let t = tempfile::tempdir().unwrap();
    let mem = MemProvider::new();
    let p = board(t.path(), &mem);
    std::fs::remove_dir_all(models(&p)).unwrap();
    std::fs::remove_dir_all(t.path().join("lib/models")).unwrap();
    let before = snapshot(&p);
    let fetches = mem.fetches();

    let r = p.restore_models(RestoreModels::default()).unwrap();
    assert_eq!(r.downloaded.len(), 1, "{r:?}");
    assert!(
        r.downloaded[0].starts_with("mem/body.obj (MEM_R10K, MEM_R1K) from mem:"),
        "{r:?}"
    );
    assert!(r.from_library.is_empty() && r.missing.is_empty(), "{r:?}");
    // One download for the one file that two footprints use.
    assert_eq!(mem.fetches(), fetches + 1);
    assert_eq!(std::fs::read(models(&p).join("mem/body.obj")).unwrap(), OBJ);
    // The library gets its copy back.
    assert!(t.path().join("lib/models/mem/body.obj").is_file());
    assert!(p.status().missing_models.is_empty());
    assert_same(&before, &p);
    assert_eq!(render_3d(&p), "2/2");

    // A second call changes nothing and downloads nothing.
    let r = p.restore_models(RestoreModels::default()).unwrap();
    assert_eq!(r.present, 1);
    assert!(r.from_library.is_empty() && r.downloaded.is_empty() && r.missing.is_empty());
    assert_eq!(mem.fetches(), fetches + 1);
    assert_same(&before, &p);
}

#[test]
fn restore_uses_the_library_records_when_the_project_cache_is_gone() {
    let t = tempfile::tempdir().unwrap();
    let mem = MemProvider::new();
    let p = board(t.path(), &mem);
    // No part records in the project and no models anywhere: the record in
    // the library still names the part that the model came from.
    std::fs::remove_dir_all(models(&p)).unwrap();
    std::fs::remove_dir_all(p.root().join(PARTS_DIR)).unwrap();
    std::fs::remove_dir_all(t.path().join("lib/models")).unwrap();
    let r = p.restore_models(RestoreModels::default()).unwrap();
    assert_eq!(r.downloaded.len(), 1, "{r:?}");
    assert!(models(&p).join("mem/body.obj").is_file());
}

#[test]
fn import_part_downloads_a_missing_model_and_keeps_the_footprint() {
    let t = tempfile::tempdir().unwrap();
    let mem = MemProvider::new();
    let mut p = board(t.path(), &mem);
    std::fs::remove_dir_all(models(&p)).unwrap();
    std::fs::remove_dir_all(t.path().join("lib/models")).unwrap();
    let before = snapshot(&p);

    let r = import(&mut p, "mem:R1K", None).unwrap();
    assert!(
        r.notes
            .iter()
            .any(|n| n.contains("mem/body.obj was missing") && n.contains("mem:R1K")),
        "{:?}",
        r.notes
    );
    assert!(r.notes.iter().any(|n| n.contains("kept as is")));
    assert!(r.changed.is_empty(), "{:?}", r.changed);
    assert!(models(&p).join("mem/body.obj").is_file());
    assert_eq!(
        p.data.footprints["MEM_R1K"].model.as_ref().unwrap().file,
        "mem/body.obj"
    );
    assert_same(&before, &p);
    assert!(p.status().missing_models.is_empty());

    // import_parts does the same for a part whose model is missing.
    std::fs::remove_dir_all(models(&p)).unwrap();
    std::fs::remove_dir_all(t.path().join("lib/models")).unwrap();
    let r = p
        .import_parts(ImportParts {
            parts: vec![ImportPart {
                part: "mem:R10K".into(),
                refdes: None,
                value: None,
                footprint_id: None,
                refresh: false,
                model: true,
                replace: false,
                strict: false,
            }],
        })
        .unwrap();
    assert!(
        r[0].notes.iter().any(|n| n.contains("was missing")),
        "{:?}",
        r[0].notes
    );
    assert!(models(&p).join("mem/body.obj").is_file());
    assert_same(&before, &p);
}

#[test]
fn import_part_without_a_model_does_not_download_it() {
    let t = tempfile::tempdir().unwrap();
    let mem = MemProvider::new();
    let mut p = board(t.path(), &mem);
    std::fs::remove_dir_all(models(&p)).unwrap();
    std::fs::remove_dir_all(t.path().join("lib/models")).unwrap();
    let fetches = mem.fetches();
    let r = p
        .import_part(ImportPart {
            part: "mem:R1K".into(),
            refdes: None,
            value: None,
            footprint_id: None,
            refresh: false,
            model: false,
            replace: false,
            strict: false,
        })
        .unwrap();
    assert_eq!(mem.fetches(), fetches);
    assert!(!models(&p).join("mem/body.obj").exists());
    assert!(r.notes.iter().any(|n| n.contains("not in the cache")));
}

#[test]
fn a_model_that_is_nowhere_is_reported_and_the_board_still_works() {
    let t = tempfile::tempdir().unwrap();
    let mem = MemProvider::new();
    let mut p = board(t.path(), &mem);
    std::fs::remove_dir_all(models(&p)).unwrap();
    std::fs::remove_dir_all(t.path().join("lib/models")).unwrap();
    mem.set_offline(true);
    let before = snapshot(&p);

    let r = p.restore_models(RestoreModels::default()).unwrap();
    assert!(r.from_library.is_empty() && r.downloaded.is_empty());
    assert_eq!(r.missing.len(), 1, "{r:?}");
    let m = &r.missing[0];
    assert!(m.contains("mem/body.obj (MEM_R10K, MEM_R1K)"), "{m}");
    assert!(m.contains("not in the part library"), "{m}");
    assert!(m.contains("no network"), "{m}");
    assert_eq!(missing_files(&p).len(), 2);
    assert_same(&before, &p);

    // import_part keeps the footprint and says why there is no model.
    let r = import(&mut p, "mem:R1K", None).unwrap();
    assert!(
        r.notes
            .iter()
            .any(|n| n.contains("not in the cache") && n.contains("no network")),
        "{:?}",
        r.notes
    );
    assert_same(&before, &p);

    // The board opens again, and the 3D scene has placeholders.
    drop(p);
    let mut p = Project::open_with(&t.path().join("b"), ports(vec![mem.clone()])).unwrap();
    p.set_library(Library::at(&t.path().join("lib")));
    assert_eq!(p.data.parts.parts.len(), 3);
    assert_eq!(p.status().placed, 2);
    let s3 = lamina_scene::build_3d(&p.data, p.root());
    assert_eq!(s3.instances.len(), 2);
    assert!(
        s3.instances
            .iter()
            .all(|i| i.model.is_none() && i.placeholder.is_some())
    );
    assert_eq!(render_3d(&p), "0/2");

    // When the network is back, the same call gets the model.
    mem.set_offline(false);
    let r = p.restore_models(RestoreModels::default()).unwrap();
    assert_eq!(r.downloaded.len(), 1, "{r:?}");
    assert!(p.status().missing_models.is_empty());
    assert_eq!(render_3d(&p), "2/2");
}

#[test]
fn without_download_only_the_library_is_used() {
    let t = tempfile::tempdir().unwrap();
    let mem = MemProvider::new();
    let p = board(t.path(), &mem);
    std::fs::remove_dir_all(models(&p)).unwrap();
    std::fs::remove_dir_all(t.path().join("lib/models")).unwrap();
    let fetches = mem.fetches();
    let r = p.restore_models(RestoreModels { download: false }).unwrap();
    assert_eq!(mem.fetches(), fetches);
    assert_eq!(r.missing.len(), 1);
    assert!(
        r.missing[0].ends_with("It is not in the part library."),
        "{r:?}"
    );
    assert!(!models(&p).join("mem/body.obj").exists());
}

#[test]
fn a_footprint_that_did_not_come_from_a_provider_says_so() {
    let t = tempfile::tempdir().unwrap();
    let mem = MemProvider::new();
    let mut p = board(t.path(), &mem);
    let user = t.path().join("my.obj");
    std::fs::write(&user, OBJ).unwrap();
    p.footprint_from_package(FootprintFromPackage {
        package: "R_0805".into(),
        density: None,
        id: None,
        overwrite: false,
    })
    .unwrap();
    p.set_model(SetModel {
        footprint: "R_0805".into(),
        file: user.display().to_string(),
        offset: [0.0; 3],
        rotation: [0.0; 3],
    })
    .unwrap();
    std::fs::remove_file(models(&p).join("user/my.obj")).unwrap();
    assert!(missing_files(&p).contains(&"R_0805=user/my.obj".to_string()));
    let r = p.restore_models(RestoreModels::default()).unwrap();
    assert_eq!(r.present, 1);
    assert_eq!(r.missing.len(), 1, "{r:?}");
    assert!(
        r.missing[0].contains("user/my.obj (R_0805)")
            && r.missing[0].contains("did not come from a provider"),
        "{r:?}"
    );
}

#[test]
fn models_that_are_there_are_never_replaced() {
    let t = tempfile::tempdir().unwrap();
    let mem = MemProvider::new();
    let mut p = board(t.path(), &mem);
    // The project has its own version of the model. The library and the
    // provider have a different one.
    let mine = b"v 0 0 0\nv 1 0 0\nv 0 1 5\nf 1 2 3\n";
    std::fs::write(models(&p).join("mem/body.obj"), mine).unwrap();
    let fetches = mem.fetches();
    for _ in 0..2 {
        let r = p.restore_models(RestoreModels::default()).unwrap();
        assert_eq!(r.present, 1);
        assert!(r.from_library.is_empty() && r.downloaded.is_empty() && r.missing.is_empty());
    }
    let r = import(&mut p, "mem:R1K", None).unwrap();
    assert!(!r.notes.iter().any(|n| n.contains("missing")), "{r:?}");
    assert_eq!(mem.fetches(), fetches);
    assert_eq!(
        std::fs::read(models(&p).join("mem/body.obj")).unwrap(),
        mine
    );
    assert_eq!(
        std::fs::read(t.path().join("lib/models/mem/body.obj")).unwrap(),
        OBJ
    );
}

#[test]
fn a_provider_with_a_new_model_name_does_not_change_the_footprint() {
    let t = tempfile::tempdir().unwrap();
    let mem = MemProvider::new();
    let p = board(t.path(), &mem);
    std::fs::remove_dir_all(models(&p)).unwrap();
    std::fs::remove_dir_all(t.path().join("lib/models")).unwrap();
    *mem.model_name.lock().unwrap() = "body-v2.obj".into();
    let before = snapshot(&p);
    let r = p.restore_models(RestoreModels::default()).unwrap();
    assert_eq!(r.missing.len(), 1, "{r:?}");
    assert!(
        r.missing[0].contains("now gives the model mem/body-v2.obj")
            && r.missing[0].contains("refresh: true"),
        "{r:?}"
    );
    assert!(!models(&p).join("mem/body-v2.obj").exists());
    assert!(!models(&p).join("mem/body.obj").exists());
    assert_same(&before, &p);
}

#[test]
fn model_paths_that_leave_the_cache_are_refused() {
    for ok in ["lcsc/abc.step", "kicad/LED_SMD/LED_0603.step", "user/a.obj"] {
        assert!(lamina_core::provider::model_path_ok(ok), "{ok}");
    }
    for bad in [
        "",
        "../evil.obj",
        "mem/../../evil.obj",
        "mem\\..\\..\\evil.obj",
        "/etc/evil.obj",
        "./mem/a.obj",
    ] {
        assert!(!lamina_core::provider::model_path_ok(bad), "{bad}");
    }

    let t = tempfile::tempdir().unwrap();
    let mem = MemProvider::new();
    let p = board(t.path(), &mem);
    // A library file that `models/../../evil.obj` would reach.
    std::fs::write(t.path().join("evil.obj"), OBJ).unwrap();
    std::fs::create_dir_all(t.path().join("lib/x")).unwrap();
    std::fs::write(t.path().join("lib/x/evil.obj"), OBJ).unwrap();
    let mut d = p.data.clone();
    let fp = d.footprints.get_mut("MEM_R1K").unwrap();
    fp.model.as_mut().unwrap().file = "../x/evil.obj".into();
    let fp = d.footprints.get_mut("MEM_R10K").unwrap();
    fp.model.as_mut().unwrap().file = t.path().join("evil.obj").display().to_string();
    d.save_all(p.root()).unwrap();
    drop(p);
    let mut p = Project::open_with(&t.path().join("b"), ports(vec![mem.clone()])).unwrap();
    p.set_library(Library::at(&t.path().join("lib")));
    // `cache/models/../x/evil.obj` is `cache/x/evil.obj`: put a file there so
    // that only the path check can report it.
    std::fs::create_dir_all(p.root().join("cache/x")).unwrap();
    std::fs::write(p.root().join("cache/x/evil.obj"), OBJ).unwrap();

    let missing = missing_files(&p);
    assert_eq!(missing.len(), 2, "{missing:?}");
    let r = p.restore_models(RestoreModels::default()).unwrap();
    assert_eq!(r.missing.len(), 2, "{r:?}");
    assert!(
        r.missing.iter().all(|m| m.contains("leaves cache/models")),
        "{r:?}"
    );
    assert!(r.from_library.is_empty() && r.downloaded.is_empty());
    // Nothing was written outside cache/models.
    assert!(!p.root().join("cache/x/evil.obj.tmp").exists());
    assert!(!t.path().join("x").exists());
    assert_eq!(missing_files(&p).len(), 2);
}

#[test]
fn a_workspace_library_feeds_the_scene_and_the_restore() {
    let t = tempfile::tempdir().unwrap();
    let ws = t.path().join("ws");
    std::fs::create_dir_all(ws.join("boards")).unwrap();
    let mut w = Workspace::create(&ws, "demo").unwrap();
    let mem = MemProvider::new();
    let mut p = Project::create_with(&ws.join("boards/a"), "a", ports(vec![mem.clone()])).unwrap();
    w.add_board(&ws.join("boards/a")).unwrap();
    // Downloads go to the workspace library in this test.
    p.set_library(Library {
        roots: vec![ws.join("lib")],
        store: ws.join("lib"),
    });
    import(&mut p, "mem:R1K", Some("R1")).unwrap();
    p.place(Place {
        refdes: "R1".into(),
        x: 1.0,
        y: 1.0,
        rotation: None,
        side: None,
        locked: None,
        strict: false,
    })
    .unwrap();
    std::fs::remove_dir_all(models(&p)).unwrap();
    mem.set_offline(true);

    // The scene finds the model in the workspace library before a restore.
    let s3 = lamina_scene::build_3d(&p.data, p.root());
    assert_eq!(
        s3.instances[0].model.as_deref(),
        Some(ws.join("lib/models/mem/body.obj").as_path())
    );
    assert_eq!(missing_files(&p), vec!["MEM_R1K=mem/body.obj"]);

    let r = p.restore_models(RestoreModels::default()).unwrap();
    assert_eq!(r.from_library.len(), 1, "{r:?}");
    let s3 = lamina_scene::build_3d(&p.data, p.root());
    assert_eq!(
        s3.instances[0].model.as_deref(),
        Some(models(&p).join("mem/body.obj").as_path())
    );
}

/// The recorded LCSC fixtures through the real provider adapter.
fn lcsc_ports() -> Ports {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../lamina-parts/tests/fixtures");
    let lcsc = lamina_parts::provider::LcscProvider::new(Arc::new(lamina_parts::DirSource { dir }));
    ports(vec![Arc::new(lcsc)])
}

#[test]
fn a_vendor_model_comes_back_from_the_provenance_alone() {
    let t = tempfile::tempdir().unwrap();
    let mut p = Project::create_with(&t.path().join("b"), "b", lcsc_ports()).unwrap();
    p.set_library(Library::at(&t.path().join("lib")));
    import(&mut p, "lcsc:C6186", Some("U1")).unwrap();
    let fp_id = p.data.parts.parts["U1"].footprint.clone();
    let file = p.data.footprints[&fp_id].model.clone().unwrap().file;
    let bytes = std::fs::read(models(&p).join(&file)).unwrap();
    // The footprint is all that is left: no models, no part records, an
    // empty library.
    std::fs::remove_dir_all(t.path().join("lib")).unwrap();
    std::fs::remove_dir_all(models(&p)).unwrap();
    std::fs::remove_dir_all(p.root().join(PARTS_DIR)).unwrap();
    let before = snapshot(&p);

    let r = p.restore_models(RestoreModels::default()).unwrap();
    assert_eq!(
        r.downloaded,
        vec![format!("{file} ({fp_id}) from lcsc:C6186")]
    );
    assert_eq!(std::fs::read(models(&p).join(&file)).unwrap(), bytes);
    assert!(p.status().missing_models.is_empty());
    assert_same(&before, &p);
}

#[test]
fn review_adds_images_if_the_program_has_a_renderer() {
    let t = tempfile::tempdir().unwrap();
    let mem = MemProvider::new();
    let p = board(t.path(), &mem);
    let r = p.review(Review::default()).unwrap();
    assert_eq!(
        r.images,
        vec![
            "output/review-top.png",
            "output/review-bottom.png",
            "output/review-checks.png"
        ]
    );
    let dir = p.root().join("output");
    assert!(dir.join("review-top.png").is_file());
    let text = std::fs::read_to_string(dir.join("review.md")).unwrap();
    assert!(text.contains("![The top side](review-top.png)"), "{text}");
    assert!(text.contains("](review-checks.png)"));
}
