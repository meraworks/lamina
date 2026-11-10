/// The serde default for a `bool` that starts as true.
pub(crate) fn yes() -> bool {
    true
}

pub mod board;
pub mod evidence;
pub use evidence::{Audit, AuditRow, CompareFootprints, Lead, Verified, VerifyFootprint};
pub mod exclusions;
pub mod fixes;
pub use exclusions::{ExcludeViolation, IncludeViolation};
pub use fixes::{ApplyFixes, FixReport, FixesApplied};
pub mod format;
pub mod import;
pub mod layout;
pub mod nets;
pub mod pairs;
pub use pairs::{RouteDiffPair, SetDiffPairs};
pub mod output;
pub mod parts;
pub mod place;
pub mod provider;
pub mod render;
pub mod review;
pub mod route;
pub mod stats;
pub mod status;
pub use stats::{BoardStats, DrillStats, FabCompat};
pub mod teardrops;
pub mod text;
pub mod tuning;
pub use tuning::TuneLength;

pub use board::{RectSpec, SetOutline, SetRules, SetStackup, UseFabPreset};
pub use format::{
    BoardExported, ExportBoard, ExportForRouter, IMPORT_DIR, ImportBoard, ImportRouterSession,
    RouterExported,
};
pub use import::{
    board_file_kind_in, import_board_file, import_dir_in, imports_dir, open_board_path_in,
};
pub use layout::{LayoutSaved, PlaceLayoutBlock, RepeatLayout, SaveLayoutBlock};
pub use nets::{Connect, Disconnect, SetNets};
pub use output::{AddNote, Export, ExportStep, Exported, ListNotes, ResolveNotes, StepExported};
pub use parts::{
    AddPart, AddParts, BodySize, FootprintFromPackage, FootprintFromPads, FootprintId,
    FootprintSummary, PinRow, Pins, RemovePart, SetModel,
};
pub use place::{
    Align, Axis, Distribute, FlipPart, Measure, Measurement, MoveParts, Place, PlaceMany,
    RotatePart, Target, Unplace,
};
pub use provider::{
    FetchLibrary, Fetched, ImportPart, ImportParts, LibraryInfo, MissingModel, ModelsRestored,
    PartSearch, Providers, RestoreModels, SearchParts,
};
pub use render::{Render, RenderView, Rendered, parse_scene_layer};
pub use review::{BoardDiff, Review, Reviewed, Verdict};
pub use route::{
    AddArea, AddHole, AddKeepout, AddText, AddVia, AddZone, Autoroute, DeleteItems, Ripup,
    RouteTrace,
};
pub use status::Status;
pub use text::{natural, today};
