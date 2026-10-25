//! Pipeline stages for world terrain.
//!
//! Artifact binary layouts (all little-endian, row-major z):
//! - `world.masks`: u64 nx, u64 nz, then nx*nz u8 planes land, mountain,
//!   forest in that order.
//! - `world.distance`: u64 nx, u64 nz, then two nx*nz f64 planes: signed
//!   coast distance and inside-mountain distance (meters).
//! - `world.height`: u64 nx, u64 nz, f64 h_min, f64 h_max, then nx*nz
//!   f64 heights, then nx*nz f64 coast distances (the export stage needs
//!   d for vertex colors but does not depend on world.distance per the
//!   stage DAG).
//! - `world.export`: a [`outputs::FileBundle`] with every output file.
//!
//! Stage params are canonical JSON of the recipe slice the stage reads
//! plus the cell size, so a recipe edit invalidates exactly what depends
//! on it.

use mb_pipeline::{Artifact, Cancelled, RunCtx, Stage};
use mb_schema::{ProjectFile, WorldRecipe};
use serde::Serialize;

use crate::outputs::{self, FileBundle};
use crate::{Grid, HeightField, HeightInputs, Masks, build_height};

fn put_u64(b: &mut Vec<u8>, v: u64) {
    b.extend_from_slice(&v.to_le_bytes());
}
fn put_f64(b: &mut Vec<u8>, v: f64) {
    b.extend_from_slice(&v.to_le_bytes());
}
fn get_u64(b: &[u8], o: &mut usize) -> u64 {
    let v = u64::from_le_bytes(b[*o..*o + 8].try_into().unwrap());
    *o += 8;
    v
}
fn get_f64s(b: &[u8], o: &mut usize, n: usize) -> Vec<f64> {
    let mut out = Vec::with_capacity(n);
    for _ in 0..n {
        out.push(f64::from_le_bytes(b[*o..*o + 8].try_into().unwrap()));
        *o += 8;
    }
    out
}

fn encode_masks(grid: &Grid, m: &Masks) -> Artifact {
    let mut b = Vec::with_capacity(16 + 3 * grid.len());
    put_u64(&mut b, grid.nx as u64);
    put_u64(&mut b, grid.nz as u64);
    b.extend_from_slice(&m.land);
    b.extend_from_slice(&m.mountain);
    b.extend_from_slice(&m.forest);
    Artifact::new(b)
}

fn decode_masks(b: &[u8]) -> (Grid, Masks) {
    let mut o = 0;
    let nx = get_u64(b, &mut o) as usize;
    let nz = get_u64(b, &mut o) as usize;
    let n = nx * nz;
    let land = b[o..o + n].to_vec();
    let mountain = b[o + n..o + 2 * n].to_vec();
    let forest = b[o + 2 * n..o + 3 * n].to_vec();
    // Grid dims decode without world-space origin; callers that need
    // coordinates pass their own Grid. Only masks rasterization needs
    // geometry upstream.
    (
        Grid {
            nx,
            nz,
            cell: 1.0,
            origin_x: 0.0,
            origin_z: 0.0,
        },
        Masks {
            land,
            mountain,
            forest,
        },
    )
}

fn encode_distance(grid: &Grid, d: &crate::Distances) -> Artifact {
    let mut b = Vec::with_capacity(16 + 16 * grid.len());
    put_u64(&mut b, grid.nx as u64);
    put_u64(&mut b, grid.nz as u64);
    for &v in &d.coast {
        put_f64(&mut b, v);
    }
    for &v in &d.mountain {
        put_f64(&mut b, v);
    }
    Artifact::new(b)
}

fn decode_distance(b: &[u8]) -> (usize, usize, Vec<f64>, Vec<f64>) {
    let mut o = 0;
    let nx = get_u64(b, &mut o) as usize;
    let nz = get_u64(b, &mut o) as usize;
    let coast = get_f64s(b, &mut o, nx * nz);
    let mountain = get_f64s(b, &mut o, nx * nz);
    (nx, nz, coast, mountain)
}

fn encode_height(grid: &Grid, hf: &HeightField, coast: &[f64]) -> Artifact {
    let mut b = Vec::with_capacity(32 + 16 * grid.len());
    put_u64(&mut b, grid.nx as u64);
    put_u64(&mut b, grid.nz as u64);
    put_f64(&mut b, hf.h_min);
    put_f64(&mut b, hf.h_max);
    for &v in &hf.h {
        put_f64(&mut b, v);
    }
    for &v in coast {
        put_f64(&mut b, v);
    }
    Artifact::new(b)
}

/// Returns (nx, nz, h_min, h_max, heights, coast).
fn decode_height(b: &[u8]) -> (usize, usize, f64, f64, Vec<f64>, Vec<f64>) {
    let mut o = 0;
    let nx = get_u64(b, &mut o) as usize;
    let nz = get_u64(b, &mut o) as usize;
    let h_min = f64::from_le_bytes(b[o..o + 8].try_into().unwrap());
    o += 8;
    let h_max = f64::from_le_bytes(b[o..o + 8].try_into().unwrap());
    o += 8;
    let h = get_f64s(b, &mut o, nx * nz);
    let coast = get_f64s(b, &mut o, nx * nz);
    (nx, nz, h_min, h_max, h, coast)
}

/// Shared per-stage parameter payload: canonical JSON bytes.
fn params<T: Serialize>(v: &T) -> Vec<u8> {
    serde_json::to_vec(v).expect("stage params serialize")
}

/// Height input for gen >= 3 is the hydrology artifact (same layout,
/// final_h in place of h); earlier gens use world.height.
pub fn height_dep(generator_version: u32) -> String {
    if generator_version >= 3 {
        HydrologyStage::ID.to_string()
    } else {
        HeightStage::ID.to_string()
    }
}

// ---------------------------------------------------------------------

pub struct MasksStage {
    pub code_version: u32,
    pub grid: Grid,
    pub mode: mb_schema::WorldMode,
    pub shapes: Vec<mb_schema::Shape>,
    pub cell_size_m: f64,
    pub extent: mb_schema::Extent,
    pub generator_version: u32,
}

impl MasksStage {
    pub const ID: &'static str = "world.masks";
}

impl Stage for MasksStage {
    fn id(&self) -> &str {
        Self::ID
    }
    fn deps(&self) -> &[String] {
        &[]
    }
    fn code_version(&self) -> u32 {
        self.code_version
    }
    fn params(&self) -> Vec<u8> {
        params(&serde_json::json!({
            "extent": self.extent,
            "cell_size_m": self.cell_size_m,
            "mode": self.mode,
            "shapes": self.shapes,
            "generator_version": self.generator_version,
        }))
    }
    fn run(&self, _inputs: &[&Artifact], _seed: u64, ctx: &RunCtx) -> Result<Artifact, Cancelled> {
        Ok({
            encode_masks(
                &self.grid,
                &crate::build_masks_poll(&self.grid, &self.shapes, self.mode, &|| ctx.tick()),
            )
        })
    }
}

pub struct DistanceStage {
    pub code_version: u32,
    pub grid: Grid,
    pub cell_size_m: f64,
    deps: Vec<String>,
}

impl DistanceStage {
    pub const ID: &'static str = "world.distance";
    pub fn new(grid: Grid, cell_size_m: f64, code_version: u32) -> Self {
        DistanceStage {
            code_version,
            grid,
            cell_size_m,
            deps: vec![MasksStage::ID.to_string()],
        }
    }
}

impl Stage for DistanceStage {
    fn id(&self) -> &str {
        Self::ID
    }
    fn deps(&self) -> &[String] {
        &self.deps
    }
    fn code_version(&self) -> u32 {
        self.code_version
    }
    fn params(&self) -> Vec<u8> {
        params(&serde_json::json!({ "cell_size_m": self.cell_size_m }))
    }
    fn run(&self, inputs: &[&Artifact], _seed: u64, ctx: &RunCtx) -> Result<Artifact, Cancelled> {
        Ok({
            let (_g, masks) = decode_masks(&inputs[0].bytes);
            encode_distance(
                &self.grid,
                &crate::build_distances_poll(&self.grid, &masks, &|| ctx.tick()),
            )
        })
    }
}

pub struct HeightStage {
    pub code_version: u32,
    pub grid: Grid,
    pub project: ProjectFile,
    pub world: WorldRecipe,
    deps: Vec<String>,
}

impl HeightStage {
    pub const ID: &'static str = "world.height";
    pub fn new(grid: Grid, project: ProjectFile, world: WorldRecipe, code_version: u32) -> Self {
        HeightStage {
            code_version,
            grid,
            project,
            world,
            deps: vec![DistanceStage::ID.to_string(), MasksStage::ID.to_string()],
        }
    }
}

impl Stage for HeightStage {
    fn id(&self) -> &str {
        Self::ID
    }
    fn deps(&self) -> &[String] {
        &self.deps
    }
    fn code_version(&self) -> u32 {
        self.code_version
    }
    fn params(&self) -> Vec<u8> {
        params(&serde_json::json!({
            "cell_size_m": self.project.world_scale.cell_size_m,
            "sea_level_m": self.world.sea_level_m,
            "terrain": self.world.terrain,
            "profile": self.world.profile,
            "seed": format!("{:016x}", self.project.seed),
            "generator_version": self.project.generator_version,
        }))
    }
    fn run(&self, inputs: &[&Artifact], _seed: u64, _ctx: &RunCtx) -> Result<Artifact, Cancelled> {
        Ok({
            let (_nx, _nz, coast, mountain) = decode_distance(&inputs[0].bytes);
            let (_g, masks) = decode_masks(&inputs[1].bytes);
            let dist = crate::Distances { coast, mountain };
            let terrain = self
                .world
                .terrain
                .feel
                .resolve(&self.world.terrain.overrides);
            let coast_for_artifact = dist.coast.clone();
            let hf = build_height(&HeightInputs {
                grid: &self.grid,
                masks: &masks,
                dist: &dist,
                sea_level_m: self.world.sea_level_m,
                project_seed: self.project.seed,
                terrain,
                profile: &self.world.profile,
            });
            encode_height(&self.grid, &hf, &coast_for_artifact)
        })
    }
}

pub struct ExportStage {
    pub code_version: u32,
    pub grid: Grid,
    pub project: ProjectFile,
    pub world: WorldRecipe,
    deps: Vec<String>,
}

impl ExportStage {
    pub const ID: &'static str = "world.export";
    pub fn new(grid: Grid, project: ProjectFile, world: WorldRecipe, code_version: u32) -> Self {
        let deps = vec![HeightStage::ID.to_string(), MasksStage::ID.to_string()]
            .into_iter()
            .chain(world.detail_regions.iter().map(|r| region_stage_id(&r.id)))
            .collect();
        ExportStage {
            code_version,
            grid,
            project,
            world,
            deps,
        }
    }
}

/// Stage id of the per-region stage.
pub fn region_stage_id(id: &str) -> String {
    format!("world.region.{id}")
}

/// One stage per detail region: emits the region's fine chunk GLBs as a
/// FileBundle artifact. Depends on height (for h and coast) and distance
/// (mountain distances feed height_at via the coarse planes — but the
/// height artifact already carries coast; mountain is needed too).
pub struct RegionStage {
    pub code_version: u32,
    pub grid: Grid,
    pub project: ProjectFile,
    pub world: WorldRecipe,
    pub region: mb_schema::DetailRegion,
    stage_id: String,
    deps: Vec<String>,
}

impl RegionStage {
    pub fn new(
        grid: Grid,
        project: ProjectFile,
        world: WorldRecipe,
        region: mb_schema::DetailRegion,
        code_version: u32,
    ) -> Self {
        let height_dep_h = height_dep(project.generator_version);
        let stage_id = region_stage_id(&region.id);
        RegionStage {
            code_version,
            grid,
            project,
            world,
            region,
            stage_id,
            deps: vec![
                height_dep_h,
                DistanceStage::ID.to_string(),
                MasksStage::ID.to_string(),
            ],
        }
    }
}

impl Stage for RegionStage {
    fn id(&self) -> &str {
        // Boxed id storage: keep a leaked string? No — deps use a stored
        // String, but id() needs &str tied to self. Store the id string.
        // We store it in `region.id`-derived owned field added below.
        &self.stage_id
    }
    fn deps(&self) -> &[String] {
        &self.deps
    }
    fn code_version(&self) -> u32 {
        self.code_version
    }
    fn params(&self) -> Vec<u8> {
        params(&serde_json::json!({
            "region": self.region,
            "cell_size_m": self.project.world_scale.cell_size_m,
            "chunk_size_m": self.world.chunk_size_m,
            "mesh_step_cells": self.world.export.mesh_step_cells,
            "generator_version": self.project.generator_version,
            "seed": format!("{:016x}", self.project.seed),
        }))
    }
    fn run(&self, inputs: &[&Artifact], _seed: u64, _ctx: &RunCtx) -> Result<Artifact, Cancelled> {
        Ok({
            let (_nx, _nz, h_min, h_max, heights, coast) = decode_height(&inputs[0].bytes);
            let (_dnx, _dnz, dcoast, mountain) = decode_distance(&inputs[1].bytes);
            let (_g, masks) = decode_masks(&inputs[2].bytes);
            let hf = HeightField {
                h: heights,
                h_min,
                h_max,
            };
            let dist = crate::Distances {
                coast: dcoast,
                mountain,
            };
            let terrain = self
                .world
                .terrain
                .feel
                .resolve(&self.world.terrain.overrides);
            let ctx = crate::HeightCtx::new(
                self.grid,
                &dist,
                self.world.sea_level_m,
                self.project.seed,
                terrain,
                self.world.profile,
            );
            let _ = (masks, coast);
            let cells_per_chunk = (self.world.chunk_size_m / self.grid.cell) as usize;
            let coarse_step = self.world.export.mesh_step_cells as usize;
            let rb = crate::region::build_region_height(
                &ctx,
                &hf,
                &self.region,
                cells_per_chunk,
                coarse_step,
            );
            let normals_full = crate::mesh::normals(&self.grid, &hf);
            let rnorms = crate::region::region_normals(&ctx, &normals_full, &rb, coarse_step);
            let mut files = Vec::new();
            let mut entries = Vec::new();
            for cz in self.region.chunk_min[1]..=self.region.chunk_max[1] {
                for cx in self.region.chunk_min[0]..=self.region.chunk_max[0] {
                    let (cx, cz) = (cx as usize, cz as usize);
                    let tex2: Option<(&mb_schema::TexturingSection, usize)> = None;
                    let mesh = crate::region::region_chunk_mesh(
                        &ctx,
                        &self.region,
                        cells_per_chunk,
                        &rb,
                        &rnorms,
                        self.world.sea_level_m,
                        self.world.profile.snow_line_m,
                        cx,
                        cz,
                        tex2,
                    );
                    let key = format!("{cx}_{cz}");
                    let id = mb_seed::EntityId::from_path(
                        self.project.seed,
                        &[
                            ("world", "main"),
                            ("region", &self.region.id),
                            ("chunk", &key),
                        ],
                    );
                    let name = format!("chunk_{cx}_{cz}");
                    let glb = crate::glb::chunk_glb(&mesh, &name, &id.to_string(), cx, cz);
                    let rel = format!("exports/terrain/regions/{}/{}.glb", self.region.id, name);
                    let e = serde_json::json!({
                        "id": id.to_string(),
                        "cx": cx,
                        "cz": cz,
                        "region": self.region.id,
                        "file": format!("regions/{}/{}.glb", self.region.id, name),
                        "origin": mesh.origin,
                        "vertex_count": mesh.vertex_count,
                        "triangle_count": mesh.triangle_count,
                        "blake3": mb_seed::hex_bytes(&mb_seed::hash_bytes(&glb)),
                    });
                    entries.push(e);
                    files.push((rel, glb));
                }
            }
            // The bundle also carries the manifest entry list as a JSON
            // pseudo-file the export stage consumes (never written to disk).
            files.push((
                format!("__manifest_entries__/{}", self.region.id),
                serde_json::to_vec(&entries).expect("entries"),
            ));
            FileBundle { files }.encode()
        })
    }
}

impl Stage for ExportStage {
    fn id(&self) -> &str {
        Self::ID
    }
    fn deps(&self) -> &[String] {
        &self.deps
    }
    fn code_version(&self) -> u32 {
        self.code_version
    }
    fn params(&self) -> Vec<u8> {
        params(&serde_json::json!({
            "cell_size_m": self.project.world_scale.cell_size_m,
            "chunk_size_m": self.world.chunk_size_m,
            "export": self.world.export,
            "sea_level_m": self.world.sea_level_m,
            "seed": format!("{:016x}", self.project.seed),
            "generator_version": self.project.generator_version,
        }))
    }
    fn run(&self, inputs: &[&Artifact], _seed: u64, _ctx: &RunCtx) -> Result<Artifact, Cancelled> {
        Ok({
            let (_nx, _nz, h_min, h_max, heights, coast) = decode_height(&inputs[0].bytes);
            let (_g, masks) = decode_masks(&inputs[1].bytes);
            let hf = HeightField {
                h: heights,
                h_min,
                h_max,
            };
            // inputs[2..] are region bundles in detail_regions order.
            let mut region_outputs = Vec::new();
            for (i, region) in self.world.detail_regions.iter().enumerate() {
                let rb = FileBundle::decode(&inputs[2 + i].bytes);
                let marker = format!("__manifest_entries__/{}", region.id);
                let mut entries = serde_json::Value::Null;
                let mut files = Vec::new();
                for (rel, data) in rb.files {
                    if rel == marker {
                        entries = serde_json::from_slice(&data).expect("region entries");
                    } else {
                        files.push((rel, data));
                    }
                }
                region_outputs.push(outputs::RegionOutput {
                    region: region.clone(),
                    entries,
                    files,
                });
            }
            let bundle = outputs::build_export_bundle(
                &self.grid,
                &self.project,
                &self.world,
                &hf,
                &masks,
                &coast,
                &region_outputs,
            );
            FileBundle::encode(&bundle)
        })
    }
}

// ---------------------------------------------------------------------
// S10: layers + per-chunk sliced stages + manifest stage.
// Chunk artifact = 8-byte header (vertex_count u32, triangle_count u32,
// LE) followed by the GLB file bytes.

pub const CHUNK_HEADER: usize = 8;

// world.layers: the raster layer bundle (r16 x2, masks, layers.json,
// hillshade preview). Normal stage, deps height + masks.

// ---------------------------------------------------------------------
/// world.hydrology (gen >= 3): priority-flood + D8 + lakes + rivers +
/// carving on the coarse node grid.
///
/// Artifact layout (all LE, f64 unless noted):
///   nx u64, nz u64, cell f64,
///   final_h, coast, delta, water (f64::NEG_INFINITY = no water), acc,
///   water_dist, water_surface, each [nx*nz] f64;
///   river[nx*nz] u8, lake[nx*nz] u8,
///   lakes: u64 count, then (id u32, level f64, area f64) each.
pub struct HydrologyStage {
    pub code_version: u32,
    pub grid: Grid,
    pub project: ProjectFile,
    pub world: WorldRecipe,
    deps: Vec<String>,
}
impl HydrologyStage {
    pub const ID: &'static str = "world.hydrology";
    pub fn new(code_version: u32, grid: Grid, project: ProjectFile, world: WorldRecipe) -> Self {
        Self {
            code_version,
            grid,
            project,
            world,
            deps: vec![HeightStage::ID.to_string(), MasksStage::ID.to_string()],
        }
    }
}

/// Decode the hydrology artifact; see HydrologyStage for the layout.
pub struct HydroPlanes {
    pub final_h: Vec<f64>,
    pub delta: Vec<f64>,
    pub water: Vec<f64>,
    pub river: Vec<u8>,
    pub lake: Vec<u8>,
    pub acc: Vec<f64>,
    pub water_dist: Vec<f64>,
    pub water_surface: Vec<f64>,
    pub lakes: Vec<(u32, f64, f64)>,
    /// River flow velocity (x,z pairs, 0 for non-river nodes).
    pub flow: Vec<f64>,
    /// Kind of the nearest wet node (1 river, 2 lake, 0 none).
    pub surface_kind: Vec<u8>,
    /// Flood-fill below-level lake coverage mask.
    pub lake_cover: Vec<u8>,
    /// Distance beyond the nearest river's half-width (m).
    pub rsand: Vec<f64>,
}
pub fn decode_hydro(b: &[u8]) -> (usize, usize, f64, HydroPlanes) {
    let mut o = 0;
    let nx = get_u64(b, &mut o) as usize;
    let nz = get_u64(b, &mut o) as usize;
    let h_min = f64::from_le_bytes(b[o..o + 8].try_into().unwrap());
    o += 8;
    let h_max = f64::from_le_bytes(b[o..o + 8].try_into().unwrap());
    o += 8;
    let cell = (h_min, h_max); // packed min/max, decoded by caller
    let n = nx * nz;
    let final_h = get_f64s(b, &mut o, n);
    let _coast = get_f64s(b, &mut o, n);
    let delta = get_f64s(b, &mut o, n);
    let water = get_f64s(b, &mut o, n);
    let acc = get_f64s(b, &mut o, n);
    let water_dist = get_f64s(b, &mut o, n);
    let water_surface = get_f64s(b, &mut o, n);
    let river = b[o..o + n].to_vec();
    o += n;
    let lake = b[o..o + n].to_vec();
    o += n;
    let nl = get_u64(b, &mut o) as usize;
    let mut lakes = Vec::with_capacity(nl);
    for _ in 0..nl {
        let id = u32::from_le_bytes(b[o..o + 4].try_into().unwrap());
        o += 4;
        let level = f64::from_le_bytes(b[o..o + 8].try_into().unwrap());
        o += 8;
        let area = f64::from_le_bytes(b[o..o + 8].try_into().unwrap());
        o += 8;
        lakes.push((id, level, area));
    }
    let flow = if o + n * 16 <= b.len() {
        get_f64s(b, &mut o, n * 2)
    } else {
        vec![0.0; n * 2]
    };
    let (surface_kind, lake_cover, rsand) = if o + 2 * n + n * 8 <= b.len() {
        let k = b[o..o + n].to_vec();
        o += n;
        let lc = b[o..o + n].to_vec();
        o += n;
        (k, lc, get_f64s(b, &mut o, n))
    } else {
        (vec![0; n], vec![0; n], vec![f64::MAX; n])
    };
    (
        nx,
        nz,
        cell.0,
        HydroPlanes {
            final_h,
            delta,
            water,
            river,
            lake,
            acc,
            water_dist,
            water_surface,
            lakes,
            flow,
            surface_kind,
            lake_cover,
            rsand,
        },
    )
}

impl Stage for HydrologyStage {
    fn id(&self) -> &str {
        Self::ID
    }
    fn code_version(&self) -> u32 {
        self.code_version
    }
    fn deps(&self) -> &[String] {
        &self.deps
    }
    fn params(&self) -> Vec<u8> {
        params(&(
            self.project.generator_version,
            &self.world.hydrology,
            self.world.sea_level_m,
        ))
    }
    fn run(&self, inputs: &[&Artifact], _seed: u64, ctx: &RunCtx) -> Result<Artifact, Cancelled> {
        let (nx, nz, _hmin, _hmax, h, coast) = decode_height(&inputs[0].bytes);
        let (_g, masks) = decode_masks(&inputs[1].bytes);
        let hydro = crate::hydrology::run(
            &self.grid,
            &h,
            &masks.land,
            self.world.sea_level_m,
            &self.world.hydrology,
            &coast,
            &|| ctx.tick(),
        );
        let n = nx * nz;
        // Prefix is the height-artifact layout with final_h in place of
        // h, so every downstream decode_height consumer just takes
        // world.hydrology instead of world.height on gen >= 3.
        let h_min = hydro.final_h.iter().fold(f64::INFINITY, |a, &x| a.min(x));
        let h_max = hydro
            .final_h
            .iter()
            .fold(f64::NEG_INFINITY, |a, &x| a.max(x));
        let mut b = Vec::with_capacity(24 + n * 8 * 4 + n * 2 + n * 8);
        b.extend_from_slice(&(nx as u64).to_le_bytes());
        b.extend_from_slice(&(nz as u64).to_le_bytes());
        b.extend_from_slice(&h_min.to_le_bytes());
        b.extend_from_slice(&h_max.to_le_bytes());
        for v in hydro.final_h.iter().chain(&coast) {
            b.extend_from_slice(&v.to_le_bytes());
        }
        for v in hydro
            .delta
            .iter()
            .chain(&hydro.water)
            .chain(&hydro.acc)
            .chain(&hydro.water_dist)
            .chain(&hydro.water_surface)
        {
            b.extend_from_slice(&v.to_le_bytes());
        }
        b.extend_from_slice(&hydro.river);
        b.extend_from_slice(&hydro.lake);
        // order changed: acc now comes after masks; see layout doc below.
        b.extend_from_slice(&(hydro.lakes.len() as u64).to_le_bytes());
        for &(id, lv, area) in &hydro.lakes {
            b.extend_from_slice(&id.to_le_bytes());
            b.extend_from_slice(&lv.to_le_bytes());
            b.extend_from_slice(&area.to_le_bytes());
        }
        // code_version 2 tail: flow velocity field (x,z f64 pairs),
        // v3: surface_kind u8 + river-beyond-halfwidth rsand f64.
        for v in &hydro.flow {
            b.extend_from_slice(&v.to_le_bytes());
        }
        b.extend_from_slice(&hydro.surface_kind);
        b.extend_from_slice(&hydro.lake_cover);
        for v in &hydro.rsand {
            b.extend_from_slice(&v.to_le_bytes());
        }
        Ok(Artifact::new(b))
    }
}

pub struct LayersStage {
    pub code_version: u32,
    pub grid: Grid,
    pub project: ProjectFile,
    pub world: WorldRecipe,
    deps: Vec<String>,
}

impl LayersStage {
    pub const ID: &'static str = "world.layers";
    pub fn new(grid: Grid, project: ProjectFile, world: WorldRecipe, code_version: u32) -> Self {
        let hd = height_dep(project.generator_version);
        LayersStage {
            code_version,
            grid,
            project,
            world,
            deps: vec![hd, MasksStage::ID.to_string()],
        }
    }
}

impl Stage for LayersStage {
    fn id(&self) -> &str {
        Self::ID
    }
    fn deps(&self) -> &[String] {
        &self.deps
    }
    fn code_version(&self) -> u32 {
        self.code_version
    }
    fn params(&self) -> Vec<u8> {
        params(&serde_json::json!({
            "cell_size_m": self.grid.cell,
            "generator_version": self.project.generator_version,
        }))
    }
    fn run(&self, inputs: &[&Artifact], _seed: u64, _ctx: &RunCtx) -> Result<Artifact, Cancelled> {
        Ok({
            let (_nx, _nz, h_min, h_max, heights, coast) = decode_height(&inputs[0].bytes);
            let (_g, masks) = decode_masks(&inputs[1].bytes);
            let hf = HeightField {
                h: heights,
                h_min,
                h_max,
            };
            let mut bundle = outputs::build_layers_bundle(&self.grid, &self.world, &hf, &masks);
            if self.project.generator_version >= 2 {
                // Whole-world splat at node resolution.
                let mut rgba = Vec::with_capacity(self.grid.nx * self.grid.nz * 4);
                for j in 0..self.grid.nz {
                    for i in 0..self.grid.nx {
                        let idx = j * self.grid.nx + i;
                        let g = crate::mesh::node_slope(
                            &|ii, jj| hf.h[jj * self.grid.nx + ii],
                            i,
                            j,
                            self.grid.nx - 1,
                            self.grid.nz - 1,
                            self.grid.cell,
                        );
                        let w = crate::splat_weights(
                            hf.h[idx],
                            g,
                            coast[idx],
                            f64::MAX,
                            self.world.sea_level_m,
                            self.world.profile.snow_line_m,
                            &self.world.texturing,
                        );
                        for x in w {
                            rgba.push((x * 255.0).round() as u8);
                        }
                    }
                }
                bundle.files.push((
                    "layers/splat.png".to_string(),
                    outputs::rgba_png(&rgba, self.grid.nx as u32, self.grid.nz as u32),
                ));
            }
            if self.project.generator_version >= 3 {
                let (_hx, _hz, _c, hp) = decode_hydro(&inputs[0].bytes);
                bundle
                    .files
                    .push(("layers/river.u8".to_string(), hp.river.clone()));
                bundle
                    .files
                    .push(("layers/lake.u8".to_string(), hp.lake.clone()));
                let mut f32s = Vec::with_capacity(hp.acc.len() * 4);
                for a in &hp.acc {
                    f32s.extend_from_slice(&(*a as f32).to_le_bytes());
                }
                bundle.files.push(("layers/flow_acc.f32".to_string(), f32s));
                // Water-tinted hillshade: river|lake nodes painted blue.
                let mut wmask = hp.river.clone();
                for (i, v) in hp.lake.iter().enumerate() {
                    wmask[i] |= v;
                }
                for (p, bytes) in bundle.files.iter_mut() {
                    if p == "previews/hillshade.png" {
                        *bytes = outputs::hillshade_water_png(
                            &self.grid,
                            &hf,
                            self.world.sea_level_m,
                            &wmask,
                        );
                    }
                }
                // lakes list in layers.json
                for (p, bytes) in bundle.files.iter_mut() {
                    if p == "layers/layers.json" {
                        let mut j: serde_json::Value = serde_json::from_slice(bytes).unwrap();
                        j["lakes"] = serde_json::json!(
                            hp.lakes
                                .iter()
                                .map(|&(id, lv, area)| {
                                    serde_json::json!({"id": id, "level": lv, "area_m2": area})
                                })
                                .collect::<Vec<_>>()
                        );
                        *bytes = serde_json::to_vec(&j).unwrap();
                    }
                }
            }
            bundle.encode()
        })
    }
}

/// world.chunk.{cx}_{cz}: sliced stage, dep = world.height. Artifact is
/// header + GLB. The slice is the chunk's node range + edge aprons +
/// coast, so a height edit only re-runs the chunks whose slice changed.
pub struct ChunkStage {
    pub code_version: u32,
    pub grid: Grid,
    pub project: ProjectFile,
    pub world: WorldRecipe,
    pub cx: usize,
    pub cz: usize,
    stage_id: String,
    deps: Vec<String>,
}

pub fn chunk_stage_id(cx: usize, cz: usize) -> String {
    format!("world.chunk.{cx}_{cz}")
}

impl ChunkStage {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        grid: Grid,
        project: ProjectFile,
        world: WorldRecipe,
        cx: usize,
        cz: usize,
        code_version: u32,
    ) -> Self {
        let hd = height_dep(project.generator_version);
        ChunkStage {
            code_version,
            grid,
            project,
            world,
            cx,
            cz,
            stage_id: chunk_stage_id(cx, cz),
            deps: vec![hd],
        }
    }
}

impl mb_pipeline::SlicedStage for ChunkStage {
    fn id(&self) -> &str {
        &self.stage_id
    }
    fn deps(&self) -> &[String] {
        &self.deps
    }
    fn code_version(&self) -> u32 {
        self.code_version
    }
    fn params(&self) -> Vec<u8> {
        params(&serde_json::json!({
            "cx": self.cx,
            "cz": self.cz,
            "chunk_size_m": self.world.chunk_size_m,
            "cell_size_m": self.grid.cell,
            "mesh_step_cells": self.world.export.mesh_step_cells,
            "sea_level_m": self.world.sea_level_m,
            "snow_line_m": self.world.profile.snow_line_m,
            "generator_version": self.project.generator_version,
            "seed": format!("{:016x}", self.project.seed),
        }))
    }
    fn slice(&self, inputs: &[&Artifact]) -> Vec<u8> {
        let (nx, nz, h_min, h_max, heights, coast) = decode_height(&inputs[0].bytes);
        let hf = HeightField {
            h: heights,
            h_min,
            h_max,
        };
        let _ = (nx, nz);
        let cells_per_chunk = (self.world.chunk_size_m / self.grid.cell) as usize;
        #[allow(clippy::type_complexity)]
        let mut extra: Option<(Vec<u8>, Vec<f64>, Vec<u8>, Vec<f64>, Vec<u8>)> = None;
        let water: Option<(Vec<f64>, Vec<f64>, Vec<f64>)> = if self.project.generator_version >= 3 {
            let hp = decode_hydro(&inputs[0].bytes).3;
            let kind: Vec<u8> = (0..hp.river.len())
                .map(|c| {
                    if hp.river[c] == 1 {
                        1
                    } else if hp.lake[c] != 0 {
                        2
                    } else {
                        0
                    }
                })
                .collect();
            extra = Some((kind, hp.flow, hp.surface_kind, hp.rsand, hp.lake_cover));
            Some((hp.water_dist, hp.water_surface, hp.water))
        } else {
            None
        };
        crate::slice::encode_chunk_slice(
            &self.grid,
            &hf,
            &coast,
            cells_per_chunk,
            self.cx,
            self.cz,
            water
                .as_ref()
                .map(|(a, b, c)| (a.as_slice(), b.as_slice(), c.as_slice())),
            extra.as_ref().map(|(k, f, sk, rs, lc)| {
                (
                    k.as_slice(),
                    f.as_slice(),
                    sk.as_slice(),
                    rs.as_slice(),
                    lc.as_slice(),
                )
            }),
        )
    }
    fn run(&self, slice: &[u8], _seed: u64, _ctx: &RunCtx) -> Result<Artifact, Cancelled> {
        Ok({
            let cs = crate::slice::decode_chunk_slice(slice);
            let cells_per_chunk = (self.world.chunk_size_m / self.grid.cell) as usize;
            let step = self.world.export.mesh_step_cells as usize;
            let (gi_hi, gj_hi) = (self.grid.nx - 1, self.grid.nz - 1);
            let cell = self.grid.cell;
            let h_at = |i: usize, j: usize| cs.h_at(i, j);
            let normal_at =
                |i: usize, j: usize| crate::mesh::node_normal(&h_at, i, j, gi_hi, gj_hi, cell);
            let coast_at = |i: usize, j: usize| cs.water_dist_at(i, j);
            let gen2 = self.project.generator_version >= 2;
            let n_texels = cells_per_chunk + 1;
            let tex2 = if gen2 {
                Some((&self.world.texturing, n_texels))
            } else {
                None
            };
            let mesh = crate::mesh::chunk_mesh_view(
                &self.grid,
                h_at,
                normal_at,
                coast_at,
                self.world.sea_level_m,
                self.world.profile.snow_line_m,
                cells_per_chunk,
                step,
                self.cx,
                self.cz,
                tex2,
            );
            let id = mb_seed::EntityId::from_path(
                self.project.seed,
                &[
                    ("world", "main"),
                    ("chunk", &format!("{}_{}", self.cx, self.cz)),
                ],
            );
            let glb = crate::glb::chunk_glb(
                &mesh,
                &format!("chunk_{}_{}", self.cx, self.cz),
                &id.to_string(),
                self.cx,
                self.cz,
            );
            let mut out = Vec::with_capacity(CHUNK_HEADER + glb.len());
            out.extend_from_slice(&(mesh.vertex_count as u32).to_le_bytes());
            out.extend_from_slice(&(mesh.triangle_count as u32).to_le_bytes());
            out.extend_from_slice(&glb);
            if gen2 {
                // Splat tail: u32 len + RGBA8 PNG at full node res.
                let mut rgba = Vec::with_capacity(n_texels * n_texels * 4);
                for j in 0..n_texels {
                    for i in 0..n_texels {
                        let gi = self.cx * cells_per_chunk + i;
                        let gj = self.cz * cells_per_chunk + j;
                        let g = crate::mesh::node_slope(
                            &h_at,
                            gi,
                            gj,
                            self.grid.nx - 1,
                            self.grid.nz - 1,
                            cell,
                        );
                        let w = crate::splat_weights(
                            cs.h_at(gi, gj),
                            g,
                            cs.water_dist_at(gi, gj),
                            cs.river_sand_at(gi, gj),
                            self.world.sea_level_m,
                            self.world.profile.snow_line_m,
                            &self.world.texturing,
                        );
                        for x in w {
                            rgba.push((x * 255.0).round() as u8);
                        }
                    }
                }
                let png = crate::outputs::rgba_png(&rgba, n_texels as u32, n_texels as u32);
                out.extend_from_slice(&(png.len() as u32).to_le_bytes());
                out.extend_from_slice(&png);
            }
            if self.project.generator_version >= 3 {
                // Water mesh tail: u32 len + GLB (empty vec when the chunk
                // has no wet triangle). Vertices at the mesh stride; a
                // triangle is emitted when ANY of its three vertices is
                // wet (river/lake), so rivers narrower than the stride
                // still surface. A wet vertex uses its own water level;
                // a dry rim vertex borrows the nearest wet level
                // (water_surface) clamped down to the terrain height so
                // the edge tucks under/onto the bank instead of floating.
                let _no_w = crate::hydrology::NO_WATER;

                let side = cells_per_chunk / step + 1;
                let mut wm = crate::mesh::ChunkMesh {
                    positions: Vec::new(),
                    normals: Vec::new(),
                    uvs: Vec::new(),
                    colors: Vec::new(),
                    uv2: Vec::new(),
                    colors4: Vec::new(),
                    indices: Vec::new(),
                    origin: mesh.origin,
                    vertex_count: 0,
                    triangle_count: 0,
                };
                let ox = self.grid.x(self.cx * cells_per_chunk);
                let oz = self.grid.z(self.cz * cells_per_chunk);
                let mut vidx = vec![u32::MAX; side * side];
                let sea = self.world.sea_level_m;
                let depth01 = |level: f64, gi: usize, gj: usize| {
                    ((level - cs.h_at(gi, gj)) / 8.0).clamp(0.0, 1.0)
                };
                let emit_vert = |wm: &mut crate::mesh::ChunkMesh,
                                 vidx: &mut [u32],
                                 vi: usize,
                                 vj: usize,
                                 side: usize,
                                 borrow_lake: bool|
                 -> u32 {
                    let cur = vidx[vj * side + vi];
                    if cur != u32::MAX {
                        return cur;
                    }
                    let (gi, gj) = (
                        self.cx * cells_per_chunk + vi * step,
                        self.cz * cells_per_chunk + vj * step,
                    );
                    let mut level = cs.water_at(gi, gj); // wet level or borrowed
                    // kind of the wet SOURCE: lake-source verts (incl.
                    // the fringe below lake level) keep lake level.
                    let kind = if cs.is_wet(gi, gj) {
                        cs.kind_at(gi, gj)
                    } else {
                        cs.skind_at(gi, gj)
                    };
                    // River-sourced rim clamps to terrain; lake-sourced
                    // stays at lake level only while inside the filled
                    // depression — outside it the sheet flushes onto the
                    // bank (the pour/downhill side must not float).
                    if !cs.is_wet(gi, gj) && (kind != 2 || cs.lake_cover_at(gi, gj) != 1) {
                        level = level.min(cs.h_at(gi, gj));
                    }
                    let id = wm.positions.len() as u32;
                    vidx[vj * side + vi] = id;
                    wm.positions.push([
                        (self.grid.x(gi) - ox) as f32,
                        level as f32,
                        (self.grid.z(gj) - oz) as f32,
                    ]);
                    wm.normals.push([0.0, 1.0, 0.0]);
                    wm.uvs
                        .push([self.grid.x(gi) as f32, self.grid.z(gj) as f32]);
                    let (fx, fz) = if kind == 1 {
                        cs.flow_at(gi, gj)
                    } else {
                        (0.0, 0.0)
                    };
                    wm.uv2.push([fx as f32, fz as f32]);
                    wm.colors4.push([
                        depth01(level, gi, gj) as f32,
                        match kind {
                            1 => 1.0,
                            2 => 0.5,
                            _ if borrow_lake => 0.5, // lake rim stays a lake
                            _ => 0.0,
                        },
                        0.0,
                        1.0,
                    ]);
                    id
                };
                let wet = |vi: usize, vj: usize| {
                    cs.is_wet(
                        self.cx * cells_per_chunk + vi * step,
                        self.cz * cells_per_chunk + vj * step,
                    )
                };
                // Lake coverage: every node whose borrowed surface is a
                // lake AND is at/below that lake's level is covered; the
                // 4-neighbour check adds one ring beyond so the
                // terrain/water intersection is always inside the mesh
                // (flood-fill guarantees the boundary has h >= level).
                let lakey = |vi: usize, vj: usize| -> bool {
                    let (gi, gj) = (
                        self.cx * cells_per_chunk + vi * step,
                        self.cz * cells_per_chunk + vj * step,
                    );
                    cs.lake_cover_at(gi, gj) == 1
                };
                let lakeish = |vi: usize, vj: usize| -> bool {
                    for (di, dj) in [(0i64, 0i64), (-1, 0), (1, 0), (0, -1), (0, 1)] {
                        let ii = vi as i64 + di;
                        let jj = vj as i64 + dj;
                        if ii < 0 || jj < 0 || ii >= side as i64 || jj >= side as i64 {
                            continue;
                        }
                        if lakey(ii as usize, jj as usize) {
                            return true;
                        }
                    }
                    false
                };
                for vj in 0..side - 1 {
                    for vi in 0..side - 1 {
                        let lakey = lakeish(vi, vj)
                            || lakeish(vi + 1, vj)
                            || lakeish(vi, vj + 1)
                            || lakeish(vi + 1, vj + 1);
                        if !(wet(vi, vj)
                            || wet(vi + 1, vj)
                            || wet(vi, vj + 1)
                            || wet(vi + 1, vj + 1)
                            || lakey)
                        {
                            continue;
                        }
                        let a = emit_vert(&mut wm, &mut vidx, vi, vj, side, lakey);
                        let b = emit_vert(&mut wm, &mut vidx, vi + 1, vj, side, lakey);
                        let c = emit_vert(&mut wm, &mut vidx, vi, vj + 1, side, lakey);
                        let d = emit_vert(&mut wm, &mut vidx, vi + 1, vj + 1, side, lakey);
                        // Skip tris fully at/below sea level — the sea
                        // quads cover them and they'd z-fight.
                        let ymax = |id: u32| wm.positions[id as usize][1];
                        let low = |tri: [u32; 3]| tri.iter().all(|&v| ymax(v) as f64 <= sea + 0.01);
                        if (wet(vi, vj) || wet(vi, vj + 1) || wet(vi + 1, vj) || lakey)
                            && !low([a, c, b])
                        {
                            wm.indices.extend_from_slice(&[a, c, b]);
                        }
                        if (wet(vi + 1, vj) || wet(vi, vj + 1) || wet(vi + 1, vj + 1) || lakey)
                            && !low([b, c, d])
                        {
                            wm.indices.extend_from_slice(&[b, c, d]);
                        }
                    }
                }
                // Sea quads on a coarse SEA_STEP grid: a quad is emitted
                // when any corner OR any mesh_step terrain sample inside
                // it is below sea level. Flat at sea level; the engine
                // depth test hides the part under the terrain.
                const SEA_STEP: usize = 16;
                debug_assert!(cells_per_chunk.is_multiple_of(SEA_STEP));
                let sstep = SEA_STEP / step.max(1); // in mesh-step units
                if cells_per_chunk.is_multiple_of(SEA_STEP) && sstep >= 1 {
                    let nside = cells_per_chunk / SEA_STEP;
                    let below = |vi: usize, vj: usize| {
                        // corner check at stride nodes + interior samples
                        let (gi, gj) = (
                            self.cx * cells_per_chunk + vi * step,
                            self.cz * cells_per_chunk + vj * step,
                        );
                        cs.h_at(gi, gj) < sea
                    };
                    let emit_sea = |wm: &mut crate::mesh::ChunkMesh, vi: usize, vj: usize| -> u32 {
                        let (gi, gj) = (
                            self.cx * cells_per_chunk + vi * step,
                            self.cz * cells_per_chunk + vj * step,
                        );
                        wm.positions.push([
                            (self.grid.x(gi) - ox) as f32,
                            sea as f32,
                            (self.grid.z(gj) - oz) as f32,
                        ]);
                        wm.normals.push([0.0, 1.0, 0.0]);
                        wm.uvs
                            .push([self.grid.x(gi) as f32, self.grid.z(gj) as f32]);
                        wm.uv2.push([0.0, 0.0]);
                        wm.colors4
                            .push([depth01(sea, gi, gj) as f32, 0.0, 0.0, 1.0]);
                        (wm.positions.len() - 1) as u32
                    };
                    // Vert dedup across quads via a side² map at sstep stride.
                    let mut sv = vec![u32::MAX; side * side];
                    for qj in 0..nside {
                        for qi in 0..nside {
                            // Quad corners in mesh-step units.
                            let (x0, x1) = (qi * sstep, (qi + 1) * sstep);
                            let (z0, z1) = (qj * sstep, (qj + 1) * sstep);
                            // Corner or interior-sample below sea?
                            let mut any =
                                below(x0, z0) || below(x1, z0) || below(x0, z1) || below(x1, z1);
                            if !any {
                                'scan: for sj in z0..z1 {
                                    for si in x0..x1 {
                                        if below(si, sj) {
                                            any = true;
                                            break 'scan;
                                        }
                                    }
                                }
                            }
                            if !any {
                                continue;
                            }
                            let mut v =
                                |wm: &mut crate::mesh::ChunkMesh, vi: usize, vj: usize| -> u32 {
                                    let cur = sv[vj * side + vi];
                                    if cur != u32::MAX {
                                        return cur;
                                    }
                                    let id = emit_sea(wm, vi, vj);
                                    sv[vj * side + vi] = id;
                                    id
                                };
                            let a = v(&mut wm, x0, z0);
                            let b = v(&mut wm, x1, z0);
                            let c = v(&mut wm, x0, z1);
                            let d = v(&mut wm, x1, z1);
                            wm.indices.extend_from_slice(&[a, c, b]);
                            wm.indices.extend_from_slice(&[b, c, d]);
                        }
                    }
                }
                wm.vertex_count = wm.positions.len();
                wm.triangle_count = wm.indices.len() / 3;
                // Wet verts with no all-wet triangle produce a GLB with an
                // empty index accessor; skip the tail entirely in that case.
                let wglb = if wm.triangle_count == 0 {
                    Vec::new()
                } else {
                    crate::glb::water_glb(
                        &wm,
                        &format!("water_{}_{}", self.cx, self.cz),
                        &id.to_string(),
                        self.cx,
                        self.cz,
                    )
                };
                out.extend_from_slice(&(wglb.len() as u32).to_le_bytes());
                out.extend_from_slice(&wglb);
            }
            Artifact::new(out)
        })
    }
}

/// world.region.{id}.chunk.{cx}_{cz}: sliced, deps height + distance.
pub struct RegionChunkStage {
    pub code_version: u32,
    pub grid: Grid,
    pub project: ProjectFile,
    pub world: WorldRecipe,
    pub region: mb_schema::DetailRegion,
    pub cx: usize,
    pub cz: usize,
    stage_id: String,
    deps: Vec<String>,
}

pub fn region_chunk_stage_id(region: &str, cx: usize, cz: usize) -> String {
    format!("world.region.{region}.chunk.{cx}_{cz}")
}

impl RegionChunkStage {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        grid: Grid,
        project: ProjectFile,
        world: WorldRecipe,
        region: mb_schema::DetailRegion,
        cx: usize,
        cz: usize,
        code_version: u32,
    ) -> Self {
        let stage_id = region_chunk_stage_id(&region.id, cx, cz);
        let hd = height_dep(project.generator_version);
        RegionChunkStage {
            code_version,
            grid,
            project,
            world,
            region,
            cx,
            cz,
            stage_id,
            deps: vec![hd, DistanceStage::ID.to_string()],
        }
    }
}

impl mb_pipeline::SlicedStage for RegionChunkStage {
    fn id(&self) -> &str {
        &self.stage_id
    }
    fn deps(&self) -> &[String] {
        &self.deps
    }
    fn code_version(&self) -> u32 {
        self.code_version
    }
    fn params(&self) -> Vec<u8> {
        // The region boundary rect is params, not data: w depends on it.
        params(&serde_json::json!({
            "cx": self.cx,
            "cz": self.cz,
            "region": self.region,
            "terrain": self.world.terrain,
            "profile": self.world.profile,
            "cell_size_m": self.grid.cell,
            "chunk_size_m": self.world.chunk_size_m,
            "mesh_step_cells": self.world.export.mesh_step_cells,
            "sea_level_m": self.world.sea_level_m,
            "generator_version": self.project.generator_version,
            "seed": format!("{:016x}", self.project.seed),
        }))
    }
    fn slice(&self, inputs: &[&Artifact]) -> Vec<u8> {
        let (_nx, _nz, h_min, h_max, heights, coast) = decode_height(&inputs[0].bytes);
        let (_dnx, _dnz, _dcoast, mountain) = decode_distance(&inputs[1].bytes);
        let hf = HeightField {
            h: heights,
            h_min,
            h_max,
        };
        let cells_per_chunk = (self.world.chunk_size_m / self.grid.cell) as usize;
        let step = self.world.export.mesh_step_cells as usize;
        let (delta, wd): (Option<Vec<f64>>, Option<Vec<f64>>) =
            if self.project.generator_version >= 3 {
                let hp = decode_hydro(&inputs[0].bytes).3;
                (Some(hp.delta), Some(hp.water_dist))
            } else {
                (None, None)
            };
        crate::slice::encode_region_slice(
            &self.grid,
            &hf,
            &coast,
            &mountain,
            cells_per_chunk,
            step,
            self.cx,
            self.cz,
            delta.as_deref(),
            wd.as_deref(),
        )
    }
    fn run(&self, slice: &[u8], _seed: u64, ctx: &RunCtx) -> Result<Artifact, Cancelled> {
        Ok({
            let rs = crate::slice::decode_region_slice(slice);
            crate::region_slice::run_region_chunk(
                &self.grid,
                &rs,
                &self.world,
                &self.region,
                self.project.seed,
                self.cx,
                self.cz,
                &|| ctx.tick(),
                self.project.generator_version,
            )
        })
    }
}

/// world.manifest: builds manifest.json from the chunk artifacts and
/// the layers.json inside the layers bundle (h_min/h_max live there).
/// deps = world.layers + all chunk stages + all region chunk stages.
pub struct ManifestStage {
    pub code_version: u32,
    pub grid: Grid,
    pub project: ProjectFile,
    pub world: WorldRecipe,
    /// Stage ids in input order: [layers] + chunks (cz,cx) + region
    /// chunks sorted by (region id, cz, cx).
    deps: Vec<String>,
}

impl ManifestStage {
    pub const ID: &'static str = "world.manifest";
    pub fn new(grid: Grid, project: ProjectFile, world: WorldRecipe, code_version: u32) -> Self {
        let cells_per_chunk = (world.chunk_size_m / grid.cell) as usize;
        let ncx = (grid.nx - 1) / cells_per_chunk;
        let ncz = (grid.nz - 1) / cells_per_chunk;
        let mut deps = vec![LayersStage::ID.to_string()];
        for cz in 0..ncz {
            for cx in 0..ncx {
                deps.push(chunk_stage_id(cx, cz));
            }
        }
        // Region chunk ids sorted by (region id, cz, cx): first sort the
        // regions by id, then emit their chunks in (cz, cx) order.
        let mut regs: Vec<&mb_schema::DetailRegion> = world.detail_regions.iter().collect();
        regs.sort_by(|a, b| a.id.cmp(&b.id));
        for r in regs {
            for cz in r.chunk_min[1]..=r.chunk_max[1] {
                for cx in r.chunk_min[0]..=r.chunk_max[0] {
                    deps.push(region_chunk_stage_id(&r.id, cx as usize, cz as usize));
                }
            }
        }
        ManifestStage {
            code_version,
            grid,
            project,
            world,
            deps,
        }
    }
}

impl Stage for ManifestStage {
    fn id(&self) -> &str {
        Self::ID
    }
    fn deps(&self) -> &[String] {
        &self.deps
    }
    fn code_version(&self) -> u32 {
        self.code_version
    }
    fn params(&self) -> Vec<u8> {
        params(&serde_json::json!({
            "world": self.world,
            "cell_size_m": self.grid.cell,
            "generator_version": self.project.generator_version,
            "seed": format!("{:016x}", self.project.seed),
        }))
    }
    fn run(&self, inputs: &[&Artifact], _seed: u64, _ctx: &RunCtx) -> Result<Artifact, Cancelled> {
        Ok({
            // input 0 = layers bundle -> parse layers.json for h_min/h_max
            let lb = FileBundle::decode(&inputs[0].bytes);
            let lj: serde_json::Value = serde_json::from_slice(
                &lb.files
                    .iter()
                    .find(|(p, _)| p == "layers/layers.json")
                    .expect("layers.json")
                    .1,
            )
            .expect("layers.json parse");
            let (h_min, h_max) = (lj["h_min"].as_f64().unwrap(), lj["h_max"].as_f64().unwrap());
            let cells_per_chunk = (self.world.chunk_size_m / self.grid.cell) as usize;
            let ncx = (self.grid.nx - 1) / cells_per_chunk;
            let ncz = (self.grid.nz - 1) / cells_per_chunk;
            // superseded map
            let mut superseded: std::collections::BTreeMap<(u32, u32), &str> =
                std::collections::BTreeMap::new();
            for r in &self.world.detail_regions {
                for cx in r.chunk_min[0]..=r.chunk_max[0] {
                    for cz in r.chunk_min[1]..=r.chunk_max[1] {
                        superseded.insert((cx, cz), r.id.as_str());
                    }
                }
            }
            let mut k = 1usize; // inputs index after layers
            let mut chunk_entries = Vec::new();
            let (mut total_v, mut total_t) = (0usize, 0usize);
            for cz in 0..ncz {
                for cx in 0..ncx {
                    let a = &inputs[k];
                    k += 1;
                    let vc = u32::from_le_bytes(a.bytes[0..4].try_into().unwrap()) as usize;
                    let tc = u32::from_le_bytes(a.bytes[4..8].try_into().unwrap()) as usize;
                    let glb_len = u32::from_le_bytes(
                        a.bytes[CHUNK_HEADER + 8..CHUNK_HEADER + 12]
                            .try_into()
                            .unwrap(),
                    ) as usize;
                    let glb = &a.bytes[CHUNK_HEADER..CHUNK_HEADER + glb_len];
                    let id = mb_seed::EntityId::from_path(
                        self.project.seed,
                        &[("world", "main"), ("chunk", &format!("{cx}_{cz}"))],
                    );
                    let ox = self.grid.x(cx * cells_per_chunk);
                    let oz = self.grid.z(cz * cells_per_chunk);
                    let mut entry = serde_json::json!({
                        "id": id.to_string(),
                        "cx": cx,
                        "cz": cz,
                        "file": format!("chunk_{cx}_{cz}.glb"),
                        "origin": [ox, 0.0, oz],
                        "vertex_count": vc,
                        "triangle_count": tc,
                        "blake3": mb_seed::hex_bytes(&mb_seed::hash_bytes(glb)),
                    });
                    if let Some(rid) = superseded.get(&(cx as u32, cz as u32)) {
                        entry["superseded_by"] = serde_json::json!(rid);
                    }
                    let tail = &a.bytes[CHUNK_HEADER + glb_len..];
                    if !tail.is_empty() {
                        let sl = u32::from_le_bytes(tail[0..4].try_into().unwrap()) as usize;
                        entry["splat"] = serde_json::json!({
                            "file": format!("splat/chunk_{cx}_{cz}.png"),
                            "blake3": mb_seed::hex_bytes(&mb_seed::hash_bytes(&tail[4..4 + sl])),
                        });
                        // gen 3: water GLB tail after the splat.
                        let wl = &tail[4 + sl..];
                        if wl.len() >= 4 {
                            let wn = u32::from_le_bytes(wl[0..4].try_into().unwrap()) as usize;
                            if wn > 0 {
                                entry["water"] = serde_json::json!({
                                    "file": format!("water/chunk_{cx}_{cz}.glb"),
                                    "blake3": mb_seed::hex_bytes(&mb_seed::hash_bytes(&wl[4..4 + wn])),
                                });
                            }
                        }
                    }
                    chunk_entries.push(entry);
                    total_v += vc;
                    total_t += tc;
                }
            }
            // Regions sorted by id; their chunk inputs follow in the same
            // (region id, cz, cx) order as deps.
            let mut regs: Vec<&mb_schema::DetailRegion> =
                self.world.detail_regions.iter().collect();
            regs.sort_by(|a, b| a.id.cmp(&b.id));
            let mut region_manifest = Vec::new();
            for r in regs {
                let mut entries = Vec::new();
                for cz in r.chunk_min[1]..=r.chunk_max[1] {
                    for cx in r.chunk_min[0]..=r.chunk_max[0] {
                        let a = &inputs[k];
                        k += 1;
                        let vc = u32::from_le_bytes(a.bytes[0..4].try_into().unwrap()) as usize;
                        let tc = u32::from_le_bytes(a.bytes[4..8].try_into().unwrap()) as usize;
                        let glb_len = u32::from_le_bytes(
                            a.bytes[CHUNK_HEADER + 8..CHUNK_HEADER + 12]
                                .try_into()
                                .unwrap(),
                        ) as usize;
                        let glb = &a.bytes[CHUNK_HEADER..CHUNK_HEADER + glb_len];
                        let (cx, cz) = (cx as usize, cz as usize);
                        let key = format!("{cx}_{cz}");
                        let id = mb_seed::EntityId::from_path(
                            self.project.seed,
                            &[("world", "main"), ("region", &r.id), ("chunk", &key)],
                        );
                        let ox = self.grid.x(cx * cells_per_chunk);
                        let oz = self.grid.z(cz * cells_per_chunk);
                        let mut e = serde_json::json!({
                            "id": id.to_string(),
                            "cx": cx,
                            "cz": cz,
                            "region": r.id,
                            "file": format!("regions/{}/chunk_{cx}_{cz}.glb", r.id),
                            "origin": [ox, 0.0, oz],
                            "vertex_count": vc,
                            "triangle_count": tc,
                            "blake3": mb_seed::hex_bytes(&mb_seed::hash_bytes(glb)),
                        });
                        let tail = &a.bytes[CHUNK_HEADER + glb_len..];
                        if !tail.is_empty() {
                            let sl = u32::from_le_bytes(tail[0..4].try_into().unwrap()) as usize;
                            e["splat"] = serde_json::json!({
                                "file": format!("regions/{}/splat/chunk_{cx}_{cz}.png", r.id),
                                "blake3": mb_seed::hex_bytes(&mb_seed::hash_bytes(&tail[4..4 + sl])),
                            });
                        }
                        entries.push(e);
                        total_v += vc;
                        total_t += tc;
                    }
                }
                region_manifest.push(serde_json::json!({
                    "id": r.id,
                    "chunk_min": r.chunk_min,
                    "chunk_max": r.chunk_max,
                    "cell_divisor": r.cell_divisor,
                    "mesh_step_cells": r.mesh_step_cells,
                    "extra_octaves": r.extra_octaves,
                    "blend_band_m": r.blend_band_m,
                    "chunks": entries,
                }));
            }
            let hf = HeightField {
                h: Vec::new(),
                h_min,
                h_max,
            };
            let bytes = outputs::manifest_json(
                &self.project,
                &self.world,
                &self.grid,
                &hf,
                chunk_entries,
                region_manifest,
                ncx * ncz,
                total_v,
                total_t,
            );
            // gen 3: copy the lakes list from layers.json.
            if self.project.generator_version >= 3 {
                let mut j: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
                let layers_json = crate::outputs::FileBundle::decode(&inputs[0].bytes)
                    .files
                    .iter()
                    .find(|(p, _)| p == "layers/layers.json")
                    .map(|(_, b)| serde_json::from_slice::<serde_json::Value>(b).unwrap());
                if let Some(lj) = layers_json {
                    j["lakes"] = lj["lakes"].clone();
                }
                j["sea"] = serde_json::json!({
                    "file": "water/sea_ring.glb",
                    "level": self.world.sea_level_m,
                });
                Artifact::new(serde_json::to_vec(&j).unwrap())
            } else {
                Artifact::new(bytes)
            }
        })
    }
}

// ---------------------------------------------------------------------
// House stages (Phase 3): one stage per house, independent of terrain.
// ---------------------------------------------------------------------

/// Stage producing the 3 LOD GLBs + manifest summary for one house.
pub struct HouseStage {
    id: String,
    spec: mb_schema::HouseSpec,
    seed: u64,
}

impl HouseStage {
    pub fn new(spec: mb_schema::HouseSpec, project_seed: u64) -> Self {
        HouseStage {
            id: format!("house.{}", spec.id),
            seed: mb_house::house_seed(project_seed, &spec.id),
            spec,
        }
    }
    pub fn stage_id(id: &str) -> String {
        format!("house.{id}")
    }
}

impl mb_pipeline::Stage for HouseStage {
    fn id(&self) -> &str {
        &self.id
    }
    fn deps(&self) -> &[String] {
        &[]
    }
    fn code_version(&self) -> u32 {
        mb_house::CURRENT_HOUSE_GENERATOR_VERSION
    }
    fn params(&self) -> Vec<u8> {
        let mut b = serde_json::to_vec(&self.spec).unwrap_or_default();
        b.extend_from_slice(&self.seed.to_le_bytes());
        b
    }
    fn run(
        &self,
        _inputs: &[&mb_pipeline::Artifact],
        _seed: u64,
        _ctx: &mb_pipeline::RunCtx,
    ) -> Result<mb_pipeline::Artifact, mb_pipeline::Cancelled> {
        let h = mb_house::generate(&self.spec, self.seed);
        let mut files = Vec::new();
        let mut tris = [0usize; 3];
        for (li, lod) in h.lods.iter().enumerate() {
            tris[li] = lod.prims.iter().map(|p| p.indices.len() / 3).sum();
            files.push((
                format!("lod{li}.glb"),
                mb_house::glb::lod_glb(lod, &self.spec.id, li as u8, h.plaster_rgb),
            ));
        }
        let colliders: Vec<serde_json::Value> = h
            .colliders
            .iter()
            .map(|c| serde_json::json!({"center": c.center, "half": c.half}))
            .collect();
        let summary = serde_json::json!({
            "id": self.spec.id,
            "footprint": self.spec.footprint,
            "floors": self.spec.floors,
            "placement": self.spec.placement,
            "tris": {"lod0": tris[0], "lod1": tris[1], "lod2": tris[2]},
            "materials": mb_house::glb::MAT_NAMES,
            "colliders": colliders,
            "bounds": {"min": h.bounds.0, "max": h.bounds.1},
            "plaster_rgb": h.plaster_rgb,
        });
        files.push((
            "summary.json".to_string(),
            serde_json::to_vec(&summary).unwrap_or_default(),
        ));
        Ok(crate::outputs::FileBundle { files }.encode())
    }
}
