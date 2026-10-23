//! The UI build entry: wraps `mb_terrain::build` with BuildEvents.

use std::path::{Path, PathBuf};

use crate::events::{BuildEvent, group_of, weight_of};
use mb_terrain::build::{BuildError, BuildOptions, BuildReport};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BuildMode {
    /// Don't READ the cache (still write it) — recompute everything.
    Full,
    Incremental,
}

pub struct BuildRequest {
    pub mode: BuildMode,
    /// Cache override; None = the project OS cache dir.
    pub cache_dir: Option<PathBuf>,
}

pub struct BuildOutcome {
    pub report: Option<BuildReport>,
    /// "ok" | "cancelled" | "failed"
    pub status: &'static str,
    pub error: Option<String>,
}

fn diag_event(d: &mb_schema::Diagnostic) -> BuildEvent {
    BuildEvent::Diagnostic {
        severity: d.severity.to_string(),
        code: d.code.to_string(),
        entity_id: d.entity_id.clone(),
        path: d.path.clone(),
        message: d.message.clone(),
        hint: d.suggestion.clone(),
    }
}

/// Run a build, emitting events to `sink`.
pub fn build(
    dir: &Path,
    req: &BuildRequest,
    cancel: &mb_pipeline::CancelToken,
    sink: &mut dyn FnMut(BuildEvent),
) -> BuildOutcome {
    let t0 = std::time::Instant::now();
    // Validation first — errors become Diagnostic events + failed.
    let (_project, world, diags) = match mb_schema::load_project(dir) {
        Ok(x) => x,
        Err(e) => {
            sink(BuildEvent::Finished {
                status: "failed".to_string(),
                error: Some(e.to_string()),
                ms: t0.elapsed().as_secs_f64() * 1000.0,
            });
            return BuildOutcome {
                report: None,
                status: "failed",
                error: Some(e.to_string()),
            };
        }
    };
    for d in &diags {
        sink(diag_event(d));
    }
    if diags
        .iter()
        .any(|d| d.severity == mb_schema::Severity::Error)
    {
        sink(BuildEvent::Finished {
            status: "failed".to_string(),
            error: Some("validation failed".to_string()),
            ms: t0.elapsed().as_secs_f64() * 1000.0,
        });
        return BuildOutcome {
            report: None,
            status: "failed",
            error: Some("validation failed".to_string()),
        };
    }
    let ncx = (world.extent.width_m / world.chunk_size_m).round() as usize;
    let ncz = (world.extent.depth_m / world.chunk_size_m).round() as usize;
    let chunks_total = ncx * ncz
        + world
            .detail_regions
            .iter()
            .map(|r| {
                ((r.chunk_max[0] - r.chunk_min[0] + 1) * (r.chunk_max[1] - r.chunk_min[1] + 1))
                    as usize
            })
            .sum::<usize>();
    let mode_str = if req.mode == BuildMode::Full {
        "full"
    } else {
        "incremental"
    };
    sink(BuildEvent::Started {
        mode: mode_str.to_string(),
        stages_total: 5 + chunks_total,
        chunks_total,
    });
    // Progress accounting: fixed estimated weights (see weight_of).
    let mut total_w = 0u64;
    for _ in 0..ncx * ncz {
        total_w += 1;
    }
    for r in &world.detail_regions {
        total_w += 2
            * (((r.chunk_max[0] - r.chunk_min[0] + 1) * (r.chunk_max[1] - r.chunk_min[1] + 1))
                as u64);
    }
    total_w += 1 + 5 + 15 + 5 + 1 + 5; // globals + write
    let mut done_w = 0u64;
    let mut chunks_done = 0usize;
    let mut cur_stage = String::new();
    let mut obs = |e: mb_pipeline::PipelineEvent| match e {
        mb_pipeline::PipelineEvent::StageStarted(id) => {
            cur_stage = id.clone();
            sink(BuildEvent::StageStarted {
                stage: id.clone(),
                group: group_of(&id).to_string(),
            });
        }
        mb_pipeline::PipelineEvent::StageFinished { id, executed, ms } => {
            let g = group_of(&id);
            sink(BuildEvent::StageFinished {
                stage: id.clone(),
                group: g.to_string(),
                executed,
                ms,
            });
            done_w += weight_of(&id);
            if g == "chunk" || g == "region_chunk" {
                chunks_done += 1;
            }
            sink(BuildEvent::Progress {
                percent: (done_w as f64 / total_w as f64 * 100.0).min(99.0),
                stage: id,
                chunks_done,
                chunks_total,
            });
        }
    };
    let opts = BuildOptions {
        clean: false,
        full: req.mode == BuildMode::Full,
        cache_dir: req.cache_dir.clone(),
    };
    let out = mb_terrain::build::build_project_observed(dir, &opts, cancel, &mut obs);
    match out {
        Ok(rep) => {
            if rep.cancelled {
                sink(BuildEvent::Finished {
                    status: "cancelled".to_string(),
                    error: None,
                    ms: t0.elapsed().as_secs_f64() * 1000.0,
                });
                return BuildOutcome {
                    report: Some(rep),
                    status: "cancelled",
                    error: None,
                };
            }
            for d in &rep.diagnostics {
                sink(diag_event(d));
            }
            sink(BuildEvent::StageStarted {
                stage: "write_outputs".to_string(),
                group: "output".to_string(),
            });
            let wt0 = std::time::Instant::now();
            let wr = &rep.write;
            sink(BuildEvent::FilesWritten {
                written: wr.written,
                unchanged: wr.unchanged,
                deleted: wr.deleted,
                skipped_unsafe: wr.skipped_unsafe,
            });
            // "executed" = did any writes/deletes, so a no-op
            // incremental build reports all executed=false.
            sink(BuildEvent::StageFinished {
                stage: "write_outputs".to_string(),
                group: "output".to_string(),
                executed: wr.written > 0 || wr.deleted > 0,
                ms: wt0.elapsed().as_secs_f64() * 1000.0,
            });
            sink(BuildEvent::Progress {
                percent: 100.0,
                stage: "write_outputs".to_string(),
                chunks_done,
                chunks_total,
            });
            sink(BuildEvent::Finished {
                status: "ok".to_string(),
                error: None,
                ms: t0.elapsed().as_secs_f64() * 1000.0,
            });
            BuildOutcome {
                report: Some(rep),
                status: "ok",
                error: None,
            }
        }
        Err(BuildError::Locked) => {
            sink(BuildEvent::Finished {
                status: "failed".to_string(),
                error: Some("another build is running on this project".to_string()),
                ms: t0.elapsed().as_secs_f64() * 1000.0,
            });
            BuildOutcome {
                report: None,
                status: "failed",
                error: Some("another build is running on this project".to_string()),
            }
        }
        Err(e) => {
            sink(BuildEvent::Finished {
                status: "failed".to_string(),
                error: Some(e.to_string()),
                ms: t0.elapsed().as_secs_f64() * 1000.0,
            });
            BuildOutcome {
                report: None,
                status: "failed",
                error: Some(e.to_string()),
            }
        }
    }
}
