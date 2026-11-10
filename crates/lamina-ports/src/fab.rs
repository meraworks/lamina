//! Fabrication outputs: the files a board house and an assembly house take.

use lamina_geom::BoardGeometry;
use lamina_model::ProjectData;
use lamina_scene::Scene2D;
use schemars::JsonSchema;
use serde::Serialize;

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct FabInfo {
    /// The short id, for example `gerber`.
    pub id: String,
    pub name: String,
    pub description: String,
}

/// One output file.
#[derive(Debug, Clone)]
pub struct Output {
    pub name: String,
    pub contents: Vec<u8>,
}

pub trait Fabricator: Send + Sync {
    fn info(&self) -> FabInfo;
    /// The artwork files of the board: copper, mask, silk, outline, drills.
    /// These go into the zip for the board house.
    fn artwork(&self, scene: &Scene2D, geometry: &BoardGeometry, name: &str) -> Vec<Output>;
    /// Electrical test files, included in the board-house archive.
    fn netlist(&self, data: &ProjectData, name: &str) -> Result<Vec<Output>, String> {
        let _ = (data, name);
        Ok(Vec::new())
    }
    /// The assembly files: BOM and placement. The strings are warnings.
    fn assembly(&self, data: &ProjectData, name: &str) -> (Vec<Output>, Vec<String>);
    /// A job file that lists the artwork files and the board data. It goes
    /// into the archive. The default is no job file.
    fn job(&self, data: &ProjectData, artwork: &[Output], name: &str) -> Option<Output> {
        let _ = (data, artwork, name);
        None
    }
    /// One archive of the artwork files, for the board house. A fabricator
    /// that has no archive gives `None`, which is the default.
    fn archive(&self, artwork: &[Output], name: &str) -> Result<Option<Output>, String> {
        let _ = (artwork, name);
        Ok(None)
    }
}
