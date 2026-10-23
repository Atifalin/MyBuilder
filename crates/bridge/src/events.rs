//! Build events emitted to the UI — one serde-typed enum, NDJSON-ready.

use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum BuildEvent {
    Started {
        /// "full" or "incremental".
        mode: String,
        stages_total: usize,
        chunks_total: usize,
    },
    StageStarted {
        stage: String,
        /// "global" | "chunk" | "region_chunk" | "output".
        group: String,
    },
    StageFinished {
        stage: String,
        group: String,
        executed: bool,
        ms: f64,
    },
    Progress {
        percent: f64,
        stage: String,
        chunks_done: usize,
        chunks_total: usize,
    },
    Diagnostic {
        severity: String,
        code: String,
        entity_id: Option<String>,
        path: String,
        message: String,
        hint: Option<String>,
    },
    FilesWritten {
        written: usize,
        unchanged: usize,
        deleted: usize,
        skipped_unsafe: usize,
    },
    Finished {
        /// "ok" | "cancelled" | "failed".
        status: String,
        error: Option<String>,
        ms: f64,
    },
}

/// Stage id -> display group.
pub fn group_of(id: &str) -> &'static str {
    if id.starts_with("world.chunk.") {
        "chunk"
    } else if id.starts_with("world.region.") {
        "region_chunk"
    } else if id == "output" || id == "write_outputs" {
        "output"
    } else {
        "global"
    }
}

/// Estimated cost weights per stage — rough numbers, tuned so progress
/// is monotone and lands on exactly 100.0 at the end. Reused stages
/// count their full weight instantly.
pub fn weight_of(id: &str) -> u64 {
    match id {
        "world.masks" => 1,
        "world.distance" => 5,
        "world.height" => 15,
        "world.layers" => 5,
        "world.manifest" => 1,
        "write_outputs" => 5,
        s if s.starts_with("world.chunk.") => 1,
        s if s.starts_with("world.region.") => 2,
        _ => 1,
    }
}
