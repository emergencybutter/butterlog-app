# Cross-Simulator AI Model & Livery Matching

## Problem

ButterLog multiplayer is peer-to-peer. Each player runs **MSFS** or **X-Plane**, and
when a player flies, their aircraft identity + telemetry is broadcast to peers. Every
receiving peer must render that aircraft as injected/AI traffic using **whatever models
its own simulator has installed**.

The hard part: sender and receiver may be different sims with **disjoint model
libraries and incompatible identity vocabularies**:

- MSFS describes aircraft by installed **title** (e.g. `"Boeing 747-8i United"`) plus
  ATC strings; liveries are usually *separate installed titles* but sometimes there is a livery name too. Worse, these are free form string and there is no rigorous way to extract the type and the airline.
- X-Plane describes aircraft by **CSL `package_name` + `model_id`**, `model_id` is of the format `{icao_type_designator}_{airline_icao}`.

Today there are two independent matchers with duplicated, drifting heuristics:
`find_best_multiplayer_model` in the app (MSFS path) and the CSL map cascade in
`butterlog_xp_plugin` (X-Plane path).

## Core principle

**Send a sim-neutral canonical identity; let each receiver own model + livery
selection.** Only the receiver knows what it has installed, so the sender's job is to
describe *what the aircraft is* as portably as possible — never to name a model the
receiver might not have.

Two corollaries that drive the wire format:

1. **The wire carries identity, not derived attributes.** Anything that is a pure
   function of the aircraft type (WTC, manufacturer, …) is *derived by the receiver from
   its own reference table*, never sent. This avoids cross-peer table-version skew: if
   peer A's table says `B738 = Medium` and peer B's says `Heavy`, we want B to match
   against *its own* notion of the type so its behavior stays self-consistent with its
   own library.
2. **Livery paths/names are not portable; airline codes are.** Normalize airline →
   ICAO code on the sender. The raw livery string is kept only as a *same-sim* hint.

## Wire format (versioned)

```jsonc
{
  "v": 2,                          // schema version — P2P has mixed client versions
  "sim": "msfs" | "xplane",        // enables a same-sim exact-match fast path
  "native_model": "<title|csl_model_id>",   // used ONLY for the same-sim tier
  "registration": "N12345",        // tail; soft signal

  // --- portable type identity (primary key) ---
  "type_icao": "B738",             // deduced from MSFS title / XP acf_ui_name or acf_ICAO, validated

  // --- fallback descriptor: populated ONLY when type_icao is empty.
  // For now we assume the ICAO databases are in sync.
  "raw": {
    "category": "airplane|helicopter|glider",
    "engine_type": "jet|turboprop|piston|...",
    "num_engines": 2
  },

  // --- portable livery identity ---
  "livery": {                       // optional, the sender may not have been able to determine a known livery
   "airline_icao": "UAL",
   "livery_variant": "std",         // optional normalized scheme tag (see "Livery variants")
   "livery_era": 2019,              // optional refresh year the scheme belongs to
   "livery_hint": "<raw sim livery>",   // same-sim only, never a cross-sim key
  },

  "metrics": { /* telemetry */ }
}
```

### What is NOT on the wire (and why)

| Field | Why it's omitted |
|-------|------------------|
| `wtc` | Pure function of `type_icao`; receiver derives from its own table. Sending it risks cross-peer skew. When ICAO is unknown it can be approximated locally from `engine_type`+`num_engines`. |
| `manufacturer` | Derivable from `type_icao`; only used for family/similarity matching, which the receiver does against its own table. |

### `type_icao` and `airline_icao` determination

type_icao and airline_icao are determined using heuristics from the aircraft title and livery name for msfs. and the ui name and livery path for xplane.
Note that this heuristics applies to both the user's aircraft but also installed models available to render other players.

### The `raw` fallback block

`raw` exists **only** for the cases where `type_icao` cannot be resolved:

In those cases the receiver has nothing to infer from, and these **sim-measured**
primitives are the only signal for the similarity fallback. They are direct readings,
independent of any ICAO lookup, so they cannot be reconstructed receiver-side:

- `category` ← MSFS `AIRCRAFT OBJECT CLASS` / X-Plane `acf_class`
- `engine_type` ← MSFS `ENGINE TYPE` / X-Plane `acf_en_type`
- `num_engines` ← MSFS `NUMBER OF ENGINES` / X-Plane `acf_num_engines`

### Matcher rule for the descriptor

- `type_icao` **known and on the wire** → derive all attributes (`wtc`, `manufacturer`, `category`,
  `engine_type`, `num_engines`) from the **local** reference table; **`raw` is absent from the protocol**.
- `type_icao` **empty/unknown** → fall back to `raw.category` (to gate
  heli/plane/glider) plus `raw.engine_type` / `raw.num_engines` for similarity scoring.

### Versioning

Because this is peer-to-peer with potentially mixed client versions, the payload is
versioned (`v`). Unknown fields are ignored; missing fields are treated as empty and
simply drop the match to a lower tier. No flag-day upgrade is required.

## Unified graded matcher (shared crate)

Extract one `model_matching` crate used by **both** the MSFS app path and the X-Plane
plugin, parameterized by a `LocalLibrary` trait:

```rust
trait LocalLibrary {
    fn sim(&self) -> Sim;
    fn has_native(&self, id: &str) -> Option<LocalModel>;        // tier 0
    fn models_for_type(&self, type_icao: &str) -> Vec<LocalModel>;
    fn liveries_for(&self, model: &LocalModel) -> Vec<Livery>;   // airline-keyed
    fn all_models(&self) -> &[LocalModel];                       // similarity scan
}

fn resolve(id: &CanonicalAircraftId, lib: &impl LocalLibrary, refs: &RefTables)
    -> Match { model, livery, tier, score }
```

### Tier ladder

Highest → lowest confidence. **Gate on `category` first** — never cross
airplane/helicopter/glider.

| Tier | Match on | Confidence |
|------|----------|------------|
| 0 | same `sim` + exact `native_model` → original `livery_hint` | exact |
| 1 | `type_icao` + `airline_icao` + `livery_hint` | exact livery |
| 2 | `type_icao` + `airline_icao` | right plane, right airline |
| 3 | `type_icao` exact | right plane, default/regional livery |
| 4 | type **family** (A319/320/321; 737 variants) | close visual match |
| 5 | characteristics similarity (`wtc` + `engine_type` + `num_engines`) | size/class match |
| 6 | per-category **size-class default** (narrowbody/widebody/GA/heli) | bucketed |
| 7 | guaranteed-present absolute fallback | last resort |

Livery is selected orthogonally once the model is fixed — see "Livery variants" below.

This replaces both existing matchers with one tested implementation. The plugin's
`livery_map` / `airline_map` / `icao_map` cascade becomes the `LocalLibrary` impl for
X-Plane; the enumerated `available_aircraft` titles become the impl for MSFS.

### Livery variants

An airline is not one livery. It typically has:

- a **standard current** scheme,
- one or more **legacy / refresh** schemes introduced in a given year (e.g. United's
  pre-2019 "Globe" vs. 2019 "Evolve"),
- **special** one-offs: retro throwbacks, alliance schemes (Star Alliance, oneworld),
  anniversary, sports-team, charity/pride liveries.

So `airline_icao` alone is too coarse. We classify each livery into a **normalized
variant**:

```
livery_variant := "std" | "legacy" | "retro" | "alliance:<name>"
                | "anniversary" | "special:<slug>"   // e.g. special:starwars
livery_era     := optional refresh year (e.g. 2019)
```

The crucial mechanism for portability: **both** the incoming canonical descriptor **and**
the receiver's *installed* liveries are run through the **same** livery resolver, tagging
each with `(airline_icao, livery_variant, livery_era)`. Matching then compares **tags,
not raw strings**, so it works even though the underlying livery names/paths differ
between installations and sims.

**Livery selection sub-tiers** (applied once the model is chosen; gated on
`airline_icao`):

| Sub-tier | Match on | Notes |
|----------|----------|-------|
| L0 | same-sim exact `livery_hint` | tier-0 fast path only |
| L1 | `airline_icao` + exact `livery_variant` (+ `special:<slug>`) | the requested scheme is installed |
| L2 | `airline_icao` + closest `livery_era` | scheme in effect at that year: latest `valid_from ≤ requested era` |
| L3 | `airline_icao` + `variant = std` (current) | a special/legacy livery not installed degrades to the airline's standard colors |
| L4 | `airline_icao`, any variant | wrong scheme but right airline |
| L5 | registration-country regional default → blank | no airline match at all |

This makes the "special livery not present on the receiver" case explicit and graceful:
it falls L1 → L3 to the airline's standard livery rather than to a generic/blank one.
Year-based refreshes are handled by L2 using each scheme's `valid_from` so a peer flying
an older era gets the closest legacy scheme the receiver actually has.

## The MSFS-specific lever

MSFS `ai_create_non_atc_aircraft_ex1` takes *title* and optional *livery*. These should come from the result of SimConnect_EnumerateSimObjectsAndLiveries.

## Shared reference tables (single source of truth)

Generated once and shipped to both the app and the plugin (extends the existing
`aircraft-characteristics.csv`, currently duplicated in both):

1. **Type table**: `type_icao → {manufacturer, wtc, engine_type, num_engines, category,
   family_id, size_class}`, plus human model names (`Model_FAA` / `Model_BADA`) for the
   resolver's title scoring.
2. **Title → ICAO alias table**: curated mappings from sim title fragments to ICAO
   designators (e.g. `"747-8i" → B748`), for resolver step 2.
3. **Airline table**: `airline_icao ↔ iata ↔ name ↔ aliases ↔ callsign` — for
   normalizing MSFS `ATC AIRLINE` / livery / title tokens (names) → code, and
   callsign-prefix → code.
4. **Livery catalog**: `(airline_icao, scheme) → {livery_variant, valid_from year,
   is_current, keywords/aliases}` — classifies a raw livery string into a normalized
   variant, and tells the receiver which installed liveries are current vs. legacy for
   the era fallback (L2/L3).
5. **Family / size-class table**: groups for tier 4 and tier 6.

Tables 1–4 also back the Identity Resolver; keeping them shared means resolution and
matching agree on the same vocabulary.

## Sender normalization

The sender populates the canonical identity. The directly-readable, sim-native fields
are trivial:

- **MSFS**: `registration` ← `ATC ID`; `raw.*` ← `OBJECT CLASS` / `ENGINE TYPE` /
  `NUMBER OF ENGINES`.
- **X-Plane**: `livery_hint` ← `acf_livery_path`; `raw.*` ← `acf_class` / `acf_en_type`
  / `acf_num_engines`.

The portable identity fields — `type_icao` and `airline_icao` — are **not** reliably
available as clean values from the sim. They are produced by the **Identity Resolver**
(next section), which extracts and validates them from the raw sim identifiers.

## Identity resolution from raw sim identifiers

The explicit sim fields that *should* carry ICAO type and airline are unreliable:

- MSFS `ATC MODEL` is frequently empty or wrong on add-on aircraft; `ATC AIRLINE` is
  often blank. The real identity is buried in the free-text **title**
  (e.g. `"Boeing 737 MAX 8 Ryanair"`) and, when present, the livery name.
- X-Plane `acf_ICAO` is often blank or garbage on custom aircraft; the airline, if
  anywhere, is encoded in the **livery path** folder name
  (e.g. `".../Boeing B738/United/"`).

So we need a dedicated **Identity Resolver**: given the raw sim identifiers, produce a
best-effort `type_icao` and airline identity, each with a confidence and a recorded
source. It lives in the shared `model_matching` crate (module `identity_resolver`) so its
alias tables are shared and unit-tested, and so a receiver can re-run it as a fallback
(see "Where it runs").

### Inputs

| | MSFS | X-Plane |
|---|------|---------|
| title (free text) | `TITLE` | `acf_ui_name` |
| claimed type | `ATC MODEL` | `acf_ICAO` |
| manufacturer | `ATC TYPE` | — |
| airline (free text) | `ATC AIRLINE` | — |
| livery string | livery title (if separate) | `acf_livery_path` |
| registration / callsign | `ATC ID` / flightplan callsign | tail / callsign |

### ICAO type resolution (cascade, highest confidence first)

1. **Trust the explicit field if valid.** If the claimed type (`ATC MODEL` / `acf_ICAO`)
   is a non-empty, well-formed ICAO designator **present in the type table**, use it.
   Validation against the table is what rejects garbage/empty values.
2. **Title → ICAO via alias table.** Normalize the title (lowercase, strip punctuation),
   then match against a curated **title→ICAO alias table** (e.g. `"747-8i" → B748`,
   `"737 max 8" → B38M`).
3. **Manufacturer + model-token scoring.** Extract manufacturer (from `ATC TYPE` or the
   leading title token) and model-number tokens, and fuzzy-score against the type table's
   human model names (`Model_FAA` / `Model_BADA`, already in `aircraft-characteristics.csv`).
4. **Give up cleanly.** If no confident ICAO is found, leave `type_icao` **empty** so the
   matcher falls back to the `raw` descriptor. The resolver and the `raw` block are
   complementary: the resolver tries hard for a clean ICAO; `raw` is the safety net.

### Airline resolution (cascade, highest confidence first)

1. **Airline name → `airline_icao`.** Match `ATC AIRLINE` (MSFS) against the airline
   table's names/aliases.
2. **Livery string → airline.** Parse the livery path / livery title for an airline token
   and match against airline names/aliases (X-Plane primary; MSFS when liveries are
   separate titles).
3. **Title → airline.** Trailing tokens of MSFS titles (`"... United"`) matched against
   airline names — works because MSFS liveries are encoded as titles.
4. **Callsign prefix → `airline_icao`.** First three letters of a flightplan callsign,
   validated against the airline table (formalizes the existing first-3-of-id parse).
5. **Registration → country** (soft signal only — feeds a regional *default* livery, not
   an airline identity).

Take the highest-confidence candidate; record the source for observability.

### Livery variant classification

After the airline is known, the resolver classifies the *variant* from the remaining
livery tokens (livery path / livery title), producing `livery_variant` + optional
`livery_era`:

1. **Keyword/alias match** against the **livery catalog** for that `airline_icao`
   (e.g. `"evolve" → (std, 2019)`, `"globe" → (legacy, 1998)`, `"star alliance" →
   alliance:staralliance`, `"retro"/"heritage" → retro`).
2. **Year extraction**: a 4-digit token, or relative words (`"old"/"new"`), set
   `livery_era`.
3. **Special detection**: tokens like team names, `"anniversary"`, `"special"`,
   `"pride"` → `special:<slug>` (slug from the normalized token).
4. **Default**: no signal → `variant = std`, `era` unset.

The receiver runs this **same** classifier over its installed liveries so both sides
speak the same `(airline_icao, variant, era)` vocabulary.

### Where it runs

Primary: **sender-side**, so the wire carries clean canonical identity. But because the
payload also carries `native_model` and `livery_hint`, a **newer receiver can re-run the
resolver** when canonical fields arrive empty (e.g. from an older sender that only sent a
raw title) — graceful degradation across client versions.

## Where matching runs

Keep the current split but unify the algorithm behind the shared crate:

- **MSFS** (app): resolves canonical → installed title using the enumerated
  `available_aircraft` list + characteristics table. Livery collapses into title scoring
  (see lever above).
- **X-Plane** (plugin): the app forwards the canonical identity in the UDP packet to
  `127.0.0.1:49020`; the plugin resolves canonical → CSL `model_id` + livery via its
  maps.

## Observability

`resolve()` returns `tier` + `score`. Log it at spawn (spawns are already logged) and
surface it on `TrackedAircraftDebugInfo`. Aggregate misses (e.g. "ICAO X has no local
model, fell to tier 6") to curate the family/default tables over time — turning match
quality from a guess into something measurable.

## Cross-sim livery: honest limits

MSFS↔X-Plane livery is fundamentally lossy. Strategy: never rely on `livery_hint`
cross-sim (paths/names won't line up). Rely on `airline_icao` (portable); if it can't be
derived, drop to a clean default livery rather than guessing. This is documented,
expected behavior.

## Rollout (incremental, each step shippable)

1. Build the type / title-alias / airline / livery-catalog / family reference tables.
2. Extract the `model_matching` crate: `CanonicalAircraftId`, `LocalLibrary` trait, tier
   ladder + livery sub-tiers, scoring, unit tests (seed from existing characteristics
   tests in both repos).
3. Build the `identity_resolver` module (ICAO + airline + livery-variant cascades) with
   unit tests over a corpus of real MSFS titles and X-Plane livery paths. The variant
   classifier is shared for inbound descriptors and the receiver's installed liveries.
4. Sender: run the resolver, compute canonical identity, ship versioned `v:2` payload
   (peers still understand `v:1`).
5. Receivers: implement `LocalLibrary` for MSFS (titles) and X-Plane (CSL maps); route
   both through the shared matcher. Keep the old matchers behind it until parity is
   proven. Re-run the resolver receiver-side when canonical fields are empty.
6. Add tier/score + resolver-source logging; iterate on the reference/alias tables from
   real misses.
