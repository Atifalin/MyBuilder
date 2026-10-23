# My Builder: Project Plan

A coherent plan built from your idea document, your replies, the ChatGPT generation hierarchy you shared, and the gaps we discussed.

**Legend:** **[Yours]** is something you decided or described. **[Proposed]** is something I filled in to close a gap. Review every Proposed item before you commit to it.

---

## 1. Vision and Scope [Yours]

A suite of procedural, rule-based builders that lets vibe coders and young students create a populated, lived-in game world without building everything by hand. No AI is needed in the generation: users dictate the rules, and the result is mathematical and logical.

- **In scope:** terrain and world, roads, villages, towns and cities, houses with interiors, districts with names and signboards, style packs, placeholders for hero assets, export to engines.
- **Out of scope:** city-management gameplay, characters, hero buildings (placeholders only), AutoCAD-style precision modeling.
- **No built-in 3D viewport** [Yours]: the apps generate and export. Users see results in Blender or Godot, whichever pipeline they use.
- **Audience:** hobbyists, students, weekend game makers. Not large productions.
- **First targets:** Godot first, Indian-style buildings first. More styles and engines come once the product is popular and earning.

## 2. Guiding Principles

1. **Rules, not CAD** [Yours]. Defaults first, manual control optional.
2. **Everything is code-generated** [Yours]. Plants use noise. Nothing is hand-modeled.
3. **Three ways to control** [Yours]: wizard, settings panel and sliders, or node graph / drag-and-drop rules. Users may skip any step and place things manually.
4. **One system underneath** [Proposed]. The wizard, sliders and nodes all edit the same recipe format, so they never drift apart.
5. **Modular by default** [Yours + Proposed]. Every builder, style pack and engine export is a separate module. Users download only what they need.
6. **Performance by construction** [Yours + Proposed]. Generation is designed so that it never produces heavy, unique-per-object assets (see section 7).
7. **Deterministic** [Proposed]. Same recipe plus same seed gives the same result, always.
8. **Build and rebuild** [Yours]. Each app has Build and Rebuild buttons instead of a viewport. Rebuild only regenerates what changed, unless the user clears everything and changes the full seed (see section 6).

---

## 3. Architecture [Proposed]

Three layers. Dependencies point inward only. The core never contains engine code.

```
 ADAPTERS (thin, replaceable)
 ├─ Standalone control app (Mac first, Windows/Linux later; 2D map canvas, wizard, rules, Build/Rebuild, no 3D viewport)
 ├─ Godot plugin
 ├─ Blender addon
 ├─ CLI
 └─ MCP server (wraps the CLI/API)
        │
 BUILDER MODULES (each one is optional)
 ├─ World Builder   ├─ House Builder
 ├─ Village/City Builder   └─ later: NPC, Props, ...
        │
 CORE (engine-agnostic, no UI)
 ├─ Schema + project files   ├─ Pipeline runner + cache
 ├─ Geometry + math + noise  ├─ Rules engine
 ├─ Style pack loader        ├─ Validation + budgets
 └─ Mesh/data exporters (glTF, data layers)
```

- The core outputs a **neutral scene description** plus mesh data. Adapters translate it for each tool.
- The same core runs headless, so the CLI and MCP server come almost free.

## 4. Tech Stack Recommendation [Proposed, decision pending]

You asked for code most compatible with Godot and Blender, with optimization in mind.

**Rust core confirmed [Yours], with thin bindings on top.**

| Piece | How |
|---|---|
| Core engine | Rust library (fast, safe, no runtime) |
| Godot | GDExtension via godot-rust (plugin), GDScript only for thin glue |
| Blender | Python module built with PyO3/maturin, wrapped in an addon |
| CLI / MCP | Rust binary exposing the same API |
| Standalone app | Tauri (Rust backend + web UI), calling the core directly |

**Why Tauri for the standalone app** [Proposed, changed from my earlier draft]: I first suggested building the app in Godot for its 3D viewport. Since the apps have no viewport and rendering happens in Blender or Godot, that advantage is gone. The app is a control panel: a 2D map canvas, wizard, settings, rules and node editor, and Build/Rebuild buttons. Tauri keeps the app in Rust so it calls the core directly, builds for Mac, Windows and Linux, and has mature web libraries for canvas drawing and node graphs. Alternative: a pure-Rust UI toolkit such as egui, which is simpler but weaker for a node editor.

**Costs to accept:** Rust has a learning curve (AI help makes it workable), and you need per-OS builds for each binding.

**Fallback if the spike fails:** a Python-first core (NumPy) that runs inside Blender and as a CLI, with the Godot-based app talking to it over a local process. Simpler to write, slower at scale. Keep the data format language-neutral so the core can be rewritten later without breaking projects.

**Decision gate (Phase 0):** prove that one tiny Rust function can be called from both Blender and Godot before building anything else.

## 5. Data Model and Project Files [Proposed]

**Semantic hierarchy** (this answers the ChatGPT point that the system must understand how pieces fit together):

`World → Region → Settlement → District → Block → Lot → Building → Floor → Room → Fixture / Prop`

Roads, rivers and terrain layers live at the World and Region level, and settlements inherit them.

**Project folder:**

```
my-project/
  project.json        (schema version, seed, world scale, units)
  recipes/            (world, city, house recipes as JSON)
  layers/             (height, water, forest, biome masks, road/river vectors)
  overrides/          (user edits, keyed by stable IDs)
  cache/              (stage hashes and intermediate results for incremental rebuild)
  styles/             (style packs in use)
  previews/           (jpg map previews)
  exports/
```

- **Layers, not just a jpg.** The jpg is only a preview. Height, water, forest, biome and roads are stored as data layers.
- **Stable IDs** derived from seed plus hierarchy path, so a building keeps its identity when other things change.
- **Overrides layer.** Manual edits sit on top of generated results. Regenerating keeps locked edits. This answers the "what happens to my edits" gap inside the app.
- **Versioned schema** so old projects keep opening.
- **Fixed world scale** (one map unit equals a set number of meters), chosen once, so terrain regions match at their edges.

## 6. Generation Pipeline

Merges your world builder flow with the hierarchy you shared. Each stage is a pure function (inputs + params + seed in, outputs out) so stages can be cached and re-run alone.

**World stage** [Yours, details proposed]
1. 2D map canvas: continent lasso, water-first or land-first, hills, mountains, forests, roads. Feel questions (bumpy, hilly, mountainous) become noise parameters.
2. Save coordinates, layers and a preview jpg.
3. Generate: land plane, land/water marking, mountains, forest zones, then texture painting from the layers.
4. Export model and data.

**Settlement stage**
1. **Settings:** scale, style, density, building count range.
2. **Terrain sampling:** load only the selected region, re-sample the height layer at higher detail, or use the pre-baked terrain.
3. **Road network:** roads, intersections, connectivity, routed around steep slopes. Long roads connect settlements.
4. **Districts and building areas:** boundaries, names, styles, density. Names and signboards are generated by math (syllable rules per style) unless customized.
5. **Lots:** blocks subdivided into building lots.
6. **Buildings:** footprint, massing, facade, roof, then interiors and props.
7. **Validation and export:** geometry checks, budgets, colliders, LODs, engine target.

**No-world mode** [Yours]: if the user only wants a city, the same pipeline runs with a small default random world they can tweak. There is no separate code path.

**Flexible entry** [Yours, per the reference]: users can skip stages, draw roads by hand, place single buildings, or let the wizard do most of it.

**Build and Rebuild** [Yours, mechanics proposed]
- The app generates data and files. The user views results in Blender or Godot.
- **Build** runs the pipeline for the first time.
- **Rebuild is incremental by default.** The app tracks which settings changed and which stages and objects depend on them (a dependency graph with stage hashes). Only those are cleared and regenerated. Everything else is left untouched.
- Example: changing the density of one district rebuilds only that district's lots and buildings. Terrain and roads stay as they are.
- **Clear all and rebuild** is a separate action for starting fresh, and it can set a new full seed. It asks for confirmation, and asks whether to keep manual overrides.
- Each rebuild writes only the files that changed, so the engine only reimports those (see section 11).

---

## 7. Performance Strategy [Proposed]

The core rule: **variety comes from parameters, not from unique models.**

1. **Kit of parts.** Buildings are assembled from parametric parts (wall segments, windows, doors, arches, balconies, roof pieces). Parts are generated by code, but each part is small and reusable.
2. **Content-addressed part cache.** Parts are keyed by a hash of their parameters. Identical parameters reuse one mesh. Quantize parameters into a small set of buckets (for example, a handful of window widths), so you get variety without a mesh explosion.
3. **Instancing and merging.** Repeated parts export as instances (MultiMesh in Godot). Static geometry is merged per chunk to keep draw calls low.
4. **Cheap variation.** Use vertex colors and per-instance shader parameters for color and wear instead of unique textures.
5. **Trim sheets and atlases.** Few materials, shared across all buildings in a style. User textures (such as wallpaper) apply to material slots with tiling based on real-world meters.
6. **UVs by construction.** Parts carry UVs from the start, so no unwrapping happens at export.
7. **LODs by construction.** LOD0 is the detailed assembly, LOD1 is merged and simplified, LOD2 is a box or impostor. Generated during building, not by an after-the-fact reducer.
8. **Lazy interiors.** Seeds make interiors reproducible, so generate them only when needed (when the player is near, or at export if requested).
9. **Simple colliders.** Footprint boxes or convex shapes, never the render mesh.
10. **Chunked terrain.** Height tiles with LOD, per-chunk origins to avoid large-coordinate precision problems in Godot.
11. **Budgets and gates.** Each building type has starting triangle and draw-call targets (to be tuned in Phase 3). Validation warns or fails the export when a scene exceeds them.

**Two export modes:**
- **Baked** (v1): glTF scenes with instanced parts plus data. Works everywhere.
- **Runtime** (later): ship the recipe and seed and generate in-game. Smaller downloads, more engineering.

## 8. Module Layout [Proposed]

Suggested package structure (each can be built and tested alone):

- `schema`: types, versioning, validation of recipes and projects
- `geom`: polygons, splines, meshes, boolean-free helpers, noise
- `pipeline`: stage runner, dependency graph, dirty tracking, caching, seeds, incremental rebuild
- `terrain`: heightfield, erosion-lite, water, biomes, chunking
- `network`: roads, rivers, pathfinding with slope cost
- `lots`: districts, blocks, lot subdivision
- `architecture`: footprints, facades, roofs, part kit
- `interiors`: room layout, essentials/luxury rules, doors/windows alignment
- `naming`: name and signboard generator per style and script
- `styles`: style pack loader and definitions
- `validate`: geometry checks, budgets, navigability
- `export-gltf`, `export-data`, `export-godot`, `export-blender`, later `export-unity`
- Adapters: `app` (Tauri control app), `plugin-godot`, `addon-blender`, `cli`, `mcp`

## 9. The Builders

### World Builder (first)
Flow in section 6. Output: height, water, forest and biome layers, textured terrain chunks, a preview jpg, and a data export.

### House Builder
- Rules: a house must have the essentials (bed, washroom, basics), then the user adds luxuries (AC, TV, washing machine tower). At least one unique luxury makes buildings explorable and explodable.
- Walls take width, thickness and length with defaults, or the user draws them Sims-style.
- Room layout uses adjacency rules; doors and windows must line up with the facade; stairs and plumbing walls are constraints.
- Houses start with few polygons; users can raise the complexity.

### Village, Town and City Builders
- Quick wizard: type, building count range, style drop-down, size, dense/sparse slider.
- Lay roads, select areas, choose building styles and rules, then watch it populate. Inspect all or one building and customize.
- Optional service placements (police station and similar) with area of effect, for logic only.
- Districts create road names and signboards. Many cities per world are supported.

## 10. Style Packs [Proposed structure]

A style pack is data plus a part kit, separate from the code, so it can be sold or shared later.

- Rules for lot shapes, massing, facades, roofs, window and door patterns, signage, color palettes.
- Parametric part definitions.
- Naming rules and script support. Indian-style signboards may need Devanagari, Urdu or other scripts, so check font and text-shaping support early.
- Customizable in the app.

**Indian style v1:** pick one flavor and make it excellent instead of covering all of India. Open question: Mughal-era old city, modern Indian town, or village? Candidate part kit (to confirm): courtyards, arched openings, jharokha-style balconies, jaali screens, domed or chhatri accents, flat roofs.

## 11. Export and Integration [Yours + Proposed]

- **Output is many small files keyed by stable ID** (per terrain chunk, per building or building group) plus a manifest. A rebuild rewrites only the changed files and updates the manifest. The Godot plugin and Blender addon watch the manifest (or offer a Sync button) and reload only what changed.
- **Godot first:** glTF with metadata, instanced parts, simple colliders, LODs, optional navmesh checkbox, plus a small plugin for importing the data.
- **Blender:** the addon runs the core and builds objects natively.
- **MCP / CLI:** tools such as create project, generate world, edit layer, generate settlement, export. An AI only edits recipes, so results stay deterministic.
- **Live connection** to Godot or Blender comes after the file-based export works.
- **One-way export first.** Regenerate in the app, not in the engine. Round-tripping hand edits made in the engine is out of scope for v1.
- Per-engine export packs are optional downloads. A models-only export always exists.

---

## 12. Roadmap with Exit Criteria [Proposed]

You said time is not the issue, so phases are defined by what must work, not by dates.

**Phase 0: Decisions and spike**
- Stack gate: one Rust function callable from Blender and from Godot.
- Draft the schema and project folder.
- Spike: a drawn blob becomes a heightmap, then a chunked mesh imported in Godot.
- *Exit:* stack chosen, spike runs end to end.

**Phase 1: Foundation ("My Builder" core)**
- Schema, project files, seeded RNG, pipeline runner with caching, overrides layer, glTF exporter, CLI.
- Godot plugin scaffold and Tauri app shell with Build and Rebuild buttons.
- Golden tests: same seed gives identical output hash.
- *Exit:* a recipe runs through the CLI and opens correctly in Godot, and changing one parameter rebuilds and rewrites only the affected files.

**Phase 2: World Builder v1**
- 2D canvas and tools, feel questions, generation, texture painting, chunked export, jpg preview.
- *Exit:* draw a map, generate it, and walk around it in Godot at a steady frame rate.

**Phase 3: House Builder v1**
- Part kit, one Indian-style flavor, room layout with essentials and luxuries, wallpaper and texture upload, LODs, colliders, budgets.
- *Exit:* generate hundreds of unique houses within the performance budget.

**Phase 4: Village Builder**
- Roads and lots on real terrain, districts, naming and signboards, service placement, region loading from a world.
- *Exit:* a village on a chosen world region, seamless with the terrain edges.

**Phase 5: Town/City scale, MCP, Blender**
- Performance pass, random-land mode, MCP server, Blender addon, navmesh option, polish.
- *Exit:* private beta with real users.

**Phase 6+: Growth**
- More style packs (paid or shared), Unity export, NPC behaviour builder, props builder, runtime generation mode.

## 13. How to Start (First Steps) [Proposed]

1. Create the repo with a monorepo layout matching section 8, but only `schema`, `geom`, `pipeline` and `cli` at first.
2. Write the project and recipe schema for the world only. Keep it small and versioned.
3. Run the Phase 0 spike: Blender and Godot calling the same function.
4. Build the heightfield pipeline: blob, noise, 16-bit height layer, chunked mesh, glTF.
5. Import into Godot and set up the golden-test habit from day one.
6. Only then start the 2D canvas UI.

Rule for every module: define its inputs and outputs and write a test before building on it.

## 14. Testing and Quality [Proposed]

- Golden tests (same seed, same hash) for every stage.
- Property tests for geometry (no overlaps, buildings stay within lots, roads connect).
- Budget tests that fail when triangle or draw-call targets are exceeded.
- Seam tests: a region regenerated at higher detail must match the world mesh at its edges.
- Incremental rebuild tests: change one parameter and check that only the expected IDs change, and that the incremental result matches a full rebuild with the same seed.
- Visual checks: render a contact sheet of generated houses and compare it to the previous version.

## 15. Open Decisions

1. **Stack:** Rust core is confirmed. The Phase 0 spike still has to prove the Blender and Godot bindings work.
2. **Indian flavor** for style pack v1.
3. **Standalone app framework:** Tauri (recommended, since there is no 3D viewport) or a pure-Rust UI toolkit.
4. **World scale:** the meters per map unit, and the maximum world size.
5. **Business model:** which parts are free and which are paid (for example, core free, style packs paid), and the core's license.
6. **Product name** and whether "My Builder" is available.
7. **Which Godot 4 version** to target as the baseline.

## 16. Risks

- **Scope creep** across builders. Mitigation: strict phases and exit criteria.
- **Procedural look feels generic.** Mitigation: style packs get design attention and strong defaults.
- **Cross-platform bindings are fiddly.** Mitigation: the Phase 0 gate and CI builds early.
- **Seams and precision** in large worlds. Mitigation: fixed scale, chunk origins and seam tests from the start.
- **Interior layout complexity.** Mitigation: start with a few room templates and grow the rules.
- **Incremental rebuild goes stale or inconsistent.** Mitigation: an explicit dependency graph, stage hashes, and tests that compare incremental and full rebuilds.
