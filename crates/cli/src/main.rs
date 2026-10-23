//! `mybuilder` — command-line front end for the My Builder core crates.

use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::{SystemTime, UNIX_EPOCH};

use clap::{Parser, Subcommand};

use mb_schema::Severity;

#[derive(Parser)]
#[command(name = "mybuilder", about = "Deterministic procedural world builder")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Create a new project folder.
    New {
        /// Directory to create the project in.
        dir: PathBuf,
        /// Project name.
        #[arg(long)]
        name: Option<String>,
        /// Project seed as a hex string (up to 16 hex digits). If omitted,
        /// one is generated from the system clock — the single allowed
        /// nondeterministic point in the toolchain.
        #[arg(long)]
        seed: Option<String>,
    },
    /// Validate a project folder and print diagnostics.
    Validate {
        dir: PathBuf,
        /// Emit diagnostics as JSON instead of text.
        #[arg(long)]
        json: bool,
    },
    /// Print the canonical BLAKE3 hashes of project and world recipe.
    Hash { dir: PathBuf },
    /// Print resolved terrain parameters and the chunk grid.
    Info { dir: PathBuf },
    /// Print the version and determinism probe hash.
    Probe,
    /// Validate, run the terrain pipeline and write outputs.
    Build { dir: PathBuf },
}

fn gen_seed() -> u64 {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let h = mb_seed::hash_bytes(&nanos.to_le_bytes());
    u64::from_le_bytes(h[..8].try_into().unwrap())
}

fn parse_seed(s: &str) -> Result<u64, String> {
    u64::from_str_radix(s.trim(), 16)
        .map_err(|e| format!("invalid --seed {s:?}: expected hex, {e}"))
}

fn cmd_new(dir: &Path, name: Option<String>, seed: Option<String>) -> Result<(), String> {
    let seed = match seed {
        Some(s) => parse_seed(&s)?,
        None => gen_seed(),
    };
    let project = mb_schema::ProjectFile {
        seed,
        name: name.unwrap_or_else(|| "My World".to_string()),
        ..mb_schema::ProjectFile::default()
    };
    let world = mb_schema::WorldRecipe::default();
    mb_schema::create_project(dir, &project, &world).map_err(|e| e.to_string())?;
    println!("created project {:?} at {}", project.name, dir.display());
    println!("seed: {seed:016x}");
    Ok(())
}

fn print_diags(diags: &[mb_schema::Diagnostic]) {
    for d in diags {
        match &d.suggestion {
            Some(hint) => println!(
                "{} {} {}: {} (hint: {hint})",
                d.severity, d.code, d.path, d.message
            ),
            None => println!("{} {} {}: {}", d.severity, d.code, d.path, d.message),
        }
    }
}

fn has_errors(diags: &[mb_schema::Diagnostic]) -> bool {
    diags.iter().any(|d| d.severity == Severity::Error)
}

fn cmd_validate(dir: &Path, json: bool) -> Result<ExitCode, String> {
    let (_p, _w, diags) = mb_schema::load_project(dir).map_err(|e| e.to_string())?;
    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&diags).map_err(|e| e.to_string())?
        );
    } else {
        print_diags(&diags);
        if diags.is_empty() {
            println!("ok: no diagnostics");
        }
    }
    Ok(if has_errors(&diags) {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    })
}

fn cmd_hash(dir: &Path) -> Result<(), String> {
    let (project, world, _diags) = mb_schema::load_project(dir).map_err(|e| e.to_string())?;
    let ph = mb_seed::hex_bytes(&mb_schema::canonical_hash(&project));
    let wh = mb_seed::hex_bytes(&mb_schema::canonical_hash(&world));
    println!("project {ph}");
    println!("world   {wh}");
    Ok(())
}

fn cmd_info(dir: &Path) -> Result<ExitCode, String> {
    let (project, world, diags) = mb_schema::load_project(dir).map_err(|e| e.to_string())?;
    if has_errors(&diags) {
        print_diags(&diags);
        return Ok(ExitCode::FAILURE);
    }
    let params = world.terrain.feel.resolve(&world.terrain.overrides);
    let cell = project.world_scale.cell_size_m;
    let chunks_x = world.extent.width_m / world.chunk_size_m;
    let chunks_z = world.extent.depth_m / world.chunk_size_m;
    let verts = world.chunk_size_m / cell + 1.0;
    println!("name: {}", project.name);
    println!("seed: {:016x}", project.seed);
    println!(
        "extent: {} x {} m",
        world.extent.width_m, world.extent.depth_m
    );
    println!(
        "terrain: feel={:?} octaves={} gain={} lacunarity={} relief_m={} ridged_mix={}",
        world.terrain.feel,
        params.octaves,
        params.gain,
        params.lacunarity,
        params.relief_m,
        params.ridged_mix
    );
    println!(
        "chunk grid: {chunks_x} x {chunks_z} chunks of {} m",
        world.chunk_size_m
    );
    println!("vertices per chunk side: {verts}");
    Ok(ExitCode::SUCCESS)
}

fn cmd_build(dir: &Path) -> Result<ExitCode, String> {
    let (project, world, diags) = mb_schema::load_project(dir).map_err(|e| e.to_string())?;
    print_diags(&diags);
    if has_errors(&diags) {
        return Ok(ExitCode::FAILURE);
    }
    let cell = project.world_scale.cell_size_m;
    let grid = mb_terrain::Grid::new(world.extent.width_m, world.extent.depth_m, cell);
    let mut pipe = mb_pipeline::Pipeline::new(project.seed);
    pipe.add(Box::new(mb_terrain::stages::MasksStage {
        code_version: 1,
        grid,
        mode: world.mode,
        shapes: world.shapes.clone(),
        cell_size_m: cell,
        extent: world.extent,
    }));
    pipe.add(Box::new(mb_terrain::stages::DistanceStage::new(
        grid, cell, 1,
    )));
    pipe.add(Box::new(mb_terrain::stages::HeightStage::new(
        grid,
        project.clone(),
        world.clone(),
        1,
    )));
    pipe.add(Box::new(mb_terrain::stages::ExportStage::new(
        grid, project, world, 1,
    )));
    let mut cache = mb_pipeline::Cache::default();
    let report = pipe.run(&mut cache).map_err(|e| e.to_string())?;
    for id in &report.executed {
        let us = report.durations_us.get(id).copied().unwrap_or(0);
        println!("stage {id}: ran in {:.3} s", us as f64 / 1e6);
    }
    for id in &report.reused {
        println!("stage {id}: reused");
    }
    // The export artifact is the whole output bundle.
    let bundle_bytes = cache
        .get(mb_terrain::stages::ExportStage::ID)
        .ok_or("export artifact missing")?
        .bytes
        .clone();
    let bundle = mb_terrain::outputs::FileBundle::decode(&bundle_bytes);
    let mut h_min = f64::NAN;
    let mut h_max = f64::NAN;
    let mut totals = String::new();
    for (rel, data) in &bundle.files {
        if rel == mb_terrain::outputs::MANIFEST_PATH {
            let m: serde_json::Value = serde_json::from_slice(data).map_err(|e| e.to_string())?;
            h_min = m["h_min"].as_f64().unwrap_or(f64::NAN);
            h_max = m["h_max"].as_f64().unwrap_or(f64::NAN);
            let t = &m["totals"];
            totals = format!(
                "{} chunks, {} vertices, {} triangles",
                t["chunks"], t["vertices"], t["triangles"]
            );
        }
    }
    let wr = mb_terrain::outputs::write_outputs(dir, &bundle).map_err(|e| e.to_string())?;
    println!("files written: {}", wr.written.len());
    for f in &wr.written {
        println!("  wrote {f}");
    }
    println!("files unchanged: {}", wr.unchanged.len());
    println!("h_min {h_min:.3} m, h_max {h_max:.3} m");
    println!("{totals}");
    Ok(ExitCode::SUCCESS)
}

fn cmd_probe() -> ExitCode {
    println!("version {}", mb_bridge::version());
    println!("probe {}", mb_bridge::probe());
    ExitCode::SUCCESS
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    let result = match cli.command {
        Command::New { dir, name, seed } => cmd_new(&dir, name, seed).map(|_| ExitCode::SUCCESS),
        Command::Validate { dir, json } => cmd_validate(&dir, json),
        Command::Hash { dir } => cmd_hash(&dir).map(|_| ExitCode::SUCCESS),
        Command::Info { dir } => cmd_info(&dir),
        Command::Probe => Ok(cmd_probe()),
        Command::Build { dir } => cmd_build(&dir),
    };
    match result {
        Ok(code) => code,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}
