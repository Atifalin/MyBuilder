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
        let stage_id = region_stage_id(&region.id);
        RegionStage {
            code_version,
            grid,
            project,
            world,
            region,
            stage_id,
            deps: vec![
                HeightStage::ID.to_string(),
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

/// world.layers: the raster layer bundle (r16 x2, masks, layers.json,
/// hillshade preview). Normal stage, deps height + masks.
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
        LayersStage {
            code_version,
            grid,
            project,
            world,
            deps: vec![HeightStage::ID.to_string(), MasksStage::ID.to_string()],
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
        ChunkStage {
            code_version,
            grid,
            project,
            world,
            cx,
            cz,
            stage_id: chunk_stage_id(cx, cz),
            deps: vec![HeightStage::ID.to_string()],
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
        crate::slice::encode_chunk_slice(&self.grid, &hf, &coast, cells_per_chunk, self.cx, self.cz)
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
            let coast_at = |i: usize, j: usize| cs.coast_at(i, j);
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
                            coast_at(gi, gj),
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
        RegionChunkStage {
            code_version,
            grid,
            project,
            world,
            region,
            cx,
            cz,
            stage_id,
            deps: vec![HeightStage::ID.to_string(), DistanceStage::ID.to_string()],
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
        crate::slice::encode_region_slice(
            &self.grid,
            &hf,
            &coast,
            &mountain,
            cells_per_chunk,
            step,
            self.cx,
            self.cz,
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
            Artifact::new(bytes)
        })
    }
}
