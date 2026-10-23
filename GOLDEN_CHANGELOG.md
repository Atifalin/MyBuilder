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
