//! Board file formats: read a board from another program, write one for it.

use std::collections::BTreeMap;
use std::path::Path;

use lamina_model::ProjectData;
use schemars::JsonSchema;
use serde::Serialize;

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct FormatInfo {
    /// The short id, for example `kicad`.
    pub id: String,
    pub name: String,
    /// What the format holds and where it comes from.
    pub description: String,
    /// File extensions without the dot, lower case.
    pub extensions: Vec<String>,
    pub read: bool,
    pub write: bool,
}

/// A board as a format reads it. The data is a complete Lamina board. The
/// models are what the footprints need from a provider, as `provider:id`,
/// so that the core can get them through the provider port.
#[derive(Debug)]
pub struct ImportedBoard {
    pub data: ProjectData,
    /// Footprint id to the model to get, as `provider:id`. The format can
    /// put more than one candidate; the core takes the first that works.
    pub models: BTreeMap<String, Vec<String>>,
    /// Files to keep next to the board, by name: artwork that the format
    /// read but did not convert (for example the Gerber layers).
    pub extras: BTreeMap<String, Vec<u8>>,
    pub notes: Vec<String>,
    /// The data of the source that is not in the board. The core keeps the
    /// losses that change the fabrication files as open notes.
    pub losses: Vec<Loss>,
}

/// What an import dropped.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum LossKind {
    /// A full footprint.
    Footprint,
    /// A pad, or a part of the shape of a pad.
    Pad,
    /// A track or a via.
    Track,
    /// A zone, or the connection type of a zone.
    Zone,
    /// A keep-out.
    Keepout,
    /// A drawing on the board edge layer.
    Edge,
    /// Records of a file that the reader did not understand.
    Records,
    /// Custom rules of the source program.
    Rules,
    /// Texts and drawings on layers that do not change the board.
    Other,
}

impl LossKind {
    /// True if the loss can change the copper, the holes, the mask, the
    /// paste or the outline of the board.
    pub fn changes_fab(self) -> bool {
        !matches!(self, LossKind::Rules | LossKind::Other)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Loss {
    pub kind: LossKind,
    /// A sentence for the user.
    pub what: String,
}

impl Loss {
    pub fn new(kind: LossKind, what: impl Into<String>) -> Loss {
        Loss {
            kind,
            what: what.into(),
        }
    }
}

/// Record that an import dropped data: one note for the user and one loss
/// with its kind. An adapter calls this at the place where it drops the data.
pub fn lost(notes: &mut Vec<String>, losses: &mut Vec<Loss>, kind: LossKind, text: String) {
    losses.push(Loss::new(kind, text.clone()));
    notes.push(text);
}

pub trait BoardFormat: Send + Sync {
    fn info(&self) -> FormatInfo;
    /// True if this format can read `path` (a file or a folder).
    fn detect(&self, path: &Path) -> bool;
    /// The path that holds the whole board for `path`. A format whose board
    /// is a set of files gives the folder of a single file. The default is
    /// `path` itself.
    fn source_root(&self, path: &Path) -> std::path::PathBuf {
        path.to_path_buf()
    }
    /// Read the board at `path`. `name` is the board name to use.
    /// `project_dir` is the folder that holds `path`, for files next to it.
    fn read(&self, path: &Path, name: &str) -> Result<ImportedBoard, String>;
    /// Write the board as text. `model_dir` holds the model files of the
    /// board, for formats that reference them.
    fn write(&self, data: &ProjectData, model_dir: &Path) -> Result<String, String> {
        let _ = (data, model_dir);
        Err(format!("The format `{}` cannot write.", self.info().id))
    }
    /// Other files to write next to the board at `path`, with their text.
    fn companions(&self, data: &ProjectData, path: &Path) -> Vec<(std::path::PathBuf, String)> {
        let _ = (data, path);
        Vec::new()
    }
    /// Validate or merge a companion before any export files are written.
    fn prepare_companion(
        &self,
        path: &Path,
        generated: &str,
        existing: Option<&str>,
    ) -> Result<String, String> {
        let _ = (path, existing);
        Ok(generated.to_owned())
    }
}
