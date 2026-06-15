//! Word → ICAO-type index for resolving an ICAO aircraft type designator from a free
//! string (e.g. a sim aircraft title or livery name like `"Asobo Boeing 737-800"`).
//!
//! The index is an inverted map `token -> [(icao, weight)]` built from the aircraft
//! characteristics database, scored with IDF so distinctive tokens (model numbers like
//! `738`) dominate over common ones (manufacturer names like `boeing`).
//!
//! Keyword sources, in decreasing trust:
//!   1. The `ICAO_Code` itself — an exact identifier, so its tokens carry the most weight
//!      (a free string containing `B738` resolves directly).
//!   2. `Model_FAA` and `Model_BADA` — the human model names.
//!   3. `Manufacturer` — brand, the least discriminating on its own.
//!   4. A small curated alias/nickname table (manufacturer spelling variants and common
//!      type nicknames) — the obvious extension point for hand-tuning.

use std::collections::{HashMap, HashSet};

use crate::aircraft_characteristics::CharacteristicsDatabase;

/// Per-field token weights. Identifier-like tokens are far more discriminating than
/// brand/model words, so they are weighted up.
const W_ICAO: f32 = 4.0;
const W_MODEL: f32 = 1.0;
const W_MFG: f32 = 0.6;
const W_ALIAS: f32 = 3.0;

/// Token-level synonyms applied to both the index and the query, so different spellings
/// of the same brand collapse together. Keep entries that are unambiguous.
const ALIASES: &[(&str, &str)] = &[
    ("beechcraft", "beech"),
    ("mcdonnelldouglas", "mcdonnell"),
    ("aerospatiale", "airbus"),
    ("eurocopter", "airbus"),
];

/// Curated marketing nicknames that do not appear in the characteristics columns,
/// mapped to an ICAO code. Only applied when the code exists in the loaded database.
const NICKNAMES: &[(&str, &str)] = &[
    ("jumbo jet", "B744"),
    ("dreamliner", "B788"),
    ("super hornet", "F18"),
    ("warthog", "A10"),
    ("Long-EZ", "LGEZ"),
    ("Fox2", "FOX"),
    ("UH-1H", "UH1"),
    ("M500", "P46T"),
    ("Sting", "TL20"),
    ("Vision", "SF50"),
];

/// Third-party add-on developer / studio names (MSFS & X-Plane) that frequently appear in
/// aircraft titles and livery paths but identify neither the aircraft type nor the operating
/// airline. Dropped during tokenization for both the ICAO-type and airline indexes.
pub const ADDON_DEVELOPERS: &[&str] = &[
    "ifly", "blacksquare", "fsreborn", "pmdg", "fenix", "flightsimware",
    "asobo", "fsltl", "passiveaircraft", "laminar",
];

/// Shortest ICAO code length considered when matching a code as the prefix of a word.
const MIN_CODE_PREFIX_LEN: usize = 3;

struct Posting {
    icao: u32,
    weight: f32,
}

/// A scored ICAO-type candidate for a query.
#[derive(Debug, Clone)]
pub struct IcaoMatch {
    pub icao: String,
    pub score: f32,
}

pub struct IcaoIndex {
    icaos: Vec<String>,
    postings: HashMap<String, Vec<Posting>>,
    idf: HashMap<String, f32>,
    /// Lowercased set of all ICAO codes, for prefix matching against query words.
    code_set: HashSet<String>,
}

/// Split a free string into normalized tokens: lowercased, broken on any non-alphanumeric
/// character, applying alias canonicalization. Pure single-letter tokens are dropped as
/// noise, but short tokens containing a digit (e.g. `8`, `a320`) are kept.
fn tokenize(s: &str) -> Vec<String> {
    s.to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|t| !t.is_empty())
        .map(canonicalize)
        .filter(|t| t.len() >= 2 || t.chars().any(|c| c.is_ascii_digit()))
        .filter(|t| !ADDON_DEVELOPERS.contains(&t.as_str()))
        .collect()
}

fn canonicalize(token: impl Into<String>) -> String {
    let token = token.into();
    ALIASES
        .iter()
        .find(|(from, _)| *from == token)
        .map(|(_, to)| to.to_string())
        .unwrap_or(token)
}

/// Add every token of `text` to `into`, keeping the strongest weight when a token shows
/// up via several fields.
fn add_tokens(into: &mut HashMap<String, f32>, text: &str, weight: f32) {
    for token in tokenize(text) {
        into.entry(token)
            .and_modify(|w| *w = w.max(weight))
            .or_insert(weight);
    }
}

impl IcaoIndex {
    /// Build the index from an already-loaded characteristics database.
    pub fn build(db: &CharacteristicsDatabase) -> Self {
        let mut icaos: Vec<String> = Vec::with_capacity(db.characteristics.len());
        let mut per_icao: Vec<HashMap<String, f32>> = Vec::with_capacity(db.characteristics.len());
        let mut by_code: HashMap<String, usize> = HashMap::new();

        for c in db.characteristics.values() {
            if c.icao_code.is_empty() {
                continue;
            }
            let idx = icaos.len();
            by_code.insert(c.icao_code.to_lowercase(), idx);
            icaos.push(c.icao_code.clone());

            let mut tokens = HashMap::new();
            add_tokens(&mut tokens, &c.icao_code, W_ICAO);
            add_tokens(&mut tokens, &c.manufacturer, W_MFG);
            add_tokens(&mut tokens, &c.model_faa, W_MODEL);
            add_tokens(&mut tokens, &c.model_bada, W_MODEL);
            per_icao.push(tokens);
        }

        // Other source: curated nicknames, applied only when the target code exists.
        for (nick, code) in NICKNAMES {
            if let Some(&idx) = by_code.get(&code.to_lowercase()) {
                add_tokens(&mut per_icao[idx], nick, W_ALIAS);
            }
        }

        // Invert into postings.
        let mut postings: HashMap<String, Vec<Posting>> = HashMap::new();
        for (idx, tokens) in per_icao.iter().enumerate() {
            for (token, weight) in tokens {
                postings.entry(token.clone()).or_default().push(Posting {
                    icao: idx as u32,
                    weight: *weight,
                });
            }
        }

        // IDF: tokens shared by few types are the most informative.
        let n = icaos.len().max(1) as f32;
        let idf = postings
            .iter()
            .map(|(token, plist)| (token.clone(), (1.0 + n / plist.len() as f32).ln()))
            .collect();

        let code_set: HashSet<String> = by_code.keys().cloned().collect();

        Self { icaos, postings, idf, code_set }
    }

    /// Accumulate IDF-weighted postings for one token, counting each token at most once.
    fn accumulate(&self, token: &str, scores: &mut HashMap<u32, f32>, seen: &mut HashSet<String>) {
        if !seen.insert(token.to_string()) {
            return;
        }
        if let (Some(plist), Some(&idf)) = (self.postings.get(token), self.idf.get(token)) {
            for posting in plist {
                *scores.entry(posting.icao).or_insert(0.0) += idf * posting.weight;
            }
        }
    }

    /// If an ICAO code is an exact, *proper* prefix of `token` and the trailing suffix
    /// does not contain a `neo`/`max` variant marker, return that code (the longest such).
    /// e.g. `b738w` -> `b738`, `dr400` -> `dr40`; but `a320neo` / `b737max` -> `None`.
    fn code_prefix_of(&self, token: &str) -> Option<String> {
        if !token.is_ascii() {
            return None;
        }
        let n = token.len();
        // Code length `i` is a proper prefix (suffix non-empty), longest first.
        for i in (MIN_CODE_PREFIX_LEN..n).rev() {
            let prefix = &token[..i];
            if self.code_set.contains(prefix) {
                let suffix = &token[i..];
                if suffix.contains("neo") || suffix.contains("max") {
                    return None;
                }
                return Some(prefix.to_string());
            }
        }
        None
    }

    /// Return up to `limit` ICAO candidates for `query`, best score first.
    pub fn candidates(&self, query: &str, limit: usize) -> Vec<IcaoMatch> {
        let mut scores: HashMap<u32, f32> = HashMap::new();
        let mut seen: HashSet<String> = HashSet::new();

        for token in tokenize(query) {
            self.accumulate(&token, &mut scores, &mut seen);
            // A type code that is an exact prefix of the word (with a non-neo/max suffix)
            // implies that exact type — credit it as if the code appeared verbatim.
            if let Some(code) = self.code_prefix_of(&token) {
                self.accumulate(&code, &mut scores, &mut seen);
            }
        }

        let mut matches: Vec<IcaoMatch> = scores
            .into_iter()
            .map(|(idx, score)| IcaoMatch {
                icao: self.icaos[idx as usize].clone(),
                score,
            })
            .collect();
        matches.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
                // Stable tiebreak so equal scores are deterministic.
                .then_with(|| a.icao.cmp(&b.icao))
        });
        matches.truncate(limit);
        matches
    }

    /// Best single ICAO match for `query`, or `None` if nothing matched.
    pub fn find(&self, query: &str) -> Option<IcaoMatch> {
        self.candidates(query, 1).into_iter().next()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::aircraft_characteristics::AircraftCharacteristic;

    fn ac(icao: &str, mfg: &str, faa: &str, bada: &str) -> AircraftCharacteristic {
        AircraftCharacteristic {
            icao_code: icao.to_string(),
            manufacturer: mfg.to_string(),
            model_faa: faa.to_string(),
            model_bada: bada.to_string(),
            engine_type: String::new(),
            num_engines: 0,
            wtc: String::new(),
            class: String::new(),
        }
    }

    fn test_db() -> CharacteristicsDatabase {
        let rows = [
            ac("A320", "AIRBUS", "Airbus A320", "Airbus A320-231"),
            ac("A321", "AIRBUS", "Airbus A321", "Airbus A321-231"),
            ac("B738", "BOEING", "Boeing 737-800", "Boeing B737-800"),
            ac("B737", "BOEING", "Boeing 737-700", "Boeing B737-700"),
            ac("B744", "BOEING", "Boeing 747-400", "Boeing B747-400"),
            ac("C172", "CESSNA", "Cessna Skyhawk 172/Cutlass", "Cessna 172S Skyhawk SP"),
            ac("C414", "CESSNA", "Cessna 414", "Cessna 414 Chancellor"),
            ac("PC12", "PILATUS", "Pilatus PC-12", "Pilatus PC-12/45"),
            ac("BE10", "BEECH", "Beech King Air 100", "Beech 100 King Air"),
            ac("DR40", "ROBIN", "Robin Regent", "Robin Regent"),
        ];
        let mut characteristics = HashMap::new();
        for r in rows {
            characteristics.insert(r.icao_code.clone(), r);
        }
        CharacteristicsDatabase { characteristics }
    }

    fn top(idx: &IcaoIndex, q: &str) -> String {
        idx.find(q).map(|m| m.icao).unwrap_or_default()
    }

    #[test]
    fn resolves_from_model_names() {
        let idx = IcaoIndex::build(&test_db());
        assert_eq!(top(&idx, "Boeing 737-800"), "B738");
        assert_eq!(top(&idx, "Boeing 737-700"), "B737");
        assert_eq!(top(&idx, "Airbus A320"), "A320");
        assert_eq!(top(&idx, "Cessna 172"), "C172");
        assert_eq!(top(&idx, "Pilatus PC-12"), "PC12");
        assert_eq!(top(&idx, "Beech King Air 100"), "BE10");
    }

    #[test]
    fn resolves_from_embedded_icao_code() {
        let idx = IcaoIndex::build(&test_db());
        // The ICAO code appearing as a standalone token resolves directly.
        assert_eq!(top(&idx, "FSLabs A320 Lufthansa"), "A320");
        assert_eq!(top(&idx, "PMDG B738 American"), "B738");
    }

    #[test]
    fn handles_noisy_titles() {
        let idx = IcaoIndex::build(&test_db());
        assert_eq!(top(&idx, "Asobo Boeing 747-400 Atlas Air"), "B744");
    }

    #[test]
    fn nickname_source() {
        let idx = IcaoIndex::build(&test_db());
        assert_eq!(top(&idx, "the queen, a jumbo jet"), "B744");
    }

    #[test]
    fn code_as_prefix_of_word_implies_type() {
        let idx = IcaoIndex::build(&test_db());
        assert_eq!(top(&idx, "B738W"), "B738");      // winglets suffix
        assert_eq!(top(&idx, "DR400"), "DR40");      // code DR40 is a prefix of dr400
        assert_eq!(top(&idx, "C414AW"), "C414");     // suffix "aw"
        assert_eq!(top(&idx, "PMDG B738BCF American"), "B738");
    }

    #[test]
    fn neo_max_suffix_is_not_assumed() {
        let idx = IcaoIndex::build(&test_db());
        // A320neo is really A20N and B737MAX is B38M — don't infer the base type.
        assert!(idx.find("A320neo").is_none());
        assert!(idx.find("B737MAX").is_none());
    }

    #[test]
    fn no_match_returns_none() {
        let idx = IcaoIndex::build(&test_db());
        assert!(idx.find("spaceship zzzqqq").is_none());
    }

    #[test]
    fn ignores_addon_developer_names() {
        let idx = IcaoIndex::build(&test_db());
        // Add-on studio names in the title must not affect type resolution.
        assert_eq!(top(&idx, "Fenix Airbus A320"), "A320");
        assert_eq!(top(&idx, "iFly Boeing 737-800"), "B738");
        // A title made up only of studio names carries no type signal.
        assert!(idx.find("PassiveAircraft Laminar").is_none());
    }
}
