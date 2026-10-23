# My Builder: Goals, Modules and Packs

An aspirational but practical list of what the product must achieve, and the modules and packs that could turn it into something people pay for.

**Tags:** **[Yours]** came from you. **[Proposed]** is my suggestion. Priority tiers: **Launch** (v1), **Next** (soon after), **Later**, **Aspirational**.

---

## 1. Must-Achieve Goals (Practical)

These decide whether v1 is a real product or a tech demo. Numbers are starting targets to tune, not promises.

### Experience
1. **Fast first result** [Proposed]. From install to a generated world open in Godot in about five minutes, using only the wizard.
2. **Beginner-friendly** [Yours]. A user who knows nothing about procedural generation can finish the wizard and get a good-looking result with defaults.
3. **Control when wanted** [Yours]. Every wizard choice can be changed later in settings, rules or nodes, and any step can be skipped or done by hand.
4. **Reproducible** [Proposed]. Same recipe plus same seed always gives the same output.
5. **Rebuild feels instant for small changes** [Yours, target proposed]. A small change rebuilds only what depends on it, in seconds, not minutes.

### Output quality
6. **Game-ready, not just pretty** [Yours]. Exports include simple colliders, LODs, UVs and an optional navmesh.
7. **Runs well on an average machine** [Yours]. Generated worlds must not choke a good system. Variety comes from parameters and instanced parts, never from piles of unique models.
8. **Budgets enforced** [Proposed]. Triangle and draw-call targets per building type, with warnings in the build output when exceeded.
9. **Clean imports** [Proposed]. Godot and Blender open exports with no manual fixing: correct scale, orientation, materials and hierarchy.
10. **Logical worlds** [Yours]. Rivers flow downhill, roads avoid steep slopes, buildings sit within lots, every house has its essentials.

### Product health
11. **Local-first** [Proposed]. Works offline with no account needed for core generation.
12. **Stable project files** [Proposed]. Versioned schema with migrations, so users never lose old projects after an update.
13. **Good docs and sample projects** [Proposed]. A quick-start, a short video-style tutorial, and ready-made example worlds.
14. **Modular install** [Yours]. Users download only the builders, style packs and engine exports they need.
15. **Safe to sell** [Proposed]. Because everything is code-generated, packs carry no third-party asset licensing problems. User-supplied textures stay the user's responsibility.

## 2. Aspirational Goals

Worth designing toward, but not promised for v1.

- **Runtime generation** [Proposed]: ship a recipe and seed, and generate the world in-game. Smaller downloads, endless variation.
- **Edit round-tripping** [Proposed]: hand edits made in the engine survive regeneration.
- **Live connection** [Yours]: a live link between the app and Blender, Godot or Unity, so changes appear as they are made.
- **Plugin SDK** [Proposed]: let others write style packs, builders and exporters against a stable API.
- **Community marketplace** [Proposed]: creators sell packs, with a revenue share.
- **Organic city growth** [Proposed]: districts with an age and history, where old centers differ from new suburbs.
- **More engines** [Yours]: Unity, then Unreal and others.
- **Teams** [Proposed]: shared packs and projects for classrooms and small studios.

---

## 3. Modules

Each module is a separately installable unit built on the shared core.

### Core platform
| Module | Purpose | Tier |
|---|---|---|
| **My Builder Core** | Schema, project files, pipeline, rebuild engine, validation, exporters | Launch |
| **Control App** | Wizard, settings, rules and node editor, Build/Rebuild | Launch |
| **CLI** | Headless runs and automation | Launch |
| **Godot Export + Plugin** | Import, manifest sync, navmesh option | Launch |
| **Blender Addon** | Builds natively in Blender | Next |
| **MCP Server** | Lets AI tools drive recipes deterministically | Next |
| **Unity Export** | Second engine | Later |
| **Unreal Export** | Third engine | Aspirational |

### Builders
| Module | Purpose | Tier |
|---|---|---|
| **World Builder** [Yours] | 2D map drawing, terrain generation, textures, region export | Launch |
| **Terrain Manager** [Yours] | Edit and tune terrain layers, chunking, biomes | Launch |
| **House Builder** [Yours] | Exterior and interior with essentials and luxuries | Launch |
| **Village Builder** [Yours] | Small settlements on real terrain | Next |
| **Town and City Builder** [Yours] | Districts, roads, services, naming | Next |
| **Roads and Rivers** [Yours] | Long roads between settlements, rivers, bridges | Next |
| **Props Builder** [Yours] | Street furniture, market stalls, clutter | Next |
| **Vegetation Builder** [Yours] | Plants from noise, forests, farmland | Next |
| **NPC Behaviour Builder** [Yours] | Daily routines and waypoints tied to buildings | Later |
| **Naming and Signage** [Yours] | Math-generated names and signboards per style and script | Next (inside city builder) |
| **Interior Sets Builder** [Proposed] | Shops, temples, schools, clinics, not just homes | Later |
| **Bridges and Tunnels** [Proposed] | Crossing rivers and mountains | Later |
| **Fortifications** [Proposed] | City walls, gates, forts | Later |
| **Ports and Harbors** [Proposed] | Docks, piers, coastal towns | Later |
| **Ruins and Aging** [Proposed] | Weathering, abandoned and damaged variants | Later |
| **Cave and Dungeon Builder** [Proposed] | Underground spaces | Aspirational |
| **Points of Interest** [Proposed] | Marker slots for quests, landmarks and hero-building placeholders | Later |
| **Time and Weather Presets** [Proposed] | Lighting and sky settings exported per engine | Aspirational |

---

## 4. Packs

Packs are data, not code, so they can be sold, shared and updated separately.

### Style packs (architecture)
| Pack | Notes | Tier |
|---|---|---|
| **Indian: first flavor** [Yours] | Flavor still to choose (Mughal-era old city, modern town or village) | Launch |
| **Indian regional set** [Proposed] | Rajasthani haveli town, Kerala coastal, Himalayan hill village, colonial-era Indo-Saracenic, modern Indian town | Next to Later |
| **Arabic / Islamic city** [Yours] | Courtyards, domes, souks | Next |
| **Victorian** [Yours] | Terraces, brick, gaslit streets | Next |
| **Utopian cyberpunk** [Yours] | Neon towers, dense vertical districts | Next |
| **Medieval European** [Proposed] | Timber frame, stone, walled towns | Later |
| **East Asian** [Proposed] | Japanese and Chinese town styles | Later |
| **Mediterranean** [Proposed] | Whitewashed hill towns, terracotta roofs | Later |
| **Sahel / Desert mud-brick** [Proposed] | Adobe and mud-brick towns | Later |
| **Latin American colonial** [Proposed] | Plazas, courtyards, pastel facades | Later |
| **Nordic** [Proposed] | Wooden villages, turf roofs | Later |
| **Southeast Asian stilt village** [Proposed] | Waterfront and stilt houses | Later |
| **Wild West** [Proposed] | Frontier town | Later |
| **Modern suburban / urban** [Proposed] | Contemporary streets and blocks | Later |
| **Sci-fi colony / post-apocalyptic / fantasy** [Proposed] | Genre-driven styles | Aspirational |

### Environment packs
- **Biome packs** [Proposed]: desert, tundra, tropical, savanna, monsoon, Mediterranean, temperate. Each defines terrain feel, vegetation, color palette and texture rules. (Next)
- **Vegetation packs** [Proposed]: noise-generated tree, shrub and grass families per biome. (Next)
- **Water and coast packs** [Proposed]: river, lake and shoreline styles. (Later)

### Interior and prop packs
- **Home essentials and luxuries** [Yours]: bed, washroom basics, then AC, TV, washing machine tower and other luxuries, in each style. (Launch)
- **Furniture sets per style** [Proposed]. (Next)
- **Shop and market packs** [Proposed]: stalls, counters, shelves. (Next)
- **Street furniture** [Proposed]: lamps, benches, signposts, wells. (Next)

### Language and naming packs
- **Script and font packs** [Proposed]: Latin, Devanagari, Arabic/Urdu, Chinese, Japanese and others for signboards. (Launch for the first style's script, then Next)
- **Name packs** [Proposed]: syllable and pattern rules for place, street and shop names per culture. (Launch with first style)

### Material packs
- **Procedural material sets** [Proposed]: default wall, roof and ground materials per style so the app looks good with no uploads. (Launch)
- **User texture import** [Yours]: bring your own packs or photos, such as wallpaper from your own house. (Launch)

### Starter and example packs
- **Example worlds and cities** [Proposed]: ready-made projects that show what is possible. (Launch)

---

## 5. What Makes a Pack Sellable [Proposed]

A pack is ready to ship only when it passes this checklist:

1. Rules for lots, massing, facades, roofs, windows and doors.
2. A parametric part kit with UVs and LODs.
3. A palette and procedural materials.
4. Interior sets with essentials and at least one unique luxury.
5. Naming rules and signage in the right script.
6. Passes the performance budgets and validation checks.
7. Includes a sample project and short docs.
8. Looks distinct from other packs at a glance.

## 6. Packaging Options [Proposed]

Business decisions are still open. Possible shape:

- **Free tier:** core, World Builder, one starter style, Godot export. Enough to try the product and share results.
- **Paid style packs:** sold individually or as bundles (regional bundle, genre bundle).
- **Builder add-ons:** Village, City and later builders as optional modules.
- **Engine exports:** Godot included, others as add-ons.
- **Pro / Studio:** MCP access, plugin SDK, team features, commercial license.
- **Marketplace (later):** community creators sell packs, with a revenue share.

## 7. Launch Scope vs Later [Proposed]

**v1 (Launch):** Core, Control App, CLI, World Builder, Terrain Manager, House Builder, first Indian style, home essentials and luxuries, procedural materials, script and name pack for the first style, Godot export, example projects.

**v1.x (Next):** Village and City builders, roads and rivers, props and vegetation, Blender addon, MCP server, second and third style packs, biome packs.

**v2 (Later):** more styles, Unity export, NPC behaviour builder, interior sets, fortifications, ports, ruins, Pro tier.

**v3 (Aspirational):** runtime generation, round-tripping, plugin SDK, marketplace, Unreal export.

## 8. Success Measures [Proposed]

- Time from install to first world in Godot.
- Share of new users who finish the wizard.
- Number of builds and rebuilds per active project (a sign people keep tinkering).
- Free to paid conversion, and packs bought per paying user.
- Export problems reported per thousand projects.

## 9. Anti-Goals [Proposed]

- No city-management gameplay.
- No hand-modeled asset libraries.
- No requirement for AI services to generate worlds.
- No built-in 3D viewport (Blender or Godot show the results).
- No CAD-style precision tooling.

## 10. Open Questions

1. Which Indian flavor ships first?
2. Is the core free, paid or open source, and what is the license?
3. Which style pack comes second, after Indian?
4. Is Godot export free forever, with other engines paid?
5. Will packs from outside creators be allowed, and under what terms?
