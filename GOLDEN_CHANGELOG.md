# Golden changelog

Every intentional change to a checked-in golden (`crates/*/tests/golden/*.blake3`)
must be logged here. The golden helper enforces it: blessing with `MB_BLESS=1`
requires `MB_BLESS_REASON` and appends a row automatically when a golden's
content changes. A test (`crates/seed/tests/golden_changelog.rs`) checks that
the LAST row for every golden matches the file's content, so rows must stay in
chronological order. Rows written before this file existed are backfilled below
with commit dates and hashes from git history; `(uncommitted)` marks old values
that never reached a commit.

| date (UTC) | generator | golden | old (12 hex) | new (full hex) | reason |
|---|---|---|---|---|---|
| 2026-10-23 | 1 | geom/fbm_bumpy | (new) | 7dcbd8a898cfffa45f73d05b4771a66779edc2e70f4457f537ada01e82b29113 | initial bless (19a99e4) |
| 2026-10-23 | 1 | geom/fbm_hilly | (new) | a0e1a61d934b218b4d8b8ee284901c7a43a765890a2a7733dc3624c0785f8c92 | initial bless (19a99e4) |
| 2026-10-23 | 1 | geom/fbm_mountainous | (new) | f25702f63e355193b60215713c41e5f989b6df382c04cb2c18367d65a5c2358c | initial bless (19a99e4) |
| 2026-10-23 | 1 | geom/ridged_fbm_bumpy | (new) | 9c9d8f09373e6fa273b089be53a515436e8d43cb879b533fd27eb13fe907f4df | initial bless (19a99e4) |
| 2026-10-23 | 1 | geom/ridged_fbm_hilly | (new) | 7a6cd530bf61729b39365323c64a0a873d8b01429d53db328c5ba51c6481daae | initial bless (19a99e4) |
| 2026-10-23 | 1 | geom/ridged_fbm_mountainous | (new) | 7a2751f0659ee5d38b8997fce2613384314df743968964856248c14f4c9fd28d | initial bless (19a99e4) |
| 2026-10-23 | 1 | pipeline/diamond_outputs | (new) | c4bf6f55df55895e63431e8de5dff351c70e43ada54d938e755723567ce1fcd3 | initial bless (19a99e4) |
| 2026-10-23 | 1 | schema/canonical_project | (new) | 35ef84f2312f2539a52f1c76752c848a533e818a1d7f5b2f327709f0d15c5594 | initial bless (19a99e4) |
| 2026-10-23 | 1 | schema/canonical_world | (new) | 250c175afda026153f85ee35b32938e9eb8405a6d54a9d26bfe0a243c91afb3f | initial bless (19a99e4) |
| 2026-10-23 | 1 | seed/derive_paths | (new) | c0a96a1448408f895879dbba86998dfed0c1275b32bd4e121826e04ef48c680f | initial bless (19a99e4) |
| 2026-10-23 | 1 | seed/rng_stream_00c0ffee | (new) | 97f3397c3189157569c87f036c7c2adf64818296e2e907ba684dc5eab0a2b2df | initial bless (19a99e4) |
| 2026-10-23 | 1 | bridge/probe | (new) | 7b70ecba080328a6d00a0eef5449cf8ed77549bad953fc760f56b501d2ff5fe8 | S2 bindings gate probe (0cadaa1) |
| 2026-10-23 | 1 | schema/canonical_world | 250c175afda0 | 82c2c47105fe704ac12c859090e3f0a143ae888f252360c30d4068b15d56800b | S3: add terrain.profile + export.mesh_step_cells fields (5aec7e7) |
| 2026-10-23 | 1 | geom/fbm_bumpy | 7dcbd8a898cf | 74358adb8c196f2a8ca736742b11a0f8f876c9f7ad4840b38abdc9ecb91af1da | lattice artifacts: rotate gradients 22.5deg + per-octave rational rotation/offset (5aec7e7) |
| 2026-10-23 | 1 | geom/fbm_hilly | a0e1a61d934b | e39a724297c0bb36cf1d0e16825410fa414a3f0edd8eafb04c8b5e29f23308e6 | lattice artifacts: rotate gradients 22.5deg + per-octave rational rotation/offset (5aec7e7) |
| 2026-10-23 | 1 | geom/fbm_mountainous | f25702f63e35 | e5d6f0592f947fdb2486abb441b2cc2684daba559be7cba89a58296778d75b66 | lattice artifacts: rotate gradients 22.5deg + per-octave rational rotation/offset (5aec7e7) |
| 2026-10-23 | 1 | geom/ridged_fbm_bumpy | 9c9d8f09373e | bcc38a0fe6fafe1bc7790a12d177aa8788a2000ea2df105732c700a5baa7194a | lattice artifacts: rotate gradients 22.5deg + per-octave rational rotation/offset (5aec7e7) |
| 2026-10-23 | 1 | geom/ridged_fbm_hilly | 7a6cd530bf61 | dc63e2166abba22ecbd8faa947b4433698189d804ede803bf665644215c54ed5 | lattice artifacts: rotate gradients 22.5deg + per-octave rational rotation/offset (5aec7e7) |
| 2026-10-23 | 1 | geom/ridged_fbm_mountainous | 7a2751f0659e | 7ad3c49cd5c706958ac3ea4d31fa2e2def9cbe7bff9d2901823a9a313b4353dc | lattice artifacts: rotate gradients 22.5deg + per-octave rational rotation/offset (5aec7e7) |
| 2026-10-23 | 1 | bridge/probe | 7b70ecba0803 | 2ea5ca993b84f1be29e078663e8f2b0edff151170f71fdd346ade3ffce37f3b7 | lattice artifacts: rotate gradients 22.5deg + per-octave rational rotation/offset (5aec7e7) |
| 2026-10-23 | 1 | terrain/height_r16 | (uncommitted) | 3a46c2b29fc98fe34ae3ce788666173106efc9d522431826222cfd0571c85348 | S3 terrain outputs; first committed value already includes the lattice-noise fix (5aec7e7) |
| 2026-10-23 | 1 | terrain/chunk_0_0_glb | (uncommitted) | 58725985e269574e772a60d301297de4f431eddeb7f41aba31bd670f85b9f476 | S3 terrain outputs; first committed value already includes the lattice-noise fix (5aec7e7) |
| 2026-10-23 | 1 | terrain/manifest | (uncommitted) | ae6cf8b3ac50f455b72b42cb263b789b359deb2171a78e925d154dc529b67cab | S3 terrain outputs; first committed value already includes the lattice-noise fix (5aec7e7) |
| 2026-10-23 | 1 | schema/canonical_project | 35ef84f2312f | dde70d4eb73e82c6fec5da06d078868d5a1c900cd04b3f6b2bf18253d8ad6bed | add generator_version to project.json (pre-release, gen 1) |
| 2026-10-23 | 1 | schema/canonical_world | 82c2c47105fe | 1c233b7481c12a3335f87803ab9010b81bd968bdeb8217f96043dc778e7f3575 | Part B/C: snow_line_m + detail_regions schema fields; manifest gains regions array; new region goldens |
| 2026-10-23 | 1 | terrain/manifest | ae6cf8b3ac50 | dd6449ca9a7293f2cc493d3e40ad6c3dd85c04fb0d14f12fd76d44b3c1b2d2ed | Part B/C: snow_line_m + detail_regions schema fields; manifest gains regions array; new region goldens |
| 2026-10-23 | 1 | terrain/manifest_region | (new) | a9183efa0e709782dcc0ba1722d46185741b88b50f5185aef05dc0afae32467a | Part B/C: snow_line_m + detail_regions schema fields; manifest gains regions array; new region goldens |
| 2026-10-23 | 1 | terrain/region_chunk_1_1_glb | (new) | fc6be58c37899af11f0f8367cd7190e0ed07ac4d82e041086c2d0d1ce4a75a13 | Part B/C: snow_line_m + detail_regions schema fields; manifest gains regions array; new region goldens |
| 2026-10-23 | 1 | terrain/manifest_region | a9183efa0e70 | aa0b5dfe5a75d0acd12b880bd7105ca3ed01620d9db0f7e71fc012d7052c0989 | S4 review: normalize n_true before blending; fix last-quad clamp in coarse_mesh_normal |
| 2026-10-23 | 1 | terrain/region_chunk_1_1_glb | fc6be58c3789 | 56ff342613cddb3d735873c4c6f543ac34cb85e75d746f3a20d2a026d8253293 | S4 review: normalize n_true before blending; fix last-quad clamp in coarse_mesh_normal |
| 2026-10-23 | 1 | geom/chaikin | (new) | 3088edabb49e093e4e286f7a611c0d35d0f221e8a8a9ad6df4f281e57fe0392f | new goldens: shape-tool simplify_dp + chaikin on fixed noisy circle (Phase 2 step 1) |
| 2026-10-23 | 1 | geom/simplify_dp | (new) | 29b1afbe904a6fc8b6b8921cea02c77d062e88b13febbab979988fb54d90e63a | new goldens: shape-tool simplify_dp + chaikin on fixed noisy circle (Phase 2 step 1) |
| 2026-10-23 | 2 | schema/canonical_project | dde70d4eb73e | 85e4426bfa80d26af167bc1269dd6261a09e1282f6c115e2740da1522deed32f | generator_version 2 becomes the default for new projects (texturing) |
| 2026-10-23 | 2 | schema/canonical_world | 1c233b7481c1 | 37bf2c41f19b23fa95fd3f61e2cdf2f308cb01f2ec6a9405d89068588e897ab9 | generator_version 2 becomes the default for new projects (texturing) |
| 2026-10-23 | 2 | terrain/gen2_chunk_glb | (new) | 74f3b533ae95045d35b0bfd408a9be58064fceb4f70169f57f61a3eb3a6eea0f | generator 2: texturing weights, splat maps, TEXCOORD_1 |
| 2026-10-23 | 2 | terrain/gen2_splat_png | (new) | be9baf40aebbbfd27b518644959cdb651029c6abbf919741eb6ba2c3645e254f | generator 2: texturing weights, splat maps, TEXCOORD_1 |
| 2026-10-23 | 2 | terrain/gen2_manifest | (new) | b66bfe1570fd0d5399c6eeab661e2f3c58f69f9862e68db7021f7f5a979bc0e0 | generator 2: texturing weights, splat maps, TEXCOORD_1 |
| 2026-10-23 | 3 | terrain/gen3_height_r16 | (new) | cd2e6ca8857669920f52fe304ba3770075e2e1d51d7105ed843d0108517b0bf4 | generator 3: hydrology (priority-flood, D8, rivers, lakes, carving) |
| 2026-10-23 | 3 | terrain/gen3_river_mask | (new) | 991cb98d04d7ffe38beea13722aaaca01b213daa0d8f15c1082e984a22201048 | generator 3: hydrology (priority-flood, D8, rivers, lakes, carving) |
| 2026-10-23 | 3 | terrain/gen3_chunk_glb | (new) | 74f3b533ae95045d35b0bfd408a9be58064fceb4f70169f57f61a3eb3a6eea0f | generator 3: hydrology (priority-flood, D8, rivers, lakes, carving) |
| 2026-10-23 | 3 | terrain/gen3_manifest | (new) | 99ea7e42bbf41847c9eba0b147bc7db4f718dad8b21f693a0c9f17946183e0ad | generator 3: hydrology (priority-flood, D8, rivers, lakes, carving) |
| 2026-10-23 | 3 | terrain/gen3_height_r16 | cd2e6ca88576 | 9133a4e9467e3d2d9e1f51b357fb6003955116e9f82b50bde306e0a8f3ac3228 | generator 3: carve with exact Euclidean nearest-river distance (was L1 chamfer); fixture region moved over the river |
| 2026-10-23 | 3 | terrain/gen3_manifest | 99ea7e42bbf4 | 7301d81c0596b689770ad2c246078497c5159bbbf2ff015394abc905bde7bbbf | generator 3: carve with exact Euclidean nearest-river distance (was L1 chamfer); fixture region moved over the river |
| 2026-10-23 | 3 | schema/canonical_project | 85e4426bfa80 | 5593599747a9354a9e1efec86407f35bdf40d3e9dbaa47bb853b677ff9924b17 | generator_version 3 becomes the default for new projects |
| 2026-10-23 | 3 | schema/canonical_world | 37bf2c41f19b | 5e3e505d17fc34cd16387fe4649bb7b425b302fb220d2faf5332d3aa61ae6955 | generator_version 3 becomes the default for new projects |
| 2026-10-23 | 3 | terrain/gen3_manifest | 7301d81c0596 | fec1e997acef876bff096992b1ea3b76f958da7d76326e889ecbe8b69d92be25 | generator 3b: water_dist texturing, water meshes, lakes in manifest |
| 2026-10-23 | 3 | terrain/gen3_manifest | fec1e997acef | 26bca1582052692dbbfe0eee80b3fb7b4bc95fd2b646f40e20374d79c9410581 | generator 3b: water_dist texturing, water meshes, lakes in manifest |
| 2026-10-23 | 3 | terrain/gen3_height_r16 | 9133a4e9467e | b214f503bfd728b231aeb7831ccacfb51dbbcd2e6791399ff194879b2c4e241c | gen-3 test fixture: shorter noise wavelength and lake thresholds so it forms lakes (exercises lake-flatness test) |
| 2026-10-23 | 3 | terrain/gen3_river_mask | 991cb98d04d7 | 3f671407a8f747d04affae576a1b860e2637d6e17acd1115705d0889ca598c78 | gen-3 test fixture: shorter noise wavelength and lake thresholds so it forms lakes (exercises lake-flatness test) |
| 2026-10-23 | 3 | terrain/gen3_manifest | 26bca1582052 | 0d48591e72d127a3b3cc3cd77bebec49d32829bfd16f842b2ee81cdb0bfc341f | gen-3 test fixture: shorter noise wavelength and lake thresholds so it forms lakes (exercises lake-flatness test) |
