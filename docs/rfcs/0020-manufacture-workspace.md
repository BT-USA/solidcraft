# RFC 0020: Manufacture workspace — CAM and powder-bed builds that follow design edits

| | |
|---|---|
| Status | Draft, open for comment |
| Discussion | the pull request that adds this file, then an issue per phase |
| Revises | RFC 0001 (2026-11-07), for CAM and additive only; its PCBCraft parts are unchanged |
| Builds on | RFC 0002 (plugins), RFC 0015 (plugin panels, workspaces) |
| Apps | SolidCraft (host), CamCraft (subtractive), a new powder-bed sibling (proposed name PowderCraft) |
| Proposer | BT-USA |

Nothing here is built. The full proposal, with derivations, acceptance tests and sources, is in [0020-manufacture-workspace/full-proposal.md](0020-manufacture-workspace/full-proposal.md).

## 1. Goal

Design a part, switch the workspace dropdown to **Manufacture**, and machine it or print it from the live design, with no export or import. After every design edit SolidCraft says at once which items the edit affected and why. Regeneration is one command and runs in the background. A reference that is lost, merged, or split in a way that changes its meaning stops its item with **needs attention** and an explanation. Nothing is guessed.

The case that matters most: one base model in one design, carried through every process. Orient, support, lay out and slice parts on a build plate next to the CAD; machine them on the plate; cut them off by wire EDM; finish them in a fixture. Each step works on the body the step before it published, and every link is versioned together.

## 2. Ownership

| Owner | Owns |
|---|---|
| SolidCraft | Manufacture workspace shell, change-set events, persistent references, staleness detection, background regeneration, the plugin seam, save-format hooks, API versioning, manufacturing variants |
| CamCraft | Setups, stock, tools, operations, toolpaths, simulation, posts |
| PowderCraft | Orientation, PBF-LB/P nesting, PBF-LB/M overhangs and supports, slicing, distortion compensation, 3MF builds |

CamCraft and PowderCraft depend on SolidCraft's public API only. SolidCraft depends on neither. Both run as `craft:plugin` workbench components (RFC 0015), shown as *Manufacture › Subtractive* and *Manufacture › Additive*.

## 3. What the user sees

Every derived item (operation, support set, nest, plate, post) has one host-computed status: **Up to date**, **Stale** (with the reasons: which face, region or parameter changed, after which feature edit), **Needs attention**, **Regenerating**, **Failed** or **Never generated**. Status is a word plus a theme role, never colour alone. Commands: `manufacture.regenerate {items | setup | scope: "stale"}`, `manufacture.status`, `manufacture.wait`, `manufacture.cancel`, `ref.repick`, `manufacture.release`, `manufacture.trace`. All have schemas; programmatic calls never open dialogs.

## 4. Host API

**References.** `GeoRef {body, kind, name, arity: one|pieces, pin}` using SolidCraft's history names. Outcomes: `exact`, `pieces`, `merged`, `unverified`, `missing`. Anything but `exact` (or `pieces` where the item accepts pieces) is needs attention. Never retargeted.

**Change sets.** After every commit, undo or redo: features, parameters and faces (`{name, digest}`) that changed. Gated by the timeline's existing fingerprints, so unchanged bodies cost nothing.

**Staleness.** A regenerate job runs in the sandbox, where its only inputs are host calls. The host records what it read into a **footprint**: references and their outcomes, the regions of exact geometry read with SHA-256 *region digests* of the faces meeting them, parameters, metadata, the plugin's own storage paths, and upstream results. An item is up to date exactly when that footprint still matches the current design. A region digest leaves out trim loops whose influence bound misses the region, so a hole drilled far from a pocket does not make the pocket stale. After an edit, an R-tree of footprint regions finds the candidates; the work scales with the edit, not the design. Change-set events are only a speed-up: correctness never depends on their delivery. Backstops: a full check on open and on demand, an export gate that recomputes every stamp before writing, and a CI property test that regenerates every "up to date" item after random edits and requires byte-identical output.

**Background regeneration.** One job per item, on a design snapshot, in dependency order, off the window thread (web workers in the browser). A finished result is adopted against the *current* design: up to date if edits since missed its footprint, stale otherwise. Results are derived caches, not undo steps; earlier results are kept by stamp, so undo restores "Up to date" with no job.

**Plugin seam.** Four additive capabilities: `geometry@0.1` (exact, recorded reads: face info, region faces, sections, batch distance and ray queries, record bytes), `derived@0.1` (items, status, regenerate entry, published result bodies), `change-set@0.1` (an event), `manufacture@0.1` (process contribution to the workspace). No kernel type crosses the seam.

**Save format.** Plugin data and caches use RFC 0015 storage. New pending files: `manufacture-derived-items.rs`, `manufacturing-variants.rs`, `manufacture-process-plan.rs`, `design-source-id.rs`.

**Versioning.** Semver; `@since`/`@deprecated` gates; the manufacturing capabilities reach 1.0 only after two consumers ship against them.

**`.craftx`.** Stays the path for standalone CamCraft, agents and CI, gaining face and loop digests and an optional plugin-data part. A small app-agnostic crate, `craft-changeset`, computes the same change sets offline.

## 5. Manufacturing variants and the process plan

A variant is a short shop-side timeline evaluated after the design on a copy: fill holes, add material, stock, fixtures, plate placement. Its faces get history names; it follows every design edit. PowderCraft lays out variant instances on a plate and publishes the **as-built plate body**; a CamCraft setup uses it as stock and fixture, picks faces once for every instance, and cuts the parts off with wire EDM. Each CNC or EDM step publishes an **in-process body** (its stock minus what its verified path removes), and the next step takes it as stock, so steps can run in either order (machine on the plate then cut off, or cut off then machine in a fixture). The ordered steps form a host-owned **process plan** saved in the design; a final check compares the last in-process body with the base model and reports remaining stock per face. Staleness flows down the chain through footprints. `manufacture.release` records an immutable chain (design revision plus every item's stamp); every 3MF and posted program carries the design id, revision and stamp, and `manufacture.trace` maps a stamp back.

## 6. Licences

SolidCraft MIT OR Apache-2.0 (brand assets separate); CamCraft Apache-2.0; PowderCraft proposed MIT OR Apache-2.0. No GPL/LGPL/AGPL anywhere; ISO/ASTM texts cited, never copied.

## 7. Governance

Host API changes are RFCs here, with a consumers table (one reviewer each from CamCraft and PowderCraft) and a 10-day final comment period. Each project otherwise keeps its own process. Contract tests run in both directions from vendored, pinned components.

## 8. Phases

| Phase | Delivers | Owner |
|---|---|---|
| 0 | This RFC accepted here and in CamCraft's decision records | Both maintainers |
| 1 | References, change sets, footprints, staleness (headless) | SolidCraft |
| 2 | Background regeneration, workspace shell, capabilities, export gate | SolidCraft |
| 3 | CamCraft plugin, 2.5D on exact reads | CamCraft |
| 4 | Manufacturing variants | SolidCraft |
| 5 | PowderCraft: polymer nesting, slices, 3MF Production and Slice | PowderCraft |
| 6 | PowderCraft: metal overhangs and supports; process plan, in-process bodies, plate-to-finish chain, releases | PowderCraft, CamCraft, SolidCraft |
| 7 | `.craftx` offline staleness; browser hand-off | SolidCraft, CamCraft |

Every phase has acceptance tests with closed-form expected values, for example: a Ø6 hole whose bounds end 0.5 mm outside a pocket's region leaves the pocket up to date, and one that overlaps by 0.5 mm makes it stale; a pocket finish pass with 5 mm corners and a Ø6 tool is 80 + 4π mm long.

## 9. Open questions

1. Manufacture as a host workspace beside Design, Drawing and Render, with plugins as its processes?
2. Region digests from the start, or whole-face digests first?
3. `geometry@0.1` as an interface, or plain engine commands recorded the same way?
4. CamCraft: an ECS-free plugin crate over its existing generator crates? SolidCraft references as the primary pick identity?
5. Where does the PowderCraft repository live, and who reviews it first?
