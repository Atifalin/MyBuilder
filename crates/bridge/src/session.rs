//! Recipe editing session: validates each edit, writes world.json
//! atomically, and keeps an undo/redo stack of full WorldRecipe
//! snapshots (recipes are small; field-level diffs can come later).

use std::path::{Path, PathBuf};

use mb_schema::{Diagnostic, ProjectFile, Severity, WorldRecipe};

const HISTORY_CAP: usize = 200;

pub enum RecipeEdit {
    SetFeel(mb_schema::TerrainFeel),
    /// Some(n) sets the octaves override; None clears it.
    SetOctaves(Option<u32>),
    ReplaceWorld(Box<WorldRecipe>),
}

pub enum BridgeError {
    Io(std::io::Error),
    Schema(mb_schema::SchemaError),
    /// Edit rejected by validation; diagnostics returned via apply().
    Rejected,
}

impl std::fmt::Debug for BridgeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BridgeError::Io(e) => write!(f, "{e}"),
            BridgeError::Schema(e) => write!(f, "{e}"),
            BridgeError::Rejected => write!(f, "edit rejected"),
        }
    }
}

impl From<std::io::Error> for BridgeError {
    fn from(e: std::io::Error) -> Self {
        BridgeError::Io(e)
    }
}

pub struct Session {
    dir: PathBuf,
    project: ProjectFile,
    world: WorldRecipe,
    undo: Vec<WorldRecipe>,
    redo: Vec<WorldRecipe>,
}

impl Session {
    pub fn open(dir: &Path) -> Result<Session, BridgeError> {
        let (project, world, _) = mb_schema::load_project(dir).map_err(BridgeError::Schema)?;
        Ok(Session {
            dir: dir.to_path_buf(),
            project,
            world,
            undo: Vec::new(),
            redo: Vec::new(),
        })
    }

    pub fn world(&self) -> &WorldRecipe {
        &self.world
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }
    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    /// Validate + write + record history. A rejected edit changes
    /// neither the file nor the history.
    pub fn apply(&mut self, edit: RecipeEdit) -> Result<Vec<Diagnostic>, BridgeError> {
        let mut candidate = self.world.clone();
        match edit {
            RecipeEdit::SetFeel(f) => candidate.terrain.feel = f,
            RecipeEdit::SetOctaves(o) => candidate.terrain.overrides.octaves = o,
            RecipeEdit::ReplaceWorld(w) => candidate = *w,
        }
        let mut diags = Vec::new();
        mb_schema::validate_world(&candidate, self.project.world_scale.cell_size_m, &mut diags);
        if diags.iter().any(|d| d.severity == Severity::Error) {
            return Ok(diags); // rejected: file and history untouched
        }
        let prev = std::mem::replace(&mut self.world, candidate);
        self.undo.push(prev);
        if self.undo.len() > HISTORY_CAP {
            self.undo.remove(0);
        }
        self.redo.clear();
        self.write_world()?;
        Ok(diags)
    }

    pub fn undo(&mut self) -> Result<bool, BridgeError> {
        match self.undo.pop() {
            Some(w) => {
                self.redo.push(std::mem::replace(&mut self.world, w));
                self.write_world()?;
                Ok(true)
            }
            None => Ok(false),
        }
    }

    pub fn redo(&mut self) -> Result<bool, BridgeError> {
        match self.redo.pop() {
            Some(w) => {
                self.undo.push(std::mem::replace(&mut self.world, w));
                self.write_world()?;
                Ok(true)
            }
            None => Ok(false),
        }
    }

    fn write_world(&self) -> Result<(), BridgeError> {
        let path = self.dir.join(&self.project.recipes.world);
        let text = serde_json::to_string_pretty(&self.world).unwrap() + "\n";
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, &text)?;
        std::fs::rename(&tmp, &path)?;
        Ok(())
    }
}
