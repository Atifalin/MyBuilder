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
    Build {
        dir: PathBuf,
        /// Wipe cache + listed outputs first (requires --yes).
        #[arg(long)]
        clean: bool,
        /// Confirm --clean.
        #[arg(long)]
        yes: bool,
        /// Override the cache location (default: OS cache dir).
        #[arg(long)]
        cache_dir: Option<PathBuf>,
        /// Recompute everything (do not read the cache).
        #[arg(long)]
        full: bool,
        /// Emit NDJSON BuildEvents on stdout; human text -> stderr.
        #[arg(long)]
        json_events: bool,
    },
    /// Wipe generated state (cache + outputs) and reseed.
    Clear {
        dir: PathBuf,
        /// New seed in hex (random if omitted).
        #[arg(long)]
        seed: Option<String>,
        /// Also delete overrides/ files (kept by default).
        #[arg(long)]
        drop_overrides: bool,
        /// Confirm the destructive operation.
        #[arg(long)]
        yes: bool,
    },
    /// Print the resolved cache directory for a project.
    CachePath { dir: PathBuf },
    /// Set generator_version to the current one.
    Upgrade { dir: PathBuf },
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

fn cmd_cache_path(dir: &Path) -> Result<ExitCode, String> {
    let p = mb_terrain::build::project_cache_dir(dir).map_err(|e| e.to_string())?;
    println!("{}", p.display());
    Ok(ExitCode::SUCCESS)
}

fn cmd_clear(dir: &Path, seed: Option<String>, drop: bool, yes: bool) -> Result<ExitCode, String> {
    if !yes {
        return Err("clear deletes the cache and all listed outputs; pass --yes".to_string());
    }
    let seed = match seed {
        Some(s) => Some(parse_seed(&s)?),
        None => None,
    };
    let rep = mb_bridge::clear_all(
        dir,
        &mb_bridge::ClearRequest {
            new_seed: seed,
            keep_overrides: !drop,
            cache_dir: None,
        },
    )
    .map_err(|e| e.to_string())?;
    println!(
        "cleared {} files; seed is now {:016x}",
        rep.deleted_files, rep.seed
    );
    println!("run `mybuilder build --full` to regenerate");
    Ok(ExitCode::SUCCESS)
}

fn cmd_build(
    dir: &Path,
    clean: bool,
    yes: bool,
    cache_dir: Option<PathBuf>,
    full: bool,
    json_events: bool,
) -> Result<ExitCode, String> {
    if clean && !yes {
        return Err(
            "--clean deletes cache/ and all previously listed outputs; pass --yes to confirm"
                .to_string(),
        );
    }
    let t0 = std::time::Instant::now();
    // Env override is read here only — libraries never read env vars.
    let cache_dir = cache_dir.or_else(|| std::env::var_os("MB_CACHE_DIR").map(PathBuf::from));
    if json_events {
        let cancel = mb_bridge::CancelToken::new();
        // stdin reader: the line "cancel" cancels; EOF does nothing.
        {
            let c = cancel.clone();
            std::thread::spawn(move || {
                use std::io::BufRead;
                let stdin = std::io::stdin();
                for line in stdin.lock().lines().map_while(Result::ok) {
                    if line.trim() == "cancel" {
                        c.cancel();
                    }
                }
            });
        }
        let stdout = std::io::stdout();
        let mut lock = stdout.lock();
        use std::io::Write as _;
        let mut sink = |e: mb_bridge::BuildEvent| {
            let _ = writeln!(lock, "{}", serde_json::to_string(&e).unwrap());
        };
        let out = mb_bridge::build(
            dir,
            &mb_bridge::BuildRequest {
                mode: if full {
                    mb_bridge::BuildMode::Full
                } else {
                    mb_bridge::BuildMode::Incremental
                },
                cache_dir,
            },
            &cancel,
            &mut sink,
        );
        return Ok(match out.status {
            "ok" => ExitCode::SUCCESS,
            "cancelled" => ExitCode::from(2),
            _ => ExitCode::FAILURE,
        });
    }
    let rep = mb_terrain::build::build_project(
        dir,
        &mb_terrain::build::BuildOptions {
            clean,
            full,
            cache_dir,
        },
        &mb_pipeline::CancelToken::new(),
    )
    .map_err(|e| e.to_string())?;
    for d in &rep.diagnostics {
        println!("{} {} {}: {}", d.severity, d.code, d.path, d.message);
    }
    // Grouped stage report.
    let (mut g_run, mut c_run, mut r_run) = (0, 0, 0);
    let (mut g_re, mut c_re, mut r_re) = (0, 0, 0);
    let (mut g_us, mut c_us, mut r_us) = (0u64, 0u64, 0u64);
    let group = |id: &str| {
        if id.starts_with("world.chunk.") {
            1
        } else if id.starts_with("world.region.") {
            2
        } else {
            0
        }
    };
    for id in &rep.run.executed {
        match group(id) {
            1 => {
                c_run += 1;
                c_us += rep.run.durations_us.get(id).copied().unwrap_or(0);
            }
            2 => {
                r_run += 1;
                r_us += rep.run.durations_us.get(id).copied().unwrap_or(0);
            }
            _ => {
                g_run += 1;
                g_us += rep.run.durations_us.get(id).copied().unwrap_or(0);
            }
        }
    }
    for id in &rep.run.reused {
        match group(id) {
            1 => c_re += 1,
            2 => r_re += 1,
            _ => g_re += 1,
        }
    }
    let n_chunks_total = c_run + c_re;
    let n_reg_total = r_run + r_re;
    println!(
        "global stages: {g_run} ran, {g_re} reused ({:.3} s)",
        g_us as f64 / 1e6
    );
    println!(
        "chunks: {c_run} ran of {n_chunks_total} ({c_re} cached, {:.3} s)",
        c_us as f64 / 1e6
    );
    if n_reg_total > 0 {
        println!(
            "region chunks: {r_run} ran of {n_reg_total} ({r_re} cached, {:.3} s)",
            r_us as f64 / 1e6
        );
    }
    let cs = &rep.run.cache;
    println!(
        "cache: {} hits, {} misses, {} corrupt, {} missing, {} blobs written, {} deleted",
        cs.hits, cs.misses, cs.corrupt, cs.missing, cs.blobs_written, cs.blobs_deleted
    );
    println!("files written: {}", rep.write.written);
    println!("files unchanged: {}", rep.write.unchanged);
    if rep.write.deleted > 0 {
        println!("files deleted: {}", rep.write.deleted);
    }
    if rep.write.skipped_unsafe > 0 {
        println!(
            "warning: {} manifest/output paths failed the confinement check and were skipped",
            rep.write.skipped_unsafe
        );
    }
    // h range + totals from the manifest.
    if let Some((_, data)) = rep
        .outputs
        .files
        .iter()
        .find(|(p, _)| p == mb_terrain::outputs::MANIFEST_PATH)
        && let Ok(m) = serde_json::from_slice::<serde_json::Value>(data)
    {
        let t = &m["totals"];
        println!("h_min {:.3} m, h_max {:.3} m", m["h_min"], m["h_max"]);
        println!(
            "{} chunks, {} vertices, {} triangles",
            t["chunks"], t["vertices"], t["triangles"]
        );
    }
    println!("total wall time: {:.3} s", t0.elapsed().as_secs_f64());
    Ok(ExitCode::SUCCESS)
}

fn cmd_upgrade(dir: &Path) -> Result<ExitCode, String> {
    let path = dir.join(mb_schema::PROJECT_FILE);
    let mut project: mb_schema::ProjectFile =
        serde_json::from_value(mb_schema::load_json_file(&path).map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
    let old = project.generator_version;
    if old > mb_schema::CURRENT_GENERATOR_VERSION {
        return Err("project needs a newer My Builder".to_string());
    }
    if old < mb_schema::MIN_SUPPORTED_GENERATOR_VERSION {
        return Err(format!("generator {old} is too old to upgrade"));
    }
    project.generator_version = mb_schema::CURRENT_GENERATOR_VERSION;
    let mut text = serde_json::to_string_pretty(&project).map_err(|e| e.to_string())?;
    text.push('\n');
    std::fs::write(&path, text).map_err(|e| e.to_string())?;
    println!(
        "generator_version {old} -> {}",
        mb_schema::CURRENT_GENERATOR_VERSION
    );
    println!("the next `mybuilder build` regenerates outputs; exported files are untouched now");
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
        Command::Build {
            dir,
            clean,
            yes,
            cache_dir,
            full,
            json_events,
        } => cmd_build(&dir, clean, yes, cache_dir, full, json_events),
        Command::Clear {
            dir,
            seed,
            drop_overrides,
            yes,
        } => cmd_clear(&dir, seed, drop_overrides, yes),
        Command::CachePath { dir } => cmd_cache_path(&dir),
        Command::Upgrade { dir } => cmd_upgrade(&dir),
    };
    match result {
        Ok(code) => code,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}
