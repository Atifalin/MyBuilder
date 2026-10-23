# My Builder: Gaps, Solutions, Algorithms and Math

A direction document. Part 1 lists the gaps found while designing the plan, the goals file and the GUI prompts, with a solution direction for each. Part 2 lists the algorithms and math each module needs. Part 3 holds a formula sheet, Part 4 a spike-by-spike plan for proving the hard parts, and Part 5 a reading list.

**Legend:** **[Verified]** means I checked it against current documentation on 2026-10-23 (sources at the end). **[Proposed]** is my recommendation. **[Unverified]** means I could not confirm it and it needs a quick test before you rely on it.

---

## Part 0: Research Findings That Change the Plan

1. **Godot baseline [Verified].** The current stable line is Godot 4.7 (4.7.0 shipped on 2026-06-18, with maintenance releases since). Target 4.7.x as the baseline, and re-check before each phase.
2. **Instancing in Godot [Verified].** MultiMesh draws thousands of instances in one draw call, but all instances are spatially indexed as one object, and the visibility AABB must be supplied by the user. So instances must be grouped per chunk and given a custom AABB, or far-away instances will always render.
3. **glTF instancing [Partly verified].** Blender's glTF exporter writes instances with EXT_mesh_gpu_instancing and its importer creates objects sharing mesh data. I could not confirm Godot's import support for that extension. **Do not depend on it.** The Godot plugin should read our manifest and build MultiMeshes itself.
4. **Blender extension packaging [Verified].** Blender extensions can bundle Python wheels, with platform-specific zips (`--split-platforms`). The docs require bundled wheels to be unmodified from Python's package index. So the Rust core wheel will probably need to be **published on PyPI** to ship on the official Blender extensions platform, or the addon is distributed outside it. Whether Blender accepts abi3-tagged wheels is **[Unverified]**.
5. **Terrain export target [Verified].** The Terrain3D addon (Godot GDExtension, MIT) imports heightmaps as r16/raw or exr, slices them into regions (1024 by default), and r16 files do not store min/max height or size. So our manifest must carry the height range and dimensions. Its control maps use its own format, so splat weights need an importer script.
6. **Rust geometry stack [Verified].** `i_overlay` provides polygon booleans and polygon/stroke offsetting, has integer and float APIs, and is the overlay engine inside `geo`. Delaunay comes through `spade`. An integer-coordinate pipeline is attractive for determinism (see gap 3).
7. **godot-rust (`gdext`) [Verified].** Usable and actively documented. Double-precision needs a custom Godot build, so do not rely on it. It is MPL 2.0, which only requires sharing changes to gdext itself.

---

## Part 1: Gaps and Solutions

| # | Gap | Why it matters | Solution direction |
|---|---|---|---|
| 1 | **No 3D viewport means a slow feedback loop.** | Users cannot judge a result without opening Blender or Godot. | Core renders 2D schematic previews: hillshaded height map, footprint/lot plan, floor plans, budget bars. Engine hot-reload covers the 3D side. |
| 2 | **Wizard answers can contradict each other.** | "200 buildings, small area, sparse" is impossible. | Capacity model (3.12). Show a live feasibility readout and auto-adjust or warn. |
| 3 | **Determinism is harder than "use a seed".** | Rebuild, caching and MCP all assume identical output on every machine. | Counter-based RNG, order-independent seeds (3.1), integer geometry for topology, no hash-map iteration order, cross-OS golden hashes in CI. |
| 4 | **Stable IDs break when topology changes.** | Add one road and the lots change, so overrides and engine files lose their targets. | Hierarchical seed keys plus IoU matching of old and new lots (3.2). Unmatched overrides go to an "orphaned edits" list. |
| 5 | **Overrides have no anchoring model.** | Manual edits need to survive regeneration. | Store overrides as patches with an ID and a spatial anchor, rebased after each rebuild. |
| 6 | **Tile seams between world and city regions.** | A regenerated mountain must match the world mesh at the edges. | All noise and detail is a function of **world coordinates**, never tile index. Two resolutions: eroded base map plus position-based detail. Shared border samples (2^k+1 vertices). |
| 7 | **Large-world precision.** | Jitter far from the origin in Godot's standard float builds. | Chunk-local f32 coordinates, f64 or i64 in the core, per-chunk origin transforms. |
| 8 | **Unlimited canvas has no data structure yet.** | Strokes and blobs on an infinite page need storage. | Sparse tile pyramid for rasters, vector strokes for shapes, rasterize on demand. |
| 9 | **"Feel" questions have no mapping to math.** | "Bumpy, hilly, mountainous" must become parameters. | Parameter table (3.3) with a few presets users can fine-tune. |
| 10 | **Roads and rivers need logic, not lines.** | The "mathematical and logical" promise. | Hydrology (priority-flood plus flow accumulation) and slope-cost routing (2.4, 2.5). |
| 11 | **Mesh merging conflicts with "explodable".** | Merged static meshes cannot break apart. | Two export profiles: **static merged** and **destructible** (separable parts with groups). Choose per pack or per distance. Pre-fracture is a later feature. |
| 12 | **Signage in Indian and Arabic scripts.** | Complex scripts need shaping and right-to-left layout. | Decide where text renders: baked sign textures from the core (rustybuzz plus MSDF) or engine-side labels. Start with baked textures. |
| 13 | **Style authenticity.** | An Indian pack that feels generic will not sell. | Choose one flavor, collect reference sets, define the rules from real plans and facades, and get review from someone who knows the style. |
| 14 | **Pack format undefined.** | Packs are the product. | A declarative, versioned, sandboxed rules format (2.12), separate from code. |
| 15 | **Selling offline software.** | Local-first conflicts with license checks. | Signed pack and license files (Ed25519), verified offline. Keep DRM light. |
| 16 | **Long builds need control.** | Users need progress and cancel. | Cooperative cancellation tokens, parallel stages (rayon), structured progress events. |
| 17 | **Failures need a language.** | "Build failed" is useless. | Structured diagnostics with severity, location ID and a suggested fix, shown in the log and mirrored to the engine plugin. |
| 18 | **Undo/redo and autosave.** | Non-destructive editing is expected. | Recipes are data, so history is a stack of recipe diffs. Autosave writes versioned snapshots. |
| 19 | **Coordinate and unit conventions.** | Y-up vs Z-up and handedness bugs. | Core uses glTF conventions (meters, right-handed, Y-up). Adapters convert once. |
| 20 | **Variety vs the part cache.** | Too many unique parameters kills instancing. | Quantize parameters into buckets (3.8) and report cache hit rate as a metric. |
| 21 | **Navmesh responsibility.** | Who bakes it? | Export walkable polygons (roads, lots, floors) as data and let the engine bake, with an optional pre-baked mode. |
| 22 | **Preview data across the UI boundary.** | The Tauri UI needs map and plan previews without heavy IPC. | Send vectors down, receive PNG preview tiles at the visible zoom only. |
| 23 | **Name and license hygiene.** | Product name conflicts, third-party licenses. | Check the name early. Track licenses (gdext MPL 2.0, Terrain3D MIT) in a manifest. |
| 24 | **Testing without a viewport.** | Visual bugs hide in data. | Golden hashes, property tests, 2D contact-sheet renders, and seam and budget tests. |

---

## Part 2: Algorithms by Module

### 2.1 Foundations
- **Deterministic RNG:** counter-based generators (Philox or a SplitMix-style hash) so any entity's random stream depends only on its key, never on generation order. This makes parallel generation reproducible.
- **Hashing:** BLAKE3 for stage and content hashes (fast, stable).
- **Spatial indexes:** uniform grid for neighbor queries, R-tree (`rstar`) for polygons and lots.
- **Integer geometry:** snap 2D topology to an integer grid (for example, millimeters) so booleans, offsets and graph building are exact and identical everywhere.
- **Robust predicates:** Shewchuk-style exact orientation tests for anything that remains in floating point.

### 2.2 Rebuild engine
- **Dependency graph (DAG)** of stages. Each node caches `(input hash, output)`. Input hash = hash(relevant params, upstream output hashes, code version).
- **Dirty propagation:** a changed parameter invalidates only dependent nodes. Add **spatial invalidation** so a change to one district invalidates only the chunks it touches.
- **Identity matching:** after regeneration, match old and new entities by overlap (IoU above a threshold, or Hungarian assignment on centroid distance plus IoU) to carry IDs and overrides across.
- **Incremental computation** ideas to borrow: memoized queries (as in Salsa) and self-adjusting computation. Do not adopt a framework until the hand-written version hurts.
- **Output diffing:** write only changed files and update the manifest atomically.

### 2.3 2D map editing
- **Stroke to shape:** Douglas-Peucker simplification, then Chaikin corner cutting or Catmull-Rom splines for smooth coastlines.
- **Boolean edits** (union, difference) with a Vatti-style clipper (`i_overlay`).
- **Rasterization:** scanline polygon fill into tile masks (land, water, forest).
- **Distance fields:** Euclidean distance transform (Felzenszwalb-Huttenlocher) or jump flooding for signed distance to coast, rivers and roads. These drive beaches, shelves, blending and falloffs.
- **Marching squares** to turn masks back into clean vector outlines.

### 2.4 Terrain and hydrology
- **Base height:** `h = base(d) + A_m * M(p) * ridged(p) + A_h * fBm(p)` where `d` is signed distance to the coast and `M` is a painted mountain mask (formulas in Part 3).
- **Noise:** Perlin or OpenSimplex, evaluated in world coordinates. Write the noise yourself (or pin a crate) so output never changes between versions.
- **Domain warping** for natural-looking ridges and coastlines.
- **Erosion:** particle-based hydraulic erosion and thermal erosion (talus angle). Run once on the base map; add position-based detail afterward so regions regenerate without re-eroding.
- **Hydrology:** priority-flood depression filling, D8 (or D-infinity) flow directions, flow accumulation by processing cells from high to low, rivers where accumulation exceeds a threshold, lakes where fills are deep, channel carving with width growing with flow.
- **Derived layers:** slope and aspect by central differences, hillshade for 2D previews, curvature for texture rules.
- **Texturing:** weight rules from slope, height, biome and distance to water (rock on steep slopes, sand near water, grass otherwise), normalized and packed into RGBA splat maps.
- **Chunking:** (2^k)+1 vertices per side so neighbors share border samples; skirts or stitching for LOD transitions; per-chunk origin.
- **Export:** 16-bit height tiles (r16 or exr) plus manifest with min/max, size and region layout; optional chunk meshes.

### 2.5 Roads and rivers
- **Between settlements:** Delaunay triangulation (`spade`), then a Gabriel or relative-neighborhood graph, a minimum spanning tree for guaranteed connectivity, and a few extra edges for loops.
- **Routing:** A* on an 8- or 16-neighbor cost grid. Cost uses a hiking-speed function of slope (Tobler), penalties for water and cliffs, and a discount on existing roads.
- **Smoothing:** simplify, then fit a spline; enforce a minimum turning radius.
- **Earthworks:** blend terrain toward the road profile with a smooth falloff (cut and fill), so roads sit on the land.
- **Bridges:** detect road-river crossings, choose span length and abutments from the river width.
- **Inside a settlement:** tensor-field street generation (Chen et al.), or Parish-Müller style rule-based growth, or style-specific patterns (grid, radial, organic) chosen by the style pack.

### 2.6 Districts, blocks and lots
- **Districts:** weighted Voronoi with Lloyd relaxation, or user-drawn polygons.
- **Blocks:** extract faces from the planar road graph using a half-edge (DCEL) structure.
- **Sidewalks and setbacks:** polygon inset (offset) of block outlines.
- **Lots:** recursive subdivision along the longest side of the oriented bounding box with a seeded split ratio, until each lot is within the area range. Require road frontage; handle leftovers by merging or marking as courtyards or open space.

### 2.7 Buildings
- **Shape grammars:** split and repeat rules in the style of CGA shape grammar, driven by the style pack.
- **Footprint:** lot inset, then style rules (L-shape, courtyard ring via polygon difference, and so on).
- **Facade layout:** fit repeating bays to the wall length (formula in Part 3), then place windows and doors by rules.
- **Roofs:** flat with parapet (offset), gable (ridge along the longer axis), hip (straight skeleton), domes and chhatris (surfaces of revolution and kit parts).
- **Walls from a 2D plan:** offset wall centerlines by half the thickness, union the results, subtract door and window openings in 2D, then extrude. This avoids fragile 3D booleans.
- **Parts and UVs:** parts carry world-scale UVs (meters) so wallpaper and textures tile at real size with no unwrapping.

### 2.8 Interiors
- **Layout:** recursive BSP or squarified treemaps on the footprint's bounding rectangle, with target areas per room type, followed by validation.
- **Scoring and repair:** simulated annealing or local search on a cost function (area error, adjacency violations, light access for habitable rooms, washroom-kitchen plumbing adjacency, aspect ratios).
- **Doors:** minimum spanning tree over the room adjacency graph, with the entrance on the street frontage.
- **Stairs and floors:** place stairs in a reserved core that stacks across floors.
- **Rules engine:** essentials required, luxuries optional, each with a placement constraint.
- **Harder alternative later:** mixed-integer optimization for layouts (see reading list).

### 2.9 Props and vegetation
- **Scatter:** Bridson Poisson-disk sampling for even, natural spacing, masked by density maps and slope.
- **Plants:** noise-driven placement, small L-system or parametric templates for trees, species by biome.
- **Output:** transforms only, grouped by chunk for instancing.

### 2.10 Naming and signage
- **Names:** order-2 Markov chains or syllable grammars per culture, uniqueness via a seeded hash set, with a blocklist.
- **Signs:** shape text with `rustybuzz` (a HarfBuzz port) for Devanagari and Arabic/Urdu, render to MSDF atlases or baked textures, and mount on a small sign part from the kit.

### 2.11 Meshes, LODs, colliders and navigation
- **Part LODs by construction;** merged LOD1 per building; LOD2 as a box or impostor.
- **Simplification:** quadric error metrics (Garland-Heckbert) through meshoptimizer for anything not authored with a LOD.
- **Chunk merging and instancing:** group per chunk so MultiMesh AABBs stay tight.
- **Colliders:** footprint polygon triangulated, merged into convex pieces (Hertel-Mehlhorn), extruded to height. Terrain uses a heightfield collider.
- **Navigation data:** export walkable polygons for the engine to bake, or a pre-baked option.
- **Budget estimators:** triangle and draw-call predictions per building and per chunk, checked at validation.
- **Destructible profile (aspirational):** separable parts with structural tags; 2D Voronoi fracture on wall planes for pre-fractured pieces.

### 2.12 Packs and rules format
- **Declarative and versioned:** data files describing lots, grammars, parts, palettes, names and interiors. A tiny expression language for formulas, run in a sandbox with no file or network access.
- **Validation:** schema check plus a "pack self-test" that generates sample buildings and verifies budgets.
- **Signing:** Ed25519 signatures for official packs and license files.

### 2.13 Export
- **Core format:** glTF 2.0 with `extras` metadata for semantics.
- **Manifest:** JSON listing chunks, buildings, parts, instance groups, hashes and coordinate origins, written atomically after the files.
- **Godot plugin:** reads the manifest, builds MultiMeshes with custom AABBs, hooks collision and navigation, reloads only changed entries.
- **Blender addon:** builds native objects from the same data, with shared mesh data for instances.

---

## Part 3: Formula Sheet

**3.1 Seed derivation.** `seed_child = H(seed_parent, kind, key)` with `H` a stable 64-bit hash (BLAKE3 truncated). A counter-based stream is `rng(seed_entity, counter = i)`. Nothing depends on iteration order.

**3.2 Identity matching.** `IoU(A, B) = area(A ∩ B) / area(A ∪ B)`. Match when IoU > 0.5 (tunable), resolving conflicts by best score.

**3.3 Terrain feel.** fractal Brownian motion:
`fBm(p) = ( Σ_{i=0}^{N-1} g^i · n(f^i · p) ) / ( Σ_{i=0}^{N-1} g^i )` with lacunarity `f ≈ 2`, gain `g ≈ 0.5`.
Ridged: `r(p) = (1 − |n(p)|)^2`. Warped: `h(p + w · n2(p))`.
Starting presets (to tune by eye):

| Feel | Octaves | Gain | Relief amplitude | Ridged mix |
|---|---|---|---|---|
| Bumpy | 3-4 | 0.45 | small | none |
| Hilly | 4-6 | 0.50 | medium | low |
| Mountainous | 6-8 | 0.55 | large | high, masked |

**3.4 Land profile from signed distance** (`d > 0` inland, in meters):
`base(d) = −D_shelf · (1 − smoothstep(0, W_shelf, −d))` for `d < 0`
`base(d) = H_coast + (H_inland − H_coast) · smoothstep(0, W_inland, d)` for `d ≥ 0`
with `smoothstep(a, b, x) = t²(3 − 2t)`, `t = clamp((x − a)/(b − a), 0, 1)`.

**3.5 Slope and hillshade.** Central difference: `dz/dx = (h[x+1] − h[x−1]) / (2Δ)`. Slope `s = atan(sqrt(zx² + zy²))`. Hillshade: `shade = cos(Z)·cos(s) + sin(Z)·sin(s)·cos(Az − aspect)`, clamped to 0..1, with sun zenith `Z` and azimuth `Az`.

**3.6 Height quantization.** `step = (h_max − h_min) / 65535`. Example: 2,000 m of relief gives about 3 cm steps.

**3.7 Hiking cost (Tobler).** Speed `W = 6 · exp(−3.5 · |S + 0.05|)` km/h, where `S` is slope as rise over run. Segment cost = length / `W`, plus water and cliff penalties.

**3.8 Parameter buckets for the part cache.** `bucket(x) = round(x / q) · q` with quantum `q`; part key = hash(part type, bucketed params). Track hit rate.

**3.9 Facade fitting.** For wall length `L` and target bay pitch `p`: `n = max(1, round(L / p))`, bay width `b = L / n`, window width `w = k · b` with `k` in a style range (for example 0.35 to 0.6).

**3.10 Lot subdivision.** While `area > A_max` or frontage `> F_max`: cut perpendicular to the longest OBB side at ratio `r ~ U(0.4, 0.6)` (seeded). Stop when `A_min ≤ area ≤ A_max`.

**3.11 Poisson-disk (Bridson).** 2D grid cell size `r / √2`; up to `k ≈ 30` candidates per active point in the annulus `[r, 2r]`.

**3.12 Wizard capacity check.** Buildable area `A_b = A_total · (1 − road share) · (1 − open space share)`. Expected buildings `N ≈ A_b · coverage / mean footprint`. Map the density slider to coverage and lot size, then clamp or warn if `N` falls outside the user's range.

**3.13 Triangle budget.** `T_scene ≈ Σ_chunks ( Σ_buildings T_LOD(distance) + T_terrain + T_props )`. Draw calls ≈ materials × chunks + instance groups.

**3.14 Interior cost** (annealing): `E = w1·Σ(area_err²) + w2·(adjacency violations) + w3·(rooms without light) + w4·(plumbing separation) + w5·Σ aspect penalty`; accept worse moves with probability `exp(−ΔE / T)` and cool `T` over iterations.

---

## Part 4: Prototype Spikes (Direction)

Each spike has a pass/fail test. They feed the phases in the project plan.

| Spike | Goal | Pass criterion |
|---|---|---|
| S1 Determinism harness | Seeded RNG, hashing, integer geometry | Identical output hash on macOS, Windows and Linux CI |
| S2 Bindings gate | One Rust function called from Blender and from Godot | Both print the same result |
| S3 Lasso to terrain | Blob, signed distance, base height plus noise, 16-bit tiles | Tiles import into Godot (and Terrain3D) and Blender with correct scale |
| S4 Seams | Regenerate one region at higher detail | No visible or numeric seam at borders |
| S5 Hydrology | Priority-flood, flow accumulation, rivers | Rivers always flow downhill, lakes sit in depressions |
| S6 Road routing | A* with slope cost between two settlements | Route avoids steep slopes and water, smoothing respects turning radius |
| S7 Blocks and lots | Road graph to faces to lots | Every lot has frontage, no overlaps, all areas in range |
| S8 One house | Footprint, walls with openings, roof, LOD0 to LOD2 | Within triangle budget, correct UV scale, colliders valid |
| S9 Interior | BSP layout, doors, essentials rule | All essentials present, all rooms reachable |
| S10 Rebuild engine | Change one parameter | Only expected IDs and files change, and it matches a full rebuild |
| S11 Pack format | Load a minimal Indian style pack from data | Same house generated from the pack as from hardcoded rules |
| S12 Sign text | Shape and render a Devanagari and an Arabic sign | Correct glyph shaping and direction in both engines |

**Suggested order:** S1, S2, S3, S4, S10, S5, S6, S7, S8, S9, S11, S12. This front-loads the risks that would force a redesign (determinism, bindings, seams, rebuild).

---

## Part 5: Reading List (verify details before citing)

- Parish and Müller, "Procedural Modeling of Cities" (SIGGRAPH 2001).
- Müller, Wonka, Haegler, Ulmer, Van Gool, "Procedural Modeling of Buildings" (SIGGRAPH 2006), the CGA shape grammar.
- Chen, Esch, Wonka, Müller, Zhang, "Interactive Procedural Street Modeling" (SIGGRAPH 2008), tensor fields.
- Vanegas et al., work on procedural parcel generation (Computer Graphics Forum, 2012).
- Merrell, Schkufza, Koltun, "Computer-Generated Residential Building Layouts" (SIGGRAPH Asia 2010).
- Wu, Fan, Liu, Wonka, "MIQP-based Layout Design for Building Interiors" (Eurographics 2018).
- Bridson, "Fast Poisson Disk Sampling in Arbitrary Dimensions" (2007).
- Musgrave, "Procedural Fractal Terrains," in *Texturing and Modeling*.
- Génevaux et al., terrain generation using hydrology-based procedural models (SIGGRAPH 2013).
- Mei, Decaudin, Hu, "Fast Hydraulic Erosion Simulation and Visualization on GPU" (2007).
- Barnes, Lehman, Mulla, "Priority-Flood: An Optimal Depression-Filling and Watershed-Labeling Algorithm for Digital Elevation Models" (2014).
- Felzenszwalb and Huttenlocher, "Distance Transforms of Sampled Functions" (2012).
- Garland and Heckbert, "Surface Simplification Using Quadric Error Metrics" (1997).
- Shewchuk, "Adaptive Precision Floating-Point Arithmetic and Fast Robust Geometric Predicates" (1997).
- Salmon et al., "Parallel Random Numbers: As Easy as 1, 2, 3" (SC 2011), counter-based RNGs.
- Chlumský, multi-channel signed distance field font rendering (MSDF).
- Aichholzer et al., straight skeletons (1995), and Felkel and Obdržálek's implementation notes (1998).
- Gumin, Wave Function Collapse (2016), as an optional tool for props and interior details.

---

## Part 6: Open Research Questions

1. Does Godot import EXT_mesh_gpu_instancing in the target 4.7 line? (Decides whether glTF alone can carry instances.)
2. Does Blender's extensions platform accept abi3-tagged native wheels, and is publishing to PyPI acceptable for the core?
3. What is the right size of the eroded base map (resolution and extent) before detail noise takes over?
4. Baked sign textures versus engine-side text: which handles Devanagari and Arabic reliably in both Blender and Godot?
5. How much destructibility ("explodable") is needed at launch, and for which building types?
6. Which Indian flavor has enough accessible reference material to define good rules?
7. What is the minimum viable rules language for packs, and what can be left to code for v1?

## Sources Checked (2026-10-23)

- Godot release blog and release listings: godotengine.org/blog/release
- Godot MultiMesh documentation: docs.godotengine.org (MultiMesh class)
- Blender glTF 2.0 add-on manual: docs.blender.org (scene_gltf2)
- Blender extensions, Python wheels: docs.blender.org (advanced/extensions/python_wheels)
- godot-rust crate documentation: docs.rs/godot
- Terrain3D import and export documentation: terrain3d.readthedocs.io
- iOverlay documentation: docs.rs/crate/i_overlay
