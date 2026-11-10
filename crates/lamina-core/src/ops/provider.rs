//! Parts from providers: search, import, and the library that makes them
//! work without a network. See `lamina_model::provider`.
//!
//! Lookup order for a part: the project cache, then the library, then the
//! provider. Each download is written to the project cache and to the
//! library store.

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Arc;

use lamina_model::error::Result;
use lamina_model::provider::{Bundle, Hit, Provider, ProviderInfo, Record, record_rel, split_name};
use lamina_model::*;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::ops::text::natural;
use crate::project::{OpResult, Project, Touched};

/// The cache folder of a project. `cache/parts` and `cache/models` are in it.
const CACHE_DIR: &str = "cache";

/// The providers that a project can use, in the search order.
#[derive(Clone, Default)]
pub struct Providers {
    list: Vec<Arc<dyn Provider>>,
}

impl Providers {
    pub fn new(list: Vec<Arc<dyn Provider>>) -> Providers {
        Providers { list }
    }

    pub fn get(&self, id: &str) -> Option<&dyn Provider> {
        self.list
            .iter()
            .find(|p| p.info().id == id)
            .map(|p| p.as_ref())
    }

    pub fn infos(&self) -> Vec<ProviderInfo> {
        self.list.iter().map(|p| p.info()).collect()
    }

    pub fn iter(&self) -> impl Iterator<Item = &dyn Provider> {
        self.list.iter().map(|p| p.as_ref())
    }

    fn ids(&self) -> String {
        self.list
            .iter()
            .map(|p| p.info().id)
            .collect::<Vec<_>>()
            .join(", ")
    }

    fn need(&self, id: &str) -> Result<&dyn Provider> {
        self.get(id).ok_or_else(|| {
            Error::invalid(format!(
                "There is no provider `{id}`. The providers are: {}.",
                self.ids()
            ))
        })
    }
}

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct SearchParts {
    /// Keywords, an MPN, a part number or a footprint name.
    pub query: String,
    /// Search only this provider. The default is all providers.
    #[serde(default)]
    pub provider: Option<String>,
    /// The maximum number of results for each provider (default 10).
    #[serde(default)]
    pub limit: Option<usize>,
    /// Keep only the parts with no extra assembly fee.
    #[serde(default)]
    pub basic_only: bool,
    /// Remove the vendor parts that have no stock (default true).
    #[serde(default = "super::yes")]
    pub in_stock: bool,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct PartSearch {
    pub hits: Vec<Hit>,
    /// What did not work, for example a provider without a network. The
    /// hits of that provider then come from the library.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub notes: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct ImportPart {
    /// The part as `provider:id`, for example `lcsc:C6186` or
    /// `kicad:LED_SMD:LED_0603_1608Metric`. The `providers` tool lists the
    /// providers and their id formats.
    pub part: String,
    /// Also add a part with this refdes.
    #[serde(default)]
    pub refdes: Option<String>,
    /// The part value (default: the MPN or the footprint name).
    #[serde(default)]
    pub value: Option<String>,
    /// The footprint id to use instead of the provider's name.
    #[serde(default)]
    pub footprint_id: Option<String>,
    /// Get the data again. Do not use the project cache or the library.
    #[serde(default)]
    pub refresh: bool,
    /// Also get the 3D model (default true).
    #[serde(default = "super::yes")]
    pub model: bool,
    /// Replace a footprint or a part that has the same id.
    #[serde(default)]
    pub replace: bool,
    #[serde(default)]
    pub strict: bool,
}

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct ImportParts {
    pub parts: Vec<ImportPart>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize, JsonSchema)]
pub struct FetchLibrary {
    /// Parts as `provider:id`, for example `["lcsc:C6186", "kicad:LED_SMD:LED_0603_1608Metric"]`.
    pub parts: Vec<String>,
    /// Download again, also when the library has the part.
    #[serde(default)]
    pub refresh: bool,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct Fetched {
    /// The folder that got the files.
    pub library: String,
    /// The parts that are now in the library.
    pub done: Vec<String>,
    /// The parts that were in the library already.
    pub kept: Vec<String>,
    /// The parts that failed, with the reason.
    pub failed: Vec<String>,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct LibraryInfo {
    /// The folder that gets new downloads.
    pub store: String,
    /// All library folders, in the search order.
    pub roots: Vec<String>,
    /// The parts in the library, as `provider:id`.
    pub parts: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct RestoreModels {
    /// Download from the provider a model that the part library does not
    /// have (default true).
    #[serde(default = "super::yes")]
    pub download: bool,
}

impl Default for RestoreModels {
    fn default() -> Self {
        RestoreModels { download: true }
    }
}

#[derive(Debug, Clone, Default, Serialize, JsonSchema)]
pub struct ModelsRestored {
    /// The models that were in `cache/models` already.
    pub present: usize,
    /// The models copied from the part library, with their footprints.
    pub from_library: Vec<String>,
    /// The models downloaded again from a provider, with their footprints.
    pub downloaded: Vec<String>,
    /// The models that are still missing, with the reason.
    pub missing: Vec<String>,
}

/// A footprint whose model file is not in `cache/models`.
#[derive(Debug, Clone, PartialEq, Serialize, JsonSchema)]
pub struct MissingModel {
    pub footprint: String,
    /// The model path relative to `cache/models`.
    pub file: String,
}

/// Where a model file came from.
enum Restored {
    Present,
    Library,
    /// Downloaded from this part (`provider:id`).
    Provider(String),
}

/// True if a model path stays in `cache/models`: relative, and with no `..`.
pub fn model_path_ok(file: &str) -> bool {
    !file.is_empty()
        && file.split(['/', '\\']).all(|s| s != "..")
        && Path::new(file)
            .components()
            .all(|c| matches!(c, std::path::Component::Normal(_)))
}

/// Search the records of the library by words in the id, the MPN, the
/// manufacturer, the package or the description. All words must match.
pub fn search_library(lib: &Library, provider: Option<&str>, query: &str) -> Vec<Hit> {
    let words: Vec<String> = query
        .split_whitespace()
        .map(|w| w.to_ascii_lowercase())
        .collect();
    lib.records(provider)
        .into_iter()
        .filter(|r| {
            let i = &r.info;
            let hay = [
                Some(r.id.as_str()),
                i.mpn.as_deref(),
                i.manufacturer.as_deref(),
                i.package.as_deref(),
                i.description.as_deref(),
            ]
            .into_iter()
            .flatten()
            .collect::<Vec<_>>()
            .join(" ")
            .to_ascii_lowercase();
            words.iter().all(|w| hay.contains(w.as_str()))
        })
        .map(|r| Hit {
            provider: r.provider.clone(),
            id: r.id.clone(),
            title: r.info.mpn.clone().unwrap_or_else(|| r.footprint.id.clone()),
            package: r.info.package.clone(),
            manufacturer: r.info.manufacturer.clone(),
            description: r.info.description.clone(),
            stock: None,
            price: None,
            basic: r.info.basic,
            datasheet: r.info.datasheet.clone(),
            local: true,
        })
        .collect()
}

pub fn library_info(lib: &Library) -> LibraryInfo {
    LibraryInfo {
        store: lib.store.display().to_string(),
        roots: lib.roots.iter().map(|r| r.display().to_string()).collect(),
        parts: lib
            .records(None)
            .iter()
            .map(|r| format!("{}:{}", r.provider, r.id))
            .collect(),
    }
}

/// Write a bundle as a record plus a model file under `base`: the `cache`
/// folder of a project, or a library root. Both have `parts/` and `models/`.
fn store_bundle(
    base: &Path,
    rec: &Record,
    model: Option<&lamina_model::provider::ModelFile>,
) -> Result<()> {
    if let (Some(m), Some(r)) = (model, rec.model()) {
        write_atomic(&base.join("models").join(&r.file), &m.bytes)?;
    }
    let text = serde_json::to_string(rec).map_err(|e| Error::invalid(e.to_string()))?;
    write_atomic(
        &base.join(record_rel(&rec.provider, &rec.id)),
        text.as_bytes(),
    )
}

/// Get a part into the library store only (no project).
pub fn fetch(lib: &Library, providers: &Providers, p: &FetchLibrary) -> Result<Fetched> {
    let mut r = Fetched {
        library: lib.store.display().to_string(),
        done: Vec::new(),
        kept: Vec::new(),
        failed: Vec::new(),
    };
    for name in &p.parts {
        let Some((pid, id)) = split_name(name) else {
            r.failed.push(format!(
                "{name}: a part is `provider:id` ({})",
                providers.ids()
            ));
            continue;
        };
        let full = format!("{pid}:{id}");
        let rel = record_rel(pid, id);
        if !p.refresh && lib.find(&rel).is_some() {
            r.kept.push(full);
            continue;
        }
        let res = providers
            .need(pid)
            .and_then(|prov| prov.fetch(id, true).map_err(Error::Invalid))
            .and_then(|b| {
                let rec = record_of(pid, id, b.clone());
                store_bundle(&lib.store, &rec, b.model.as_ref())
            });
        match res {
            Ok(()) => r.done.push(full),
            Err(e) => r.failed.push(format!("{full}: {e}")),
        }
    }
    Ok(r)
}

/// Make the record of a bundle. The model goes to `models/<provider>/<name>`.
fn record_of(provider: &str, id: &str, b: Bundle) -> Record {
    let mut fp = b.footprint;
    if let Some(m) = &b.model {
        fp.model = Some(ModelRef {
            file: format!("{provider}/{}", m.name),
            offset: m.offset,
            rotation: m.rotation,
            scale: m.scale,
        });
        if m.height.is_some() {
            fp.height = m.height;
        }
    }
    Record {
        provider: provider.into(),
        id: id.into(),
        footprint: fp,
        info: b.info,
        notes: b.notes,
    }
}

impl Project {
    pub fn providers(&self) -> &Providers {
        &self.providers
    }

    pub fn provider_infos(&self) -> Vec<ProviderInfo> {
        self.providers().infos()
    }

    pub fn search_parts(&self, p: SearchParts) -> Result<PartSearch> {
        if p.query.trim().is_empty() {
            return Err(Error::invalid(
                "The query is empty. Give one or more keywords.",
            ));
        }
        let limit = p.limit.unwrap_or(10).clamp(1, 50);
        let query = p.query.trim();
        let chosen: Vec<&dyn Provider> = match &p.provider {
            Some(id) => vec![self.providers().need(id)?],
            None => self.providers().iter().collect(),
        };
        let mut hits = Vec::new();
        let mut notes = Vec::new();
        for prov in chosen {
            let id = prov.info().id;
            match prov.search(query, limit * 2) {
                Ok(mut h) => {
                    if p.in_stock {
                        h.retain(|x| x.stock.is_none_or(|s| s > 0));
                    }
                    h.truncate(limit);
                    hits.extend(h);
                }
                Err(e) => {
                    let mut local = search_library(self.library(), Some(&id), query);
                    local.truncate(limit);
                    if local.is_empty() {
                        notes.push(format!("{id}: {e}"));
                    } else {
                        notes.push(format!(
                            "{id}: {e}. These hits come from the local library; stock and price are not known."
                        ));
                        hits.extend(local);
                    }
                }
            }
        }
        if p.basic_only {
            hits.retain(|h| h.basic == Some(true));
        }
        if hits.is_empty() && !notes.is_empty() {
            return Err(Error::Invalid(notes.join(" ")));
        }
        Ok(PartSearch { hits, notes })
    }

    /// Copy the model file of a record from the library when the cache does
    /// not have it. Else, with `download`, get it from the provider again.
    /// If no copy exists, the record loses its model.
    fn ensure_model_in_cache(&self, rec: &mut Record, download: bool, notes: &mut Vec<String>) {
        let Some(m) = rec.model().cloned() else {
            return;
        };
        let source = download.then_some((rec.provider.as_str(), rec.id.as_str()));
        match self.restore_model(&m.file, source) {
            Ok(Restored::Present | Restored::Library) => {}
            Ok(Restored::Provider(name)) => notes.push(format!(
                "The model file {} was missing. It was downloaded again from {name}.",
                m.file
            )),
            Err(e) => {
                notes.push(format!(
                    "The model file {} is not in the cache. {e} The part has no model.",
                    m.file
                ));
                rec.footprint.model = None;
            }
        }
    }

    /// Put a missing model file into `cache/models`: a copy from the library,
    /// else a download of the part `source`. A file that is there is kept.
    fn restore_model(
        &self,
        file: &str,
        source: Option<(&str, &str)>,
    ) -> std::result::Result<Restored, String> {
        if !model_path_ok(file) {
            return Err("The path leaves cache/models, so Lamina does not use it.".into());
        }
        let dst = self.root().join(MODEL_DIR).join(file);
        if dst.is_file() {
            return Ok(Restored::Present);
        }
        if let Some(src) = self.library().find(&format!("models/{file}")) {
            let bytes = std::fs::read(&src).map_err(|e| format!("{}: {e}", src.display()))?;
            write_atomic(&dst, &bytes).map_err(|e| e.to_string())?;
            return Ok(Restored::Library);
        }
        let Some((pid, id)) = source else {
            return Err("It is not in the part library.".into());
        };
        let prov = self.providers().need(pid).map_err(|e| e.to_string())?;
        let b = prov.fetch(id, true).map_err(|e| {
            format!(
                "It is not in the part library, and {pid}:{id} failed: {}.",
                e.trim_end_matches('.')
            )
        })?;
        let m = b.model.ok_or_else(|| {
            format!("It is not in the part library, and {pid}:{id} gave no model.")
        })?;
        let got = format!("{pid}/{}", m.name);
        if got != file {
            return Err(format!(
                "{pid}:{id} now gives the model {got}. Import the part with refresh: true to use it."
            ));
        }
        write_atomic(&dst, &m.bytes).map_err(|e| e.to_string())?;
        let _ = self.library().put(&format!("models/{file}"), &m.bytes);
        Ok(Restored::Provider(format!("{pid}:{id}")))
    }

    /// The footprints whose model file is not in `cache/models`.
    pub fn missing_models(&self) -> Vec<MissingModel> {
        self.data
            .footprints
            .values()
            .filter_map(|f| {
                let m = f.model.as_ref()?;
                let there =
                    model_path_ok(&m.file) && self.root().join(MODEL_DIR).join(&m.file).is_file();
                (!there).then(|| MissingModel {
                    footprint: f.id.clone(),
                    file: m.file.clone(),
                })
            })
            .collect()
    }

    /// The part that a model came from, as (provider, id): a record in the
    /// project cache or in the library with this model, else the provenance
    /// of the footprint.
    fn model_source(records: &[Record], fp: &Footprint, file: &str) -> Option<(String, String)> {
        if let Some(r) = records
            .iter()
            .find(|r| r.model().is_some_and(|m| m.file == file))
        {
            return Some((r.provider.clone(), r.id.clone()));
        }
        let (pid, id) = match &fp.provenance {
            Provenance::Lcsc { part, .. } => ("lcsc".to_string(), part.clone()),
            Provenance::Kicad {
                library, footprint, ..
            } if !library.is_empty() => ("kicad".to_string(), format!("{library}:{footprint}")),
            _ => return None,
        };
        file.starts_with(&format!("{pid}/")).then_some((pid, id))
    }

    /// Put every missing model of the footprints back into `cache/models`:
    /// from the part library, else from the provider of the footprint. The
    /// footprints do not change, and files that are there are kept.
    pub fn restore_models(&self, p: RestoreModels) -> Result<ModelsRestored> {
        let mut files: BTreeMap<String, Vec<String>> = BTreeMap::new();
        for f in self.data.footprints.values() {
            if let Some(m) = &f.model {
                files.entry(m.file.clone()).or_default().push(f.id.clone());
            }
        }
        let mut r = ModelsRestored::default();
        let mut records = None;
        for (file, fps) in &files {
            let label = format!("{file} ({})", fps.join(", "));
            let there = model_path_ok(file) && self.root().join(MODEL_DIR).join(file).is_file();
            if there {
                r.present += 1;
                continue;
            }
            let source = if p.download && model_path_ok(file) {
                let records = records.get_or_insert_with(|| {
                    let mut v = Library::at(&self.root().join(CACHE_DIR)).records(None);
                    v.extend(self.library().records(None));
                    v
                });
                fps.iter()
                    .find_map(|id| Self::model_source(records, &self.data.footprints[id], file))
            } else {
                None
            };
            let res =
                self.restore_model(file, source.as_ref().map(|(a, b)| (a.as_str(), b.as_str())));
            match res {
                Ok(Restored::Present) => r.present += 1,
                Ok(Restored::Library) => r.from_library.push(label),
                Ok(Restored::Provider(name)) => r.downloaded.push(format!("{label} from {name}")),
                Err(e) if p.download && source.is_none() && model_path_ok(file) => r.missing.push(
                    format!("{label}: {e} The footprint did not come from a provider."),
                ),
                Err(e) => r.missing.push(format!("{label}: {e}")),
            }
        }
        Ok(r)
    }

    /// The footprint of a part from a provider, as a second source for a
    /// comparison. With `online` false, only the project cache and the part
    /// library are used: no network.
    pub(crate) fn source_footprint(&self, pid: &str, id: &str, online: bool) -> Option<Footprint> {
        let rel = record_rel(pid, id);
        let cache = self.root().join(CACHE_DIR).join(&rel);
        if cache.is_file() {
            return read_record(&cache).ok().map(|r| r.footprint);
        }
        if let Some(path) = self.library().find(&rel) {
            return read_record(&path).ok().map(|r| r.footprint);
        }
        if !online {
            return None;
        }
        let prov = self.providers().get(pid)?;
        prov.fetch(id, false)
            .ok()
            .map(|b| record_of(pid, id, b).footprint)
    }

    /// The record of a part: from the project cache, the library, or the
    /// provider. A download is written to both.
    fn fetch_record(
        &self,
        pid: &str,
        id: &str,
        refresh: bool,
        model: bool,
        notes: &mut Vec<String>,
    ) -> Result<Record> {
        let rel = record_rel(pid, id);
        let cache = self.root().join(CACHE_DIR).join(&rel);
        let lib = self.library();
        if !refresh && cache.is_file() {
            let mut rec = read_record(&cache)?;
            self.ensure_model_in_cache(&mut rec, model, notes);
            return Ok(rec);
        }
        if let Some(p) = (!refresh).then(|| lib.find(&rel)).flatten() {
            let mut rec = read_record(&p)?;
            self.ensure_model_in_cache(&mut rec, model, notes);
            store_bundle(&self.root().join(CACHE_DIR), &rec, None)?;
            notes.push(format!(
                "{pid}:{id} came from the library ({})",
                p.display()
            ));
            return Ok(rec);
        }
        let prov = self.providers().need(pid)?;
        let b = prov.fetch(id, model).map_err(|e| {
            Error::Invalid(format!(
                "{e}. `{pid}:{id}` is not in the project cache or in the library. \
                 Run `lamina fetch {pid}:{id}` on a computer with a network to put it in the library."
            ))
        })?;
        let mut rec = record_of(pid, id, b.clone());
        store_bundle(&self.root().join(CACHE_DIR), &rec, b.model.as_ref())?;
        // Check that the model loads. Take the height from the mesh when the
        // provider did not know it.
        if let Some(m) = rec.model().cloned() {
            let path = self.root().join(MODEL_DIR).join(&m.file);
            match self.mesh_height(&path) {
                Ok(h) => {
                    if rec.footprint.height.is_none() {
                        rec.footprint.height = h;
                    }
                    rec.notes.push(format!("3D model {}", m.file));
                }
                Err(e) => {
                    rec.notes.push(format!(
                        "The 3D model did not load, so the part has none: {e}"
                    ));
                    rec.footprint.model = None;
                }
            }
            store_bundle(&self.root().join(CACHE_DIR), &rec, None)?;
        }
        if let Err(e) = store_bundle(&lib.store, &rec, b.model.as_ref()) {
            notes.push(format!("The library did not get a copy: {e}"));
        }
        Ok(rec)
    }

    pub fn import_part(&mut self, p: ImportPart) -> Result<OpResult> {
        let (pid, id) = split_name(&p.part).ok_or_else(|| {
            Error::invalid(format!(
                "`{}` is not a part name. Use `provider:id`, for example `lcsc:C6186`. The providers are: {}.",
                p.part,
                self.providers().ids()
            ))
        })?;
        let (pid, id) = (pid.to_string(), id.to_string());
        let mut notes = Vec::new();
        let rec = self.fetch_record(&pid, &id, p.refresh, p.model, &mut notes)?;
        let mut fp = rec.footprint.clone();
        if let Some(fid) = &p.footprint_id {
            fp.id = fid.clone();
        }
        if !valid_id(&fp.id) {
            return Err(Error::invalid(format!(
                "The footprint id `{}` cannot be used as a file name. Set footprint_id to a different id.",
                fp.id
            )));
        }
        notes.extend(rec.notes.iter().cloned());
        let mut d = self.data.clone();
        let mut touched = Touched::default();
        let fp_id = fp.id.clone();
        if d.footprints.contains_key(&fp_id) && !p.replace && !p.refresh {
            notes.push(format!(
                "The footprint `{fp_id}` is already in the project. It is kept as is."
            ));
        } else {
            if !p.model {
                fp.model = None;
            }
            touched = touched.footprint(fp_id.clone());
            d.footprints.insert(fp_id.clone(), fp);
        }

        let fp = &d.footprints[&fp_id];
        let pads: Vec<String> = {
            let mut v: Vec<&Pad> = fp.pads.iter().filter(|x| !x.number.is_empty()).collect();
            v.sort_by(|a, b| natural(&a.number, &b.number));
            v.dedup_by(|a, b| a.number == b.number);
            v.iter()
                .map(|x| match &x.name {
                    Some(n) => format!("{}={n}", x.number),
                    None => x.number.clone(),
                })
                .collect()
        };
        let info = &rec.info;
        let class = match info.basic {
            Some(true) => ", basic",
            Some(false) => ", extended",
            None => "",
        };
        let mut summary = format!(
            "{pid}:{id} {} ({}){class}: footprint `{fp_id}` with {} pads [{}]",
            info.mpn.as_deref().unwrap_or(&fp_id),
            info.package.as_deref().unwrap_or("?"),
            fp.pads.len(),
            pads.join(" ")
        );
        let default_value = info.mpn.clone().unwrap_or_else(|| fp_id.clone());

        let parts_generated = self.source_owned().contains(&FileKind::Parts);
        if let (Some(r), true) = (p.refdes.as_deref(), parts_generated) {
            let q = |s: &str| s.replace('\\', "\\\\").replace('\'', "\\'");
            let mut fields = vec![format!("footprint: '{}'", q(&fp_id))];
            fields.push(format!(
                "value: '{}'",
                q(p.value.as_deref().unwrap_or(&default_value))
            ));
            if let Some(m) = &info.mpn {
                fields.push(format!("mpn: '{}'", q(m)));
            }
            if let Some(m) = &info.manufacturer {
                fields.push(format!("manufacturer: '{}'", q(m)));
            }
            if let Some(c) = &info.order_code {
                fields.push(format!("lcsc: '{}'", q(c)));
            }
            notes.push(format!(
                "parts.json is generated from {}. Add the part there: {r}: {{ {} }}",
                self.source_name().unwrap_or("the design source"),
                fields.join(", ")
            ));
            summary = format!("{r} not added (see notes): {summary}");
        } else if let Some(r) = p.refdes {
            if !valid_refdes(&r) {
                return Err(Error::invalid(format!("The refdes `{r}` is not valid.")));
            }
            if d.parts.parts.contains_key(&r) && !p.replace {
                return Err(Error::invalid(format!(
                    "{r} already exists. Set replace: true to change it."
                )));
            }
            let mut part = Part::new(fp_id.clone());
            part.value = Some(p.value.unwrap_or(default_value));
            part.mpn = info.mpn.clone();
            part.manufacturer = info.manufacturer.clone();
            part.lcsc = info.order_code.clone();
            part.description = info.description.clone();
            d.parts.parts.insert(r.clone(), part);
            touched = touched.and(FileKind::Parts);
            summary = format!("{r}: {summary}");
        }
        let mut res = self.commit(d, touched, p.strict, summary)?;
        res.notes = notes;
        Ok(res)
    }

    /// Import several parts in one call. Stop at the first error.
    pub fn import_parts(&mut self, p: ImportParts) -> Result<Vec<OpResult>> {
        let mut out = Vec::new();
        for item in p.parts {
            let name = item.part.clone();
            out.push(
                self.import_part(item)
                    .map_err(|e| Error::invalid(format!("{name}: {e}")))?,
            );
        }
        Ok(out)
    }

    /// Fill the library with the providers of this project.
    pub fn fetch_library(&self, p: FetchLibrary) -> Result<Fetched> {
        fetch(self.library(), self.providers(), &p)
    }

    pub fn library_info(&self) -> LibraryInfo {
        library_info(self.library())
    }

    /// A model from a provider, written into `cache/models/<provider>/` and
    /// into the library. Gives the model reference and the top of the model
    /// above the board.
    pub(crate) fn provider_model(&self, name: &str) -> Result<(ModelRef, Option<f64>)> {
        let (pid, id) = split_name(name).ok_or_else(|| {
            Error::invalid(format!(
                "`{name}` is not `provider:id`. The providers are: {}.",
                self.providers().ids()
            ))
        })?;
        let prov = self.providers().need(pid)?;
        let m = prov.model(id).map_err(|e| {
            Error::Invalid(format!(
                "{e}. Run `lamina fetch {pid}:{id}` on a computer with a network to put it in the library."
            ))
        })?;
        let rel = format!("{pid}/{}", m.name);
        let path = self.root().join(MODEL_DIR).join(&rel);
        write_atomic(&path, &m.bytes)?;
        let _ = self.library().put(&format!("models/{rel}"), &m.bytes);
        let height = match m.height {
            Some(h) => Some(h),
            None => self.mesh_height(&path).map_err(Error::Invalid)?,
        };
        Ok((
            ModelRef {
                file: rel,
                offset: m.offset,
                rotation: m.rotation,
                scale: m.scale,
            },
            height,
        ))
    }

    /// A model from a provider, written into `cache/models/<provider>/`.
    /// Gives the path relative to `cache/models/`.
    pub(crate) fn provider_model_into_cache(&self, name: &str) -> Result<String> {
        Ok(self.provider_model(name)?.0.file)
    }
}

impl Project {
    /// The top of a model file above the board, through the mesh loader port.
    fn mesh_height(&self, path: &Path) -> std::result::Result<Option<f64>, String> {
        let mesh = self
            .load_mesh(path)
            .map_err(|e| format!("The 3D model did not load: {e}"))?;
        Ok(mesh.bounds().map(|(_, hi)| json::round6(hi[2] as f64)))
    }
}

fn read_record(path: &Path) -> Result<Record> {
    let text = std::fs::read_to_string(path).map_err(|e| Error::io(path, e))?;
    serde_json::from_str(&text).map_err(|e| Error::invalid(format!("{}: {e}", path.display())))
}
