# Craft bridge, revision 2: a Manufacture workspace for SolidCraft, powered by CamCraft and a powder-bed sibling

| | |
|---|---|
| Status | Proposal draft (not yet posted) |
| Proposer | BT-USA |
| Revises | `docs/rfcs/0001-craft-bridge.md` (Draft, 2026-11-07) |
| Builds on | RFC 0002 (plugins), RFC 0015 (plugin panels, workspaces and viewport content) |
| Repositories | tangency-labs/solidcraft (host), NeoTech/Camcraft (subtractive), a new additive repository (proposed name PowderCraft) |
| Read against | solidcraft `main` at `89a44fe` (2026-11-28); Camcraft `main` at `740b441` (2026-11-28) |

Nothing here is built. Every behaviour below is described from first principles and from public standards and papers listed in section 10. All example geometry is invented: blocks, pockets and holes with round numbers.

---

## 1. Summary and motivation

**The ask.** Let a person (or an agent) design a part in SolidCraft, switch the workspace dropdown from Design to Manufacture, and machine or print that same design with no export and no import. When the design changes, SolidCraft says at once which setups, operations, supports and build plates the change touched, says why, and lets the user regenerate them with one command that runs in the background. Nothing is ever guessed: a reference that is lost, merged or split in a way that changes its meaning stops its operation with "needs attention" and an explanation.

**Who owns what** (fixed by this proposal, not open for debate in it):

| Piece | Owner | Depends on |
|---|---|---|
| Manufacture workspace shell, change-set events, persistent geometry references, staleness detection, background-regenerate framework, plugin seam, save-format hooks, API versioning | **SolidCraft** | nothing below |
| Setups, stock, tools, operations, toolpaths, simulation, post-processing | **CamCraft** | SolidCraft's public host API only |
| Build orientation, powder-bed nesting, overhang and support analysis, distortion compensation, 3MF build output | **PowderCraft** (new sibling of CamCraft) | SolidCraft's public host API only |

The dependency arrow points one way. SolidCraft never depends on CamCraft or PowderCraft code. Both plug into the same seam, and both appear under one Manufacture entry: *Manufacture › Subtractive* and *Manufacture › Additive*.

**Why now.** Each piece already exists in part, and this RFC is mostly about joining them correctly:

- SolidCraft names every face from exact kernel history (`F2:top`, split pieces told apart by their neighbours, merges recorded as aliases) and already matches linked-design faces by those names with explicit outcomes.
- RFC 0002 and RFC 0015 already decided that a full manufacturing workflow may ship as a plugin, with host-drawn panels, a plugin workspace, per-design plugin data in the `.sc` container, long jobs and file-format slots. The `craft-plugin` and `craft-plugin-wasm` crates already run sandboxed WebAssembly components with transactions, panel data, storage and `design.changed` listeners.
- CamCraft already derives an operation's state from a digest of its inputs taken at generation time (no stored "stale" flag), already reads SolidCraft's model cache, and already follows picks by SolidCraft face names when the names leave no doubt.

What is missing is the part that makes the joined system trustworthy: an **exact, cheap, never-missing** answer to "which operations did this edit affect?", owned by the host so that every plugin gets the same answer.

**The one critical failure.** A path that is shown as up to date when it is not can put a cutter through a fixture, a clamp or a hand. The design below therefore makes staleness a property the host *computes* from what each generator actually read, inside a sandbox where nothing else can be read. It is never a flag that someone has to remember to set.

**One chain from design to finished part.** This is the case I care about most. A part is designed, oriented, supported, laid out on a build plate and sliced next to its CAD, all in the same design. Then the parts are machined on that same plate, cut off by wire EDM and finished in a fixture, each step taking as its stock the exact body the step before it left (section 2.4). Every link in that chain (design revision, manufacturing variant, plate layout, supports, slices, finishing setups, cut-off, posted programs) is versioned together. An edit anywhere upstream marks exactly the downstream links it affects, and every output file says which design revision and which chain produced it (section 5.2).

**What changes in RFC 0001.** RFC 0001 proposed two separate apps linked by an exchange file. This revision keeps that file (`.craftx`) as the path for standalone use, and makes the in-app path primary: CAM and additive run inside SolidCraft as `craft:plugin` workbench components, against the live design.

---

## 2. User experience

### 2.1 The workspace dropdown

The switcher that today offers Design, Drawing and Render gains one entry, **Manufacture**. It is a host workspace (`ui.workspace {"workspace": "manufacture"}`), not a `plugin:` workspace, because the host draws its shell. Inside it, a process selector picks one of the installed process plugins:

```
Design ▾
  Design
  Drawing
  Render
  Manufacture  ›  Subtractive   (CamCraft)
                  Additive      (PowderCraft)
```

If no process plugin is installed, Manufacture still opens and shows one line: which plugins provide processes, and that nothing is downloaded or run without the user's approval (RFC 0002 rules unchanged).

The host draws: the switcher entry, the process selector, the **Manufacture browser** (one tree: manufacturing variants, then setups or build plates, then items), the status column in that tree, the regenerate commands, the needs-attention banner and the jobs rows in the existing operations panel. The plugins draw nothing themselves. They declare panels (operation forms, tool library, nesting view), ribbon tabs and overlay layers through RFC 0015 interfaces, and each of their panels carries the plugin badge RFC 0015 requires.

### 2.2 Status of every item, and why

Every derived item (an operation, a toolpath, a support set, a nest, a build plate, a post output) has one status, computed by the host and never stored:

| Status | Shown as (theme roles, plus a word, never colour alone) | Meaning | What the user can do |
|---|---|---|---|
| Up to date | `ok` dot, "Up to date" | Every input this item's generator read is unchanged | Post, export, simulate |
| Stale | `warn` dot, "Stale" | At least one input it read has changed; the old result is still shown, drawn dimmed | Regenerate |
| Needs attention | `error` triangle, "Needs attention" | A reference it depends on is missing, merged, split in a way that changes its meaning, or only found by geometry | Re-pick or edit; regenerate is refused until then |
| Regenerating | progress ring, "Regenerating 40%" | A background job is running | Cancel |
| Failed | `error` dot, "Failed" | The generator refused (for example a tool that cannot reach a floor); its message is shown | Edit the item |
| Never generated | hollow dot | No result yet | Regenerate |

Status rolls up: a setup shows the worst status of its items, with a count ("3 stale, 1 needs attention").

**The reason is always shown.** Hovering or selecting a stale item shows the reason list, computed from the same data that decided the status:

```
Pocket 1 — Stale
  • Face "F2:top" changed inside this operation's region
      after "Hole 3" was edited (diameter 6 → 8 mm)
  • Parameter "wall" changed 3 → 3.5 mm (read by: finish allowance)
```

A needs-attention item shows a banner at the top of its form and in the browser:

```
Contour 2 — Needs attention
  The face picked as "Wall A" (F4:side:l2) was split into 2 pieces by "Slot 1".
  This operation needs one face. Re-pick the wall, or change the pick to "all pieces".
  [Show the pieces]  [Re-pick]
```

The old toolpath stays visible (dimmed, hatched), is never posted, and is never silently retargeted.

### 2.3 Regenerate commands

Regeneration is always explicit and always runs off the window thread.

| Command | Does | Returns at once |
|---|---|---|
| `manufacture.regenerate {items: [id]}` | Regenerates the named items | `{jobs: [{item, job}], refused: [{item, reason}]}` |
| `manufacture.regenerate {setup: id}` | Every stale or never-generated item in one setup | same |
| `manufacture.regenerate {scope: "stale"}` | Every stale item in the design | same |
| `manufacture.status {items?}` | Status and reasons, as data | `{items: [{id, status, reasons, job?}]}` |
| `manufacture.wait {jobs, timeout_ms}` | For scripts and agents: waits for jobs | `{done, pending}` |
| `manufacture.cancel {jobs}` | Cancels; nothing is adopted | `{cancelled}` |
| `ref.repick {item, slot, refs}` | Replaces one reference slot; one undo step | `{status}` |
| `manufacture.release {name, items}` | Records an immutable release: design revision plus every item's stamp; one undo step | `{release, items}` |
| `plan.add_step {kind, after, input}` / `plan.move_step` / `plan.set_input` | Edits the process plan (which step takes which earlier step's body); one undo step each | `{plan}` |
| `manufacture.trace {stamp}` | Which item, revision and releases a stamp (from an output file) belongs to | `{item, revision, releases}` |

All have input and output schemas. Programmatic calls never open dialogs: an item that needs attention is refused with a reason, not prompted. The ribbon shows the same three regenerate buttons (this item, this setup, all stale); the browser's context menu has them too. Keyboard shortcuts follow SolidCraft's shared-controls rules and are left to the maintainers.

### 2.4 One environment, one base model: design, print, EDM, CNC

This is the workflow the whole proposal is built around. One design file holds one **base model**. Every process step works on that model, or on a body derived from it inside the same design, and every step is linked to the steps before it. There is no export between steps and no second copy of the part anywhere.

**The steps, in one design:**

| # | Step | Workspace | Works on | Produces (published to the next step) |
|---|---|---|---|---|
| 1 | Design the part | Design | the base model | base body, face names |
| 2 | Shop variant: machining allowance on the faces to be finished, holes filled for drilling later, labels | Manufacture › Variants | base body | variant body (exact) |
| 3 | Orient, lay out instances on the build plate | Manufacture › Additive | variant body | plate layout |
| 4 | Supports, slices, 3MF build | Manufacture › Additive | layout | supports, slice stacks, 3MF, **as-built plate body** |
| 5 | Manual step marker: print, powder removal, stress relief | Manufacture › Plan | (none) | a recorded step with its own status and notes; no geometry |
| 6a | CNC on the plate: face, drill, bore while the parts are still held by the plate | Manufacture › Subtractive | as-built plate body as stock and fixture | toolpaths, posts, **in-process body** |
| 6b | Wire EDM cut-off from the plate | Manufacture › Subtractive | in-process body (or the as-built body if 6a is skipped) | EDM program, **cut-off part body** (with support stubs) |
| 7 | CNC finishing in a fixture: remove stubs, finish datums, final features | Manufacture › Subtractive | cut-off part body as stock, fixture bodies from the variant | toolpaths, posts, **finished in-process body** |
| 8 | Compare finished in-process body with the base model | Manufacture › Plan | finished body and base body | remaining stock per face; anything not removed is flagged |

Steps 6a and 6b can be swapped (cut off first, then machine in a fixture), or repeated (several CNC setups), because each step only reads the body the previous step published. Sinker or wire EDM finishing steps fit at any point the same way.

**What links them.** Each step's footprint records the regions it read of the body it was given (section 3.3). Each step publishes its result body through the host (section 3.5). So a design edit flows down the chain exactly as far as it really reaches:

```
Base model "Bracket"  rev 41
└─ Variant "AM + 0.5"                                   Up to date
   └─ Plate 1: layout ▸ supports ▸ slices ▸ 3MF          Up to date
      └─ As-built plate body
         └─ Setup "On plate"  (CNC)                      Stale  ← bore Ø8 → Ø8.5 in Design
            ├─ Face mill tops                            Up to date
            └─ Bore finish                               Stale  ← face "F6:bore" changed
               └─ In-process body
                  └─ Wire EDM cut-off                    Up to date (its region is the support layer)
                     └─ Cut-off part body
                        └─ Setup "Fixture 2"  (CNC)      Up to date
Plan check: remaining stock                              Stale
Release "R3"  rev 39                                     Outdated by 3 items
```

The user can move between Design, Manufacture › Additive and Manufacture › Subtractive at any time; all show the same revision of the same base model. A design edit made from Manufacture is an ordinary design edit.

**The plan.** The ordered list of steps is a host object, the **process plan**: steps, their order, which body each step takes from which earlier step, and manual-step markers. It is design data (one format file), editable as commands (`plan.add_step`, `plan.move_step`, `plan.set_input`), shown as the top level of the Manufacture browser, and versioned with the design. Releases (section 5.2) snapshot the whole plan.

### 2.5 Editing the design while in Manufacture

Design edits are allowed from any workspace. After every committed edit, undo or redo, the status column updates in the same frame the model does. Undo restores "Up to date" with no regeneration when the earlier result is still in the result cache (section 3.4).

---

## 3. Host API (owned by SolidCraft)

The host API is six pieces. All of them are additions: nothing existing changes meaning.

### 3.1 Persistent references

A **reference** is how a plugin names geometry. It is SolidCraft's own data, never a kernel type:

```
GeoRef {
  body:   BodyKey,              // component path + body name; renames are remapped by the host
  kind:   face | edge | vertex | sketch_curve | construction,
  name:   String,               // SolidCraft's history name, e.g. "F2:top", "F4:side:l4"
  arity:  one | pieces,         // does the operation need exactly one entity, or accept all pieces of it?
  pin:    Digest,               // the entity's geometry digest when picked (section 3.3)
}
```

`ref.resolve {refs}` returns one outcome per reference. The outcomes reuse the vocabulary SolidCraft already uses for linked designs, with one rule added: anything that is not certain stops the item.

| Outcome | Host's meaning | Item status |
|---|---|---|
| `exact` | The name, or a synonym of it (one face built from several roles), names exactly one entity | normal |
| `pieces` | Several pieces carry the name (a cut split the face) | `arity: pieces` → normal, using all pieces; `arity: one` → **needs attention** |
| `merged` | The name now survives only as an alias of a merged face | **needs attention**: the entity is not the one picked |
| `unverified` | Found only by geometry (`verified: false` in the naming record) | **needs attention**: "check this" |
| `missing` | Nothing answers to the name | **needs attention**, naming the face and the feature that last owned it |

The host never retargets a reference. Only `ref.repick` changes one, and it is one undo step.

Body renames and component moves go through the host's existing reference remapping (the same walk that renames body references in features), so plugin references follow them.

### 3.2 Change-set events

After every committed change to the document (an edit, undo, redo, a parameter change, a link update), the host computes a **change set** between the previous and the new revision:

```
ChangeSet {
  document_id, from_revision, to_revision,
  features_changed: [FeatureId],          // timeline features whose evaluation changed, in timeline order
  params_changed:   [{name, old, new}],
  bodies: [{
    body: BodyKey, state: unchanged | modified | added | removed | renamed{from},
    faces_removed:  [FaceKey], faces_added: [FaceKey], faces_changed: [FaceKey],
  }],
  metadata_changed: [key],                 // material, units, design metadata
}
FaceKey = {name, digest}
```

How it is computed cheaply:

1. **Gate on the timeline.** SolidCraft's evaluation already reuses a feature's result when its fingerprint (its definition, the values it reads and the prefix before it) is unchanged. A body whose producing results were all reused is `unchanged`, with no geometry work at all.
2. **Diff only modified bodies.** For each body that was re-evaluated, the host computes one digest per face (section 3.3), keyed by the face's history name. A face whose name and digest both match is unchanged. Digests are cached by the face's record identity, so an unchanged face is not re-hashed.
3. The change set lists face keys, not geometry. It is a few hundred bytes for an ordinary edit.

The change set is published as a `design.change_set` event on the control channel and to plugin listeners (an additive event next to the existing `design.changed`). **Correctness never depends on its delivery.** Events can be coalesced or missed; staleness (section 3.3) is computed by the host from stored footprints against the current design, and the change set only tells the host which footprints to re-check first.

### 3.3 Staleness detection: exact, cheap, never misses

**Principle.** An item is up to date exactly when everything its generator *read* is unchanged. So the host records what was read, at the moment it was read, and compares that record against the current design. Three properties make this sound:

1. **The sandbox closes the input set.** A regenerate job runs as a WebAssembly component with no file system, no network and no clock or random source unless granted (RFC 0002). Its only inputs are its arguments and the host calls it makes. The host records those calls. A generator cannot read something the host did not see, so a footprint cannot be incomplete by omission.
2. **Reads are region-scoped and exact.** Geometry is read through `geometry@0.1` (below). Every read names a region (an axis-aligned box in the item's frame) or a reference. The host answers from exact geometry and records, for each read: the region, and the key and *region digest* of every face whose exact bounds meet it.
3. **Comparison is bitwise.** Digests are SHA-256 of a canonical byte form of the exact record. Any change in any bit is a change. The cost of a false "stale" is one regeneration; the cost of a false "up to date" is a crash into a fixture. The design accepts the first to make the second impossible.

**The footprint** of one item after a successful generation:

```
Footprint {
  generator: {plugin_id, component_digest, entry, generator_version},
  host:      {host_api, kernel_version},
  args_digest,
  refs:     [{GeoRef, outcome, resolved: [FaceKey]}],
  regions:  [{body, frame, box, faces: [{key, region_digest}]}],
  params:   [{name, value_digest}],
  metadata: [{key, value_digest}],
  storage:  [{path, value_digest}],    // the plugin's own data it read: tool, setup, stock, previous ops
  upstream: [{item, result_digest}],   // results of other items it read (rest machining, a nest's parts)
}
stamp = SHA-256(canonical(Footprint))
```

**Region digest.** A large face (a block's top) is changed by a hole drilled far away from the pocket an operation machines. Hashing whole faces would make the pocket stale for no reason. The region digest of a face for a region Ω hashes the face's surface definition, its orientation, and only those trim loops whose *influence bounds* meet Ω, in a canonical order. A loop's influence bound is the exact bound of the surface patch over the loop's parameter box (for a plane, simply the loop's bounding box). A loop whose influence bound misses Ω cannot change which points of the face lie in Ω, so leaving it out loses nothing. Computing influence bounds is a kernel primitive and goes in solidcore, as SolidCraft's layer rules require.

**Fine reads stay cheap.** A gouge check may ask millions of distance questions. Each is a batch call over a buffer (`geometry.distance_batch`, RFC 0015 buffers). The host records only the union of the query regions (each point's ball of the asked radius), merged into a few boxes. Merging only grows boxes, which can only add false "stale", never remove a true one. A query with no radius (a global nearest) records the whole body.

**Checking after an edit.** The host keeps an R-tree of every footprint region, per body and frame. For a change set:

- each removed or changed face key is looked up in a key → items index;
- each added face's exact bounds are probed against the R-tree;
- each changed parameter, metadata key or plugin-storage path is looked up in its own index;
- each item whose upstream result digest changed is marked, transitively.

Candidates get their region digests recomputed. An item is stale if any recorded digest differs, or if a new face's region digest appears in one of its regions. Everything else keeps its status without being touched. The work is proportional to the size of the change, not the size of the design.

**Frames.** Region boxes and digests are taken in the item's own frame (a setup's work coordinate system, a build plate's frame), and a body's digest pairs its local record with the digest of its placement relative to that frame. Moving a component together with the setup that is attached to it therefore changes nothing; moving it relative to the setup changes the placement digest, and every item reading that body goes stale.

**Backstops** (a never-miss claim needs evidence, not only an argument):

- On open, after a host or plugin update, and on demand (`manufacture.verify`), the host re-checks every footprint in full, ignoring the indexes.
- The post and export gate recomputes the stamp of every item in the program from scratch before writing a byte. If any item is not up to date, the export is refused with the item names.
- A property test in CI (section 8) runs random edit sequences on invented parts and, for every item the host reports up to date, regenerates it from scratch and requires a byte-identical result. Expected mismatches: zero.
- Generators must be deterministic: the same footprint gives the same bytes. The test above enforces it.

**Why the host owns this.** CamCraft's existing digest is a good, conservative design, but it is a single 64-bit `DefaultHasher` value that is "not a cross-version identifier" and covers whole models. Moving the computation into the host gives one 256-bit, persisted, explained answer that the post gate, the UI, agents and every plugin share, and it lets the answer be finer (region-scoped) without trusting each plugin to list its inputs.

### 3.4 Background-regenerate framework

A regenerate request creates one **job** per item, in dependency order (an item that reads another item's result runs after it).

1. The job takes an immutable snapshot of the design at the current revision (the evaluated model is already shared and copy-on-write).
2. It runs the plugin's declared `regenerate` entry in its own component instance, on a worker thread (a web worker in the browser build). Independent items run in parallel, up to the worker count; WebAssembly components do not need threads of their own for this.
3. All geometry reads go to the snapshot and are recorded into the footprint.
4. On success the plugin returns its result as a buffer (a toolpath, a support mesh, a slice stack) plus a small summary.
5. **Adoption.** The host checks the new footprint against the *current* design, not the snapshot. If the user edited something outside the footprint while the job ran, the result is adopted as up to date. If the edit touched the footprint, the result is adopted as stale, with the reason "the design changed while this was regenerating", and the scheduler may run it again if the user asked for "all stale".
6. Adoption writes the result as a cache blob in the plugin's storage (RFC 0015 section 5, `cache: true`) and the footprint into host data. It is not an undo step: results are derived data, like the model cache.

**Result cache.** The host keeps up to N earlier results per item keyed by stamp. After an undo, the item's current stamp often matches an earlier result exactly; the host adopts it at once and the item is up to date again with no job.

The window thread only moves state; it never waits on a job. Progress, partial overlays (a toolpath growing on screen) and cancel use the operations panel exactly as RFC 0015 section 6 describes. The 10-minute job deadline from RFC 0015 becomes a per-plugin declared budget within the user's ceiling, as RFC 0002 section 6d already proposes for background roles.

### 3.5 The plugin seam

The seam is `craft:plugin`, extended with four **additive** capabilities. Each is its own WIT interface, so a plugin imports only what it uses and older plugins keep loading.

| Capability | Interface (host side) | Provides |
|---|---|---|
| `geometry@0.1` | `geometry` | Exact, recorded reads: `resolve(refs)`, `face-info(ref)` (surface type and exact parameters: a plane's point and normal, a cylinder's axis and radius), `region-faces(body, frame, box)`, `section(body, plane)` as exact curves, `distance-batch(points, radius)`, `ray-batch`, `body-record(body, frame)` (the solidcore-record bytes, footprinted as the whole body). No kernel types cross the seam: records are bytes in a versioned format the host names. |
| `derived@0.1` | `derived`, `derived-provider` | The plugin registers its derived items (`id`, label, kind, parent, reference slots with arity). The host owns their status, reasons, browser rows and regenerate commands. The plugin exports `regenerate(item) -> result`. |
| `change-set@0.1` | event | `design.change_set` delivered to a declared read-only listener, for cache invalidation and panel refresh. Never needed for correctness. |
| `manufacture@0.1` | `workspace-contribution` | Declares the plugin as a process under Manufacture (`subtractive` or `additive`), its ribbon tabs and panels in that workspace, under RFC 0015's limits and badges. |

Plugins still change the design only through engine commands, so every edit stays undoable and journaled. A plugin cannot read another plugin's storage. CamCraft and PowderCraft do not call each other; where additive needs a machining allowance, both read the same manufacturing variant (section 5).

**Published bodies.** `derived@0.1` also lets an item publish a *result body* through the host: for example a build plate publishes the as-built body (plate, supports and placed part instances, exact where they come from exact geometry, with supports as mesh bodies). Other plugins read it through `geometry@0.1` like any design body, and their footprints record the regions they read of it with region digests, exactly as for a design body (a whole-body read records the whole body's digest). This is how CamCraft machines a plate that PowderCraft laid out, with no plugin-to-plugin call and with staleness flowing down the chain.

The host API never exposes truck or any other kernel internal. Exact geometry crosses the seam as SolidCraft's reference strings, SolidCraft's measured values and solidcore-record bytes, which are already SolidCraft's public "geometry as data" form and already the preferred body format in `.craftx`.

### 3.6 Save-format hooks

| Data | Where | Format change |
|---|---|---|
| Plugin data (setups, tools, operations, nests, supports) | `plugins/<id>/data.json` in the `.sc` container | none: RFC 0015 `storage@0.1` exists |
| Cached results | `plugins/<id>/blobs/...`, `cache: true` | none |
| Derived-item registry and footprints (host-owned) | `manufacture/derived.json` | **one new file:** `crates/doc/src/format/pending/manufacture-derived-items.rs` |
| Manufacturing variants (section 5) | timeline data of the design | **one new file:** `crates/doc/src/format/pending/manufacturing-variants.rs` |
| Permanent design id (from RFC 0001) | design metadata | **one new file:** `crates/doc/src/format/pending/design-source-id.rs` |
| Process plan and releases | `manufacture/plan.json` | **one new file:** `crates/doc/src/format/pending/manufacture-process-plan.rs` |

Each file starts with the `//!` note the format rules require. An older release opens such a design with its usual warning, keeps what it cannot read and saves it back unchanged. A design with Manufacture data opened where the plugin is missing behaves as RFC 0015 already says: the design opens, a quiet banner names the plugin, and its data survives a save. Footprints are kept so that a reopened design shows the right status without regenerating; the open-time full check (3.3) confirms them, and a changed kernel version simply makes items stale.

### 3.7 API versioning

- The contract is: the `craft:plugin` WIT package, the capability strings, the JSON schemas of the `manufacture.*`, `ref.*` and `geometry.*` commands, the change-set schema and the `.craftx` manifest.
- Versions follow semantic versioning. While at 0.x, a minor version may break the one before (the rule `craft-plugin`'s WIT already states). The host keeps running components built against the previous minor, as it already runs 0.1 components.
- New items in a stable package carry `@since` gates; removals go through `@deprecated` first and stay for at least two host minor releases.
- **1.0 needs two shipped consumers.** The manufacturing capabilities are frozen as 1.0 only after both CamCraft and PowderCraft have shipped against them. One consumer cannot tell a general API from its own shape.

### 3.8 The `.craftx` path for standalone use

The in-app path is primary. `.craftx` stays as the exchange for anyone running CamCraft as its own application, for agents and CI, and for the browser hand-off RFC 0001 described. Three additions make it carry the same guarantees:

1. `faces.json` gains each face's `digest` and loop digests (the inputs of region digests), so two exchange revisions can be diffed into the same change set offline.
2. A small app-agnostic crate, `craft-changeset` (in SolidCraft's repository, like `craft-plugin`, with `cargo xtask layers` forbidding any SolidCraft dependency), implements digests, change sets and footprint checks. Standalone CamCraft links it and gets the same staleness answer as the plugin does.
3. An optional `plugins/<id>/` part carries the plugin's own data, so a design's CAM data can travel to a standalone CamCraft and back without loss.

The manifest stays `craft-exchange/1`; these are new optional parts and fields, which RFC 0001 already allows as minor additions.

---

## 4. CamCraft's side

### 4.1 What CamCraft brings

CamCraft is a large Apache-2.0 Rust workspace. The parts this proposal uses are already separated from its application layer:

| Crate | Role here |
|---|---|
| `camcraft-cl` | The move model and the machine-neutral intermediate path |
| `camcraft-tools` | Cutters and hole-making cycles as data |
| `camcraft-motion` | The generators: 2D pockets and profiles, 3D depths, waterline and parallel finishing, drilling, and more. Model and tool in, path out; no ECS |
| `camcraft-sim` | Stock simulation, rest material, verification; no ECS |
| `camcraft-post` | The post engine and machine definitions |
| `camcraft-geometry` | Mesh and 2D geometry; today also STEP and SolidCraft-model import through `camcraft-kernel` |

`camcraft-app` (the ECS document, commands, MCP and project file) and `camcraft-ui` stay what they are: the standalone application.

### 4.2 What CamCraft would change

| Change | Why | Size |
|---|---|---|
| **A `camcraft-plugin` crate** building a `craft:plugin` workbench component, with no Flecs dependency. It keeps CamCraft's document as plain serde data in plugin storage and calls the existing ECS-free crates. | The component must build for the host's wasm targets; the host's app code is Rust-only. The generators are already ECS-free, so the work is the orchestration that today reads the ECS world (boundaries, heights, setups, ordering). | Large; can grow one operation family at a time |
| **Exact geometry for decisions.** Floor depths, wall positions, hole diameters and axes come from `geometry.face-info` and exact sections, not from facets. Gouge and containment verification runs against exact `distance-batch` queries. A mesh may still accelerate generation, but only with a certified chord bound inside the tolerance budget, and the result is verified against exact geometry before it is adopted. | SolidCraft's rule: model decisions use exact geometry; a mesh is for display and preview. It is also CamCraft's own "never gouge" directive made provable. | Large; done per operation family |
| **Picks are SolidCraft references first.** A pick stores a `GeoRef` with arity. Facet-hash ids remain for meshes CamCraft imports itself. | CamCraft already follows picks by SolidCraft face names when the names leave no doubt; this makes the name the identity rather than the fallback. | Medium |
| **Inputs through the host.** Tool, setup, stock and previous-operation data are read through recorded storage paths, so the host's stamp covers them. CamCraft's own stamp becomes a cross-check in tests. | One staleness answer for UI, agents and the post gate. | Small |
| **No second kernel in the component.** The plugin build leaves out `camcraft-kernel`'s pinned solidcore fork and its truck reference implementation; exact geometry comes from the host. | One implementation per geometric job, one kernel version per answer, and no kernel version skew between the design and its toolpaths. | Small (feature flags) |
| **Posting as an export slot** (RFC 0015 section 4), refused by the host's export gate unless every operation in the program is up to date. | The gate must not depend on the plugin remembering to check. | Small |
| **Crate metadata.** Add `license = "Apache-2.0"` to each crate's `Cargo.toml`. | So the host's licence tooling can read it without opening the LICENSE file. | Trivial |

The standalone application keeps working throughout. Nothing here asks CamCraft to remove features or to change its own process.

### 4.3 Text and labels

Labels and parameter names shown inside SolidCraft's workspace go through SolidCraft's message catalogs and its text review, like every other string the host shows. CamCraft's own provenance records (each module's one-line source note and its decision files) are the evidence a reviewer needs.

---

## 5. The manufacturing variant

A **manufacturing variant** is a shop-side copy of a design body that lives in the same `.sc` design, follows every design edit, and carries the changes a shop makes and a designer should never see: stock, filled holes, added material, fixtures, an additive build allowance.

### 5.1 How it works

- A variant is a **second short timeline that starts from the evaluated design body**. Its features ("shop features") are ordinary SolidCraft features evaluated by the same engine, on a copy, after the design timeline. The design itself is never changed.
- Shop features in the first version:

| Shop feature | Use | Built from |
|---|---|---|
| Fill holes and pockets | Leave small holes for drilling later, or close holes that a later setup machines | existing delete-face and patch operations |
| Add material to faces | Finish allowance, or a machining allowance on an additively built part | face offset |
| Stock body | Box, cylinder or offset stock around the variant | primitives and offsets |
| Fixture bodies | Vises, plates, clamps as linked components | Insert Design links |
| Build plate placement (additive) | Orientation and plate transform | placement |
| Compensated shape (additive, later) | Pre-deformed geometry for distortion (section 6) | displacement field; export mesh only in its first version |

- Faces made by shop features get history names of their own (for example `V1:fill:F7:wall`), so references into a variant are as persistent as references into the design.
- Shop features reference design faces with the same `GeoRef` rules. If a designer deletes the hole a "Fill" feature filled, that shop feature needs attention, by name. It is never re-aimed.
- CAM setups and additive build plates reference **variant** bodies, not design bodies. An edit to the design re-evaluates the variant; only faces that really changed get new digests; only items whose footprints meet them go stale.
- One design can hold several variants: one for a first setup with holes filled, one for an additive build with a 0.5 mm machining allowance on the faces CamCraft will finish afterwards. Both plugins read the same additive variant, which is how "print, then machine" works without the two plugins talking to each other.

### 5.2 Print and finish on one plate, versioned as one chain

**The flow.**

1. A variant adds machining allowance to the faces that will be finished (bores, sealing faces, datum faces).
2. PowderCraft places instances of that variant on a build plate, generates supports and slices, and publishes the **as-built plate body**: plate, supports and parts in the plate frame.
3. CamCraft creates a setup whose stock *and* fixture is that published body and whose work frame is attached to the plate frame. Finishing operations reference the variant's faces through the instances (each instance's references are the variant's references under its placement), so one pick serves every copy on the plate.
4. A **wire EDM cut-off** operation (CamCraft already has wire EDM generators) separates the parts from the plate along a cut surface the user places above the plate, typically through the supports. Its footprint reads the supports and the plate body, so a change in support height marks it stale.
5. Each CNC and EDM step publishes its own **in-process body**: the stock it was given minus the material its verified toolpath removes, computed by CamCraft's stock simulation and published through the host. The cut-off step publishes the separated part with its support stubs. A later setup (finishing in a fixture after cut-off) takes that body as its stock, so stub removal and final features are programmed against what will actually be in the vise.
6. A final plan check compares the last in-process body with the base model and reports remaining stock per face. Anything left that the design does not have is flagged; anything removed that the design needs is an error that blocks release.
7. Posts for every CNC and EDM step, and the 3MF for the build, are written from the same revision.

**Linked versioning.** Three mechanisms, all host-owned:

- **The chain is the footprints.** Every item's footprint lists its upstream result digests. A design edit that moves one wall makes that variant face change; the plate's layout and supports are stale only if their footprints meet it; the as-built body digest changes only then; and each finishing operation is stale only if its own regions meet a changed face. Exactly the affected links go stale, transitively, with the reason traced back to the design edit.
- **Releases.** `manufacture.release {name, items}` records an immutable entry in the design: the design revision, and the stamp of every item in the chain (plate, supports, slices, setups, operations, posts). A release never changes. The browser shows whether the current state still matches it ("Outdated by 2 items", listing them). Releases are saved in the host's `manufacture/derived.json`, so they travel with the design and are versioned by whatever version control holds the `.sc` file.
- **Outputs carry their lineage.** Every 3MF build (as package metadata) and every posted program (as a header comment in the post's own syntax) carries the design's permanent id, its revision, the release name if any, and the stamp of the item that produced it. A program found on a machine can be traced to the exact design revision and plate that produced the part, and `manufacture.trace {stamp}` answers which items and which release it belongs to.

**What is exact and what is mesh here.** Parts and the plate are exact B-rep. Supports are generated as mesh bodies (that is what supports are in a build file). CamCraft's collision and clearance checks against supports use their meshes as obstacles, which is the safe direction: the mesh is the support's actual shape. Every decision about the part (depths, walls, bores, cut-off clearance to the part) uses exact geometry.

### 5.3 Ownership and format

Variants are modelling, so they belong to SolidCraft: host data, host commands (`variant.create`, `variant.add_feature`, ...), host undo. They are shown in the Manufacture browser above the setups and build plates. They need one format file (section 3.6).

### 5.4 What a variant is not

It is not a second design file, not an exported copy and not a mesh. It is exact geometry, re-evaluated on every edit, saved inside the design it came from.

---

## 6. The additive sibling project

### 6.1 Name, scope and terms

- **Proposed name: PowderCraft.** It says what the first version does (powder-bed fusion) and sits in the Craft family. If the scope later grows past powder beds, *LayerCraft* is the fallback. A trademark search is still to do before the repository is created.
- **Scope of version 1**: laser powder-bed fusion of polymers and of metals. This proposal uses the ISO/ASTM 52900:2021 designations, **PBF-LB/P** and **PBF-LB/M**, and the standard's vocabulary throughout (build volume, build platform, part orientation, support, downskin and upskin surfaces).
- **Not in scope**: machine-native build or job formats, machine control, scan-vector and laser-parameter generation, heat treatment. A user who needs a machine format adds an export slot of their own.

### 6.2 Features common to both processes

| Feature | Description | Exact or mesh |
|---|---|---|
| Machine and material profiles | User-entered data: build volume, layer thickness, minimum spacing, overhang threshold, material shrinkage. No vendor data is bundled. | data |
| Orientation study | For candidate orientations: build height, projected support area, downskin area by angle band, cusp height estimate (layer thickness × the cosine of the angle between face normal and build direction) | exact faces |
| Design checks against ISO/ASTM 52910 and 52911-1/-2 considerations | Minimum wall, minimum hole, enclosed voids with no powder escape path, thin features across the recoater direction. Thresholds are profile data the user owns; standards text is never copied into the code or the UI. | exact faces and exact sections |
| Part labels and allowances | Through manufacturing variants (section 5) | exact |
| Slicing | Exact planar sections of the variant body at each layer, polygonised at a stated chord tolerance only when written out | exact, mesh only at output |

### 6.3 PBF-LB/P (polymer) features

- **3D nesting** of many parts in the build volume, in three stages: bounding-box placement first (fast, predictable), then a voxel-occupancy packer, then rotations. Minimum part spacing and a margin from the build volume walls come from the profile.
- Packing density (part volume over the used build volume) and build height, shown for every nest.
- Optional enclosing cage for small parts, generated as a variant body.
- Powder-use estimate from build height and the build area.

### 6.4 PBF-LB/M (metal) features

- **Overhang classification** of exact faces by the angle between the downward face and the build platform, against the profile's threshold (commonly about 45° in the literature, material-dependent, so a profile value, not a constant).
- **Support generation**: block, line and cone supports under the classified regions, written as 3MF objects of type `support`, with exact contact regions on the B-rep faces. Support height and contact areas reported.
- **Build platform placement** and a minimum gap to the platform edges.
- **Distortion compensation**, marked experimental: a pre-deformation computed from a user-calibrated inherent-strain model, written as a compensated mesh for the 3MF output only, never fed back into the exact design.
- **Machining allowance handshake**: the variant that adds stock to faces CamCraft will finish is the same body PowderCraft builds, so "build then machine" is one design.

### 6.5 3MF output

PowderCraft writes builds through SolidCraft's existing 3MF writer, extended once in `crates/io` rather than written a second time:

- 3MF Core 1.3 objects and build items, with part numbers;
- the **Production** extension (1.1.2): a UUID for the build and every build item, objects in separate model parts;
- the **Slice** extension (1.0.2): per-object slice stacks taken from the exact sections above;
- supports as objects of type `support` (or `solidsupport` where the support is a closed solid);
- SolidCraft's existing face-tag attribute, so a downstream tool can map triangles back to B-rep faces.

### 6.6 Licence

| Project | Licence | Effect |
|---|---|---|
| SolidCraft | MIT OR Apache-2.0 for code; a brand licence for the name, logo, app icon and interface icon artwork | Any plugin may link `craft-plugin` and use the WIT. Plugins may not reuse SolidCraft's interface icon artwork outside SolidCraft without permission. |
| CamCraft | Apache-2.0 | May depend on SolidCraft's MIT OR Apache-2.0 crates. SolidCraft does not depend on CamCraft, so CamCraft's licence places no condition on SolidCraft. A CamCraft component shipped alongside SolidCraft keeps its NOTICE. |
| PowderCraft (proposed) | **MIT OR Apache-2.0** | The same terms as SolidCraft, so a geometric job that turns out to belong in solidcore or SolidCraft (exact slicing, influence bounds) can move there with no relicensing, and the Apache-2.0 option carries an express patent licence from contributors, which matters in a field with many patents. |

All three use only licences on the Apache "Category A" list in their dependency trees (MIT, Apache-2.0, BSD, ISC, BSL-1.0) and no GPL, LGPL or AGPL code, checked by `cargo xtask licences` on the host and `cargo deny` in the plugins. ISO/ASTM standards are copyrighted: they are cited and used as references, and their text and tables are not copied.

### 6.7 How it starts

1. **Repository.** A new, standalone repository, created only after Phase 0 agreement. My preference, in order: a neutral organisation for the Craft family that the three projects' maintainers co-own; otherwise tangency-labs, next to the host whose API it uses; otherwise NeoTech. It is a separate repository in every case, never a folder inside SolidCraft or CamCraft, so it releases on its own schedule and is clearly a peer.
2. **First maintainers.** I (BT-USA) maintain it at first, with one reviewer named by each of SolidCraft and CamCraft as code owner for the host-API surface (`Cargo.toml` dependencies on `craft-*`, the manifest and the WIT bindings). More maintainers by the usual merit route.
3. **Day-one files.** `AGENTS.md` with the same hard rules as the host (never crash, exact geometry for decisions, everything a command, no GPL code, clean-room protocol, one implementation per job), a licence check, an ATTRIBUTION file, and the Phase 5 acceptance tests as the first issues.

---

## 7. Governance: three projects, one API

### 7.1 Who decides

| Question | Decides | Process |
|---|---|---|
| A change to the host API (WIT, capabilities, command schemas, change-set schema, `.craftx`) | SolidCraft maintainer | An RFC in `tangency-labs/solidcraft/docs/rfcs`, with a **consumers** table: CamCraft and PowderCraft each name a reviewer. Acceptance needs the host maintainer's decision and no unresolved blocking concern from a consumer after a final comment period of 10 days. |
| Anything inside CamCraft | CamCraft maintainer | CamCraft's own issue, PR and decision-record process, unchanged |
| Anything inside PowderCraft | PowderCraft maintainers | An RFC folder in its own repository, the same shape as SolidCraft's |
| A change that touches two repositories (for example a new geometry read both plugins need) | SolidCraft first, then each consumer | Host RFC first; consumers adopt in their own repositories afterwards |

The process is modelled on the Rust project's: substantial changes go through a written RFC, discussion happens in the pull request, and a final comment period gives everyone a last chance to raise a concern before the decision.

### 7.2 Compatibility in practice

- **Contract tests in both directions.** Each consumer publishes a pinned test component and its expected results (closed-form values, as in section 8). The host's CI runs them offline, from vendored files. Each consumer's CI runs against the host's nightly build.
- **A compatibility table** in the host's documentation: host version, `craft:plugin` version, capability versions, and the plugin versions known to work.
- **Deprecation** as in 3.7. A host release never drops a capability a published consumer still uses without a replacement that has shipped for two minor releases.
- **Security and approval** stay the host's: install review, permissions, budgets and the badge on every plugin panel follow RFC 0002 and RFC 0015, for first-party and third-party plugins alike.
- **Shared crates** (`craft-plugin`, `craft-plugin-wasm`, the proposed `craft-changeset`) start in SolidCraft's repository and move to their own repository, as solidcore did, when a second app needs to release on its own schedule.

---

## 8. Phased plan

Every phase has one deliverable, one owner and acceptance tests whose expected values are derived by hand below each test. No expected value is taken from the code under test. "Block" means an invented test part: a block 100 × 60 × 20 mm, centred on the origin in X and Y, bottom face at Z = 0.

### Phase 0: Agreement

| | |
|---|---|
| Deliverable | This RFC accepted (status Accepted) in `docs/rfcs/`, and a matching decision record in CamCraft's `docs/decisions/` |
| Owner | SolidCraft and CamCraft maintainers; proposer drafts |
| Acceptance | (a) The SolidCraft maintainer approves the integration, as the packaging rules require for any platform integration. (b) The CamCraft maintainer agrees to the plugin build and to the reference-first picks. (c) Both agree the repository home and first reviewers for PowderCraft. (d) Open questions in section 9 each have an answer or an owner. |

### Phase 1: References, change sets and footprints (headless)

| | |
|---|---|
| Deliverable | `ref.resolve`, face and region digests (influence bounds in solidcore), `design.change_set`, footprints, the R-tree staleness check, `manufacture.status`, the backstops; `craft-changeset` crate; one in-repo toy plugin fixture |
| Owner | SolidCraft |

| Test | Set-up | Expected (derivation) |
|---|---|---|
| 1.1 Unrelated parameter | Block; add a parameter `note = 5` used by nothing; change it to 6 | 0 bodies modified; toy item stays up to date (no feature reads it, so every fingerprint is reused) |
| 1.2 Far hole | Block with pocket 40 × 20 × 5 at the centre; toy item's region = pocket grown by tool radius 3 + clearance 2 = X ∈ [−25, 25], Y ∈ [−15, 15]. Add a Ø6 through hole at (−40, −20) | Hole loop bounds X ∈ [−43, −37]: max −37 < −25, so disjoint. Change set: 1 face added (the hole wall), 2 changed (top and bottom). Region digests of top and bottom for the item's region unchanged. Item **up to date**. |
| 1.3 Near hole, outside | Same, hole at (−28.5, 0) | Bounds X ∈ [−31.5, −25.5]; gap 0.5 mm > tolerance. **Up to date.** |
| 1.4 Near hole, inside | Same, hole at (−27.5, 0) | Bounds X ∈ [−30.5, −24.5]; overlaps by 0.5 mm. **Stale**, reason names the top face and the hole feature. |
| 1.5 Rigid move with frame | Move the block's component 10 mm in X with the toy setup frame attached to it | Local records and relative placement unchanged → **up to date** |
| 1.6 Rigid move without frame | Same move, frame fixed in the world | Placement digest changes → **stale** |
| 1.7 Split face | A 4 mm wide slot across the pocket floor in Y | Floor splits into 2 pieces of (40 − 4) / 2 × 20 = 360 mm² each (sum 720 = 800 − 80). Ref with `arity: one` → **needs attention** naming the floor; with `arity: pieces` → 2 resolved faces, **stale** |
| 1.8 Missing | Delete the pocket feature | Floor ref `missing` → **needs attention** naming the face and the deleted feature |
| 1.9 Undo | Edit as 1.4, regenerate, undo | Stamp equals the earlier result's stamp → **up to date** with no job |
| 1.10 Never-miss property | 10 000 random edit sequences over the invented parts (seeded, reproducible) | For every item reported up to date, a fresh regeneration is byte-identical to its cached result: **0 mismatches** |

### Phase 2: Background regeneration and the Manufacture shell

| | |
|---|---|
| Deliverable | Job scheduler (desktop threads, web workers), adoption against the current design, result cache, `manufacture.regenerate / wait / cancel`, `ref.repick`, capabilities `geometry@0.1`, `derived@0.1`, `change-set@0.1`, `manufacture@0.1`, the host-drawn workspace with status column, reasons and banner, the export gate, the format files |
| Owner | SolidCraft |

| Test | Set-up | Expected (derivation) |
|---|---|---|
| 2.1 Toy contour length | Toy plugin offsets the outline of a 40 × 20 face outward by r = 3 with round corners | L = 2(40 + 20) + 2π·3 = 120 + 6π = **138.849556 mm** (±1 µm) |
| 2.2 Off-thread | Job held on a test barrier | The harness renders ≥ 10 frames and accepts an unrelated edit while the job is blocked (no wall-clock timing) |
| 2.3 Edit during job, outside | Release the barrier after the far-hole edit of 1.2 | Result adopted **up to date** |
| 2.4 Edit during job, inside | Same with the inside edit of 1.4 | Adopted **stale**, reason "design changed while regenerating" |
| 2.5 Export gate | One item stale in a program | Export refused; the error names that item; 0 bytes written |
| 2.6 No dialogs | `manufacture.regenerate` on a needs-attention item over MCP | Refusal with reason; no form opened |
| 2.7 Browser build | `cargo xtask wasm`, `web-offline-test` with the toy plugin | Same results as 2.1–2.5 |

### Phase 3: CamCraft as a plugin, 2.5D first

| | |
|---|---|
| Deliverable | `camcraft-plugin` component: setups, stock, tool library, pocket, contour, drilling, posting as an export slot, all on exact reads |
| Owner | CamCraft |

| Test | Set-up | Expected (derivation) |
|---|---|---|
| 3.1 Floor depth | Block, pocket 40 × 20, 5 deep from the top | Floor Z = 20 − 5 = **15.000** read from the exact plane |
| 3.2 Finish pass length | Same pocket with 5 mm corner radii, flat end mill Ø6 | Tool centre path: straight runs (40 − 10) and (20 − 10), corner radius 5 − 3 = 2: L = 2(30 + 10) + 2π·2 = 80 + 4π = **92.566371 mm** |
| 3.3 Wall clearance | Same | Minimum exact distance from tool centre to walls = **3.000 mm** (−0 / +1 µm) |
| 3.4 Drill points | 4 × Ø6 holes on a 30 × 10 rectangle | 4 points at (±15, ±5), depth to the exact hole bottom |
| 3.5 Edit and regenerate | Pocket depth 5 → 6 | Pocket **stale** naming the floor; after regeneration floor Z = **14.000**; drilling ops unchanged and **up to date** (their regions do not meet the pocket) |
| 3.6 Lost floor | Delete the pocket feature | Pocket **needs attention**; post refused |

### Phase 4: Manufacturing variants

| | |
|---|---|
| Deliverable | Variant timeline, shop features (fill, add material, stock, fixtures, placement), browser rows, format file |
| Owner | SolidCraft |

| Test | Set-up | Expected (derivation) |
|---|---|---|
| 4.1 Fill a hole | Block with one Ø6 through hole | Design volume 120 000 − π·3²·20 = **119 434.513 mm³**; variant with the hole filled **120 000 mm³** |
| 4.2 Allowance | Variant adds 0.5 mm to the top face | **123 000 mm³** (+100 × 60 × 0.5) |
| 4.3 Follows the design | Change block length 100 → 110 | Variant = 110 × 60 × 20.5 = **135 300 mm³**; CAM items on the variant's top face **stale**, others per their regions |
| 4.4 Lost target | Delete the hole the fill feature filled | Fill feature **needs attention** by name; never re-aimed |

### Phase 5: PowderCraft, polymer builds

| | |
|---|---|
| Deliverable | New repository; profiles; orientation study; PBF-LB/P nesting (bounding-box stage); 3MF with Production and Slice extensions through the host writer |
| Owner | PowderCraft (host writer extension: SolidCraft) |

| Test | Set-up | Expected (derivation) |
|---|---|---|
| 5.1 Nest | Ten 20 mm cubes, spacing 5, build volume 100 × 100 × 100 | Per row ⌊(100 + 5) / 25⌋ = 4, so 16 per layer: all 10 in one layer; build height **20 mm**; packing density 10 × 8 000 / (100 × 100 × 20) = **0.40** |
| 5.2 Production extension | Write the nest of 5.1 | 1 build UUID, **10** distinct item UUIDs; SolidCraft's 3MF reader reads 10 objects of volume **8 000 mm³** each (a cube's tessellation is exact) |
| 5.3 Slices, prism | One cube, layer 0.1 mm | **200** slices, each of area **400 mm²** |
| 5.4 Slices, cylinder | Ø20 vertical cylinder, chord tolerance δ = 0.01 mm | Fewest segments n = ⌈π / arccos(1 − δ/r)⌉ = ⌈70.24⌉ = 71; each slice area A satisfies (n/2)·r²·sin(2π/n) = **313.749** ≤ A ≤ π·r² = **314.159 mm²** |
| 5.5 Orientation | Ø20 × 10 cylinder lying on its side, threshold 45° | Downward lateral surface within 45° of straight down is 90° of 360°: π·20·10 / 4 = **157.080 mm²** |

### Phase 6: PowderCraft, metal builds

| | |
|---|---|
| Deliverable | Overhang classification, block and cone supports as `support` objects, platform placement; experimental distortion compensation behind a profile switch |
| Owner | PowderCraft |

| Test | Set-up | Expected (derivation) |
|---|---|---|
| 6.1 Shallow incline | Downward face at 30° from the platform, 10 mm run in X, 20 mm in Y; threshold 45° | Area 10 / cos 30° × 20 = **230.940 mm²**, all supported |
| 6.2 Steep incline | Same at 60° | **0 mm²** supported |
| 6.3 Ledge | 20 × 20 mm horizontal ledge at Z = 10 | Support contact area **400 mm²**, support envelope 20 × 20 × 10 = **4 000 mm³** |
| 6.4 Stale supports | Raise the ledge to Z = 12 | Supports **stale** naming the ledge face; after regeneration envelope **4 800 mm³** |

### Phase 6b: Design, print, EDM and CNC on one base model, with releases

| | |
|---|---|
| Deliverable | Process plan; published result bodies in `derived@0.1`; CamCraft in-process bodies chained step to step; CamCraft setups on an as-built plate body with instance-aware references; wire EDM cut-off on the plate; `manufacture.release` and `manufacture.trace`; lineage in 3MF metadata and post headers |
| Owner | SolidCraft (published bodies, releases, lineage), PowderCraft (as-built body), CamCraft (plate setups, cut-off) |

Set-up for all rows: a plate 100 × 100 × 10 mm (top at Z = 10); four instances of a 20 × 20 × 15 mm block variant, each with one Ø6 bore through its centre, placed at (±25, ±25) on 3 mm block supports (part bottoms at Z = 13).

| Test | Set-up | Expected (derivation) |
|---|---|---|
| 6b.1 One pick, four copies | Drill the bore picked once on the variant | **4** drill points at (±25, ±25); depth to the exact bore bottom at Z = 13 |
| 6b.2 Cut-off path | Wire EDM cut along X at Z = 11.5 (mid-support), 5 mm lead-in and lead-out | Wire path length 100 + 2 × 5 = **110 mm**; minimum distance from wire to any part = 13 − 11.5 = **1.5 mm** (exact) |
| 6b.3 Upstream change | Supports 3 → 4 mm | Supports, slices, as-built body, drilling and cut-off **stale** (5 items); after regeneration bore bottom Z = **14**, part bottoms Z = 14, cut-off clearance at Z = 11.5 is **2.5 mm** |
| 6b.4 Unrelated design edit | Cut a notch 2 mm wide, 2 mm deep, 2 mm high into one side face of the variant, 5 to 7 mm above the part bottom. Drilling region: the bore axis ± 4 mm in X and Y | Notch spans X ∈ [8, 10] in part coordinates, outside \|x\| ≤ 4: drilling **up to date**. Notch is above the bottom: supports **up to date**. A face-mill op on the tops **up to date**. Only the slices between Z = 13 + 5 = **18** and 13 + 7 = **20** change, so the slice stack is **stale**: 1 item |
| 6b.5 Release | Release "R1" before 6b.3, then apply 6b.3 | `manufacture.release` status lists exactly the **5** stale items of 6b.3 as outdated; R1's recorded stamps are unchanged |
| 6b.6 Lineage | Post the drilling after regeneration | The program header carries the design id, the current revision and the item's stamp; `manufacture.trace` on that stamp returns the drilling item and the current revision |
| 6b.7 Cut-off part body | Cut-off at Z = 11.5 as 6b.2; next setup takes the cut-off part body as stock | Support stub below each part = 13 − 11.5 = **1.5 mm**; a facing op that removes the stubs reads **1.5 mm** of stock below the part bottom from the exact in-process body |
| 6b.8 Swap order | Plan reordered: cut-off first, then drilling in a fixture | Same 4 bores at the same part-local positions (0, 0); drilling's footprint now reads the cut-off part body, so editing support height (6b.3) leaves drilling **up to date** in this order and **stale** in the on-plate order |
| 6b.9 Plan check | After 6b.1, 6b.2 and a stub-removal facing | Remaining stock on the bore = 0; on the 0.5 mm allowance faces before finishing = **0.5 mm**; after the finishing op **0 mm**; release allowed only at 0 |

### Phase 7: Standalone and browser paths

| | |
|---|---|
| Deliverable | `.craftx` digests and plugin parts; standalone CamCraft computing staleness with `craft-changeset`; browser hand-off |
| Owner | SolidCraft and CamCraft |
| Acceptance | For the edits of tests 1.2 to 1.8, the change set and the item statuses computed offline from two `.craftx` revisions equal the in-app results exactly. |

---

## 9. Risks and open questions

### 9.1 Risks

| Risk | Mitigation |
|---|---|
| **Spurious "stale"** from bit-level noise when an unrelated edit re-evaluates a body | Accepted by design (safe direction). Phase 1 measures the rate on the invented parts; if it is high, the fix is in the kernel's determinism, never in looser comparison. |
| **Generator non-determinism** (thread scheduling, hash-map order, floating-point reassociation) | Test 1.10 fails loudly; plugins must produce byte-identical output for one footprint. |
| **Performance of exact reads across the seam** | Batch calls over buffers; region recording instead of per-call recording; measured with the method in SolidCraft's performance rules before any claim. |
| **CamCraft's port is large** | The plugin grows one operation family at a time; the standalone app keeps every feature meanwhile. |
| **Web build** | Every capability ships with its web path at the same time (RFC 0015 rule); jobs use web workers. |
| **Influence bounds on curved faces** are a new kernel primitive | One implementation, in solidcore, with its own tests; until it lands, curved faces use their whole exact bounds (more false "stale", never fewer true ones). |
| **Patents** in support generation and distortion compensation | Clean-room design does not answer patent questions. Each such feature records its public sources; the Apache-2.0 option gives contributors' patent licences; the maintainers may want advice before Phase 6. |
| **Scope creep** (five-axis, turning, other additive processes) | Out of scope for this RFC; each needs its own. |

### 9.2 Open questions for the SolidCraft maintainer

1. Is Manufacture acceptable as a **host** workspace in the switcher next to Design, Drawing and Render, with plugins as its processes? RFC 0015's open question 1 placed plugin workspaces either beside the built-ins or under "More"; this proposal asks for a third shape.
2. Should the host compute region digests and influence bounds, or would you rather the first version used whole-face digests and accept more false "stale"?
3. Is `geometry@0.1` the right shape, or should exact reads stay plain engine commands (`model.faces` and new siblings) called through `run`, with the host recording them the same way?
4. Should regenerated results be undo steps? This proposal says no (derived data, like the model cache).
5. May `craft-changeset` start in this repository, under the same rule that keeps `craft-plugin` free of SolidCraft dependencies?
6. Which body identity should references use long-term: component path plus body name with remapping, or a new permanent body id?

### 9.3 Open questions for the CamCraft maintainer

1. Would you accept a `camcraft-plugin` crate with no Flecs dependency, built from the existing ECS-free crates, rather than compiling the application layer into the component?
2. Are you willing to make SolidCraft references the primary identity of picks on SolidCraft geometry, with facet hashes kept for meshes CamCraft imports itself?
3. Which operation families should come first after 2.5D: 3D roughing and finishing, or turning?
4. Should the host's stamp replace CamCraft's own operation digest inside the plugin, or should both run, with CamCraft's as a cross-check?
5. Where should posts that users write live, so that a post is an export slot SolidCraft can approve like any other plugin capability?
6. What is the timing of your solidcore milestone, and does the plugin build drop the pinned solidcore fork in favour of the host's geometry reads?
7. Can a setup take a published as-built plate body (exact parts and plate, mesh supports) as both stock and fixture, and can the wire EDM generators run a cut-off on it? Which machine-side constraints (wire tilt, start holes, plate clamping) should the first version model?

### 9.4 Questions for both

1. Where should the additive repository live, and who are its first reviewers?
2. Do you agree that the manufacturing capabilities move to 1.0 only after both CamCraft and PowderCraft have shipped against them?

---

## 10. Sources

Consulted to understand which capabilities exist and why. Nothing is copied from any of them; behaviour above is described in my own words.

**Project documents read (read-only)**
- tangency-labs/solidcraft: `AGENTS.md`; `docs/architecture.md`; `docs/layers.md`; `docs/rfcs/0001-craft-bridge.md`, `0002-plugins.md`, `0015-plugin-ui-workspaces.md`, `README.md`; `.agents/skills/` (commands, file-formats, kernel-rules, layers, performance, ui, packaging); `crates/doc/src/history_names.rs`, `eval.rs`, `feature_refs.rs`, `format/pending/README.md`; `crates/craft-plugin` (`lib.rs`, `wit/plugin.wit`); `crates/craft-plugin-wasm/README.md`; `crates/ui-egui/src/workspace.rs`, `plugin_workspaces.rs`; `docs/control-protocol.md`; `docs/formats/3mf-face-tags.md`; `LICENSE-BRAND`.
- NeoTech/Camcraft: `AGENTS.md` (clean-room protocol, prime directive, everything is a command, dependencies, code shape); crate manifests; `crates/app/src/operation/stamp.rs`, `operation/types/sel.rs`, `schema.rs`, `model_source.rs`, `source_faces.rs`; `crates/geometry/src/solidcraft.rs`, `source_follow.rs`; `crates/kernel/src/lib.rs`; `web/build-app-wasm.sh`.

**Persistent naming and associativity**
- D. Marcheix, G. Pierra, "A survey of the persistent naming problem", ACM Symposium on Solid Modeling and Applications, 2002. doi:10.1145/566282.566288
- S. H. Farjana, S. Han, "Mechanisms of persistent identification of topological entities in CAD systems: a review", Alexandria Engineering Journal 57(4), 2018, 2837–2849. https://doaj.org/article/bf42fdae16584d0ba2d824137aba6599
- J. Kripac, "A mechanism for persistently naming topological entities in history-based parametric solid models", Computer-Aided Design 29(2), 1997, 113–122.
- Y. Wang, B. O. Nnaji, "Geometry-based semantic ID for persistent and interoperable reference in feature-based parametric modeling", Computer-Aided Design 37(10), 2005, 1081–1093.
- D. Marcheix, "A persistent naming of shells", International Journal of CAD/CAM 6(1), 2006. https://www.lias-lab.fr/publications/7550/2006-CAD-CAM-Marcheix.pdf
- D. Mun, S. Han, "Identification of topological entities and naming mapping for parametric CAD model exchanges". https://dspace.kaist.ac.kr/bitstream/10203/6600/1/Identification%20of%20Topological%20Entities%20and%20Naming.pdf
- Automatic update and propagation of machining process models after modification, Int J Adv Manuf Technol (2015). doi:10.1007/s00170-015-7371-0

**Incremental and background recomputation**
- A. Mokhov, N. Mitchell, S. Peyton Jones, "Build systems à la carte: Theory and practice", Journal of Functional Programming 30, 2020. doi:10.1017/S0956796820000088
- Salsa incremental computation framework, red-green algorithm and durability: https://docs.rs/salsa/ and https://rust-analyzer.github.io/blog/2023/07/24/durable-incrementality.html

**Manufacturing data models**
- ISO 14649-10:2004, data model for computerized numerical controllers, general process data (workpiece, features, executables). https://www.iso.org/standard/40895.html
- STEP-NC overview. https://en.wikipedia.org/wiki/STEP-NC

**Additive manufacturing**
- ISO/ASTM 52900:2021, Additive manufacturing — General principles — Fundamentals and vocabulary. https://www.iso.org/standard/74514.html
- ISO/ASTM 52910:2018, Additive manufacturing — Design — Requirements, guidelines and recommendations. https://www.iso.org/standard/67289.html
- ISO/ASTM 52911-2:2019, Design — Part 2: Laser-based powder bed fusion of polymers. https://www.iso.org/standard/72952.html
- ISO/ASTM 52911-1:2019, Design — Part 1: Laser-based powder bed fusion of metals.
- 3MF Core Specification 1.3.0. https://3mf.io/spec/core
- 3MF Production Extension 1.1.2. https://github.com/3MFConsortium/spec_production/blob/1.1.2/3MF%20Production%20Extension.md
- 3MF Slice Extension 1.0.2. https://3mf.io/?p=2168
- Toward better build volume packing in additive manufacturing: classification of existing problems and benchmarks. https://nottingham-repository.worktribe.com/OutputFile/759193
- Deepest-bottom-left-fill based methods for AM build volume packing, International Journal of Production Research. https://people.cs.nott.ac.uk/pszeo/docs/publications/DBLF_AM.pdf
- V. Griffiths, J. P. Scanlan, M. H. Eres, "Cost-driven build orientation and bin packing of parts in selective laser melting", European Journal of Operational Research. https://eprints.soton.ac.uk/416924/2/corrected_ejor_paper.pdf
- A. Arndt, H. Hackbusch, R. Anderl, "An algorithm-based method for process-specific three-dimensional nesting for additive manufacturing processes", SFF Symposium 2015. https://utw10945.utweb.utexas.edu/sites/default/files/2015/2015-109-Arndt.pdf
- B. Rankouhi, D. J. Thoma, K. Suresh, "Support structure design for selective laser melting process" (overhang angle is material-dependent, about 45° for 316L). https://ersl.wisc.edu/publications/2019/SupportStructureBookChapter.pdf
- S. C. Subedi, D. J. Thoma, K. Suresh, "Deformation constrained support-structure optimization for laser powder bed fusion". https://par.nsf.gov/servlets/purl/10519490
- Q. Han et al., "Manufacturability of AlSi10Mg overhang structures fabricated by laser powder bed fusion", Materials and Design 160, 2018. https://core.ac.uk/download/323906976.pdf
- Support-free metal additive manufacturing: a structured review. https://www.cambridge.org/core/services/aop-cambridge-core/content/view/3969D4E6D526FCCB38B78AC0990EA2DF/S2732527X21005423a.pdf
- W. Dong et al., "Data-driven distortion compensation for laser powder bed fusion process using Gaussian process regression and inherent strain method", Materials & Design, 2024. https://doaj.org/article/802aa177e1ac48d98abc94b8746df875
- A new variant of the inherent strain method for the prediction of distortion in powder bed fusion, Int J Adv Manuf Technol, 2024. doi:10.1007/s00170-024-13255-x
- Right-first-time distortion compensation in laser powder bed fusion of a thin nickel-alloy 718 manifold, Journal of Manufacturing Processes 69. https://irep.ntu.ac.uk/id/eprint/44079
- Thermal history and cooling in polymer laser sintering builds (packing density and job height). https://utw10945.utweb.utexas.edu/sites/default/files/2014-006-Josupeit.pdf

**Governance, versioning and licences**
- Rust RFC process and final comment period. https://rust-lang.github.io/rfcs/ and https://lang-team.rust-lang.org/fcp-decisions.html
- WIT `@since`, `@unstable` and `@deprecated` gates. https://github.com/WebAssembly/component-model/pull/332 and https://bytecodealliance.org/articles/WASI-0.2.1
- ASF third-party licence policy (Category A). https://apache.org/legal/resolved
- Apache License 2.0 and GPL compatibility. https://apache.org/licenses/GPL-compatibility.html
