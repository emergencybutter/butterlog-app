//! Word → airline index for identifying an operator from a free string (sim aircraft
//! title or livery name, e.g. `"Boeing 737-800 Ryanair"` or a livery path containing
//! `".../United/"`). Same design as [`crate::icao_index`]: an inverted token index built
//! from `airlines.csv`, scored with IDF so distinctive airline words dominate.
//!
//! Keyword sources, in decreasing trust:
//!   1. The airline ICAO designator (e.g. `UAL`) — exact identifier, highest weight.
//!   2. The call sign (e.g. `SPEEDBIRD`).
//!   3. The airline name.
//!
//! Aircraft-manufacturer words and ultra-generic aviation words are dropped as stopwords
//! so a bare model title like `"Airbus A320"` doesn't masquerade as an operator.

use std::collections::{HashMap, HashSet};

use crate::airlines::AirlinesDatabase;

const W_ICAO: f32 = 4.0;
const W_CALLSIGN: f32 = 1.5;
const W_NAME: f32 = 1.0;

/// Words removed from both the index and the query. Aircraft manufacturers (so model
/// titles don't match operators) plus generic aviation filler that carries no operator
/// identity on its own.
const STOPWORDS: &[&str] = &[
    // manufacturers
    "boeing", "airbus", "cessna", "embraer", "bombardier", "cirrus", "piper", "diamond",
    "daher", "pilatus", "beechcraft", "beech", "gulfstream", "dassault", "mcdonnell",
    "douglas", "antonov", "tupolev", "sukhoi", "robin", "mooney", "textron", "tecnam",
    "icon", "kodiak", "honda", "hondajet", "asobo", "microsoft",
    // generic aviation filler
    "air", "airline", "airlines", "airway", "airways", "aviation", "aero", "cargo",
    "international", "intl", "express", "transport", "transports", "group", "company",
    "co", "ltd", "inc", "limited", "holdings", "charter", "flight", "flights", "service",
    "services",
];

struct Posting {
    airline: u32,
    weight: f32,
}

/// A scored airline candidate for a query.
#[derive(Debug, Clone)]
pub struct AirlineMatch {
    pub icao: String,
    pub name: String,
    pub score: f32,
}

pub struct AirlineIndex {
    icaos: Vec<String>,
    names: Vec<String>,
    postings: HashMap<String, Vec<Posting>>,
    idf: HashMap<String, f32>,
    /// Minimum score for `find` to accept a match, scaled to the table size so a match
    /// must rest on at least a moderately distinctive word rather than common filler.
    min_score: f32,
}

fn tokenize(s: &str) -> Vec<String> {
    s.to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|t| !t.is_empty())
        .map(|t| t.to_string())
        .filter(|t| t.len() >= 2 || t.chars().any(|c| c.is_ascii_digit()))
        .filter(|t| !STOPWORDS.contains(&t.as_str()))
        .collect()
}

fn add_tokens(into: &mut HashMap<String, f32>, text: &str, weight: f32) {
    for token in tokenize(text) {
        into.entry(token)
            .and_modify(|w| *w = w.max(weight))
            .or_insert(weight);
    }
}

impl AirlineIndex {
    pub fn build(db: &AirlinesDatabase) -> Self {
        let mut icaos: Vec<String> = Vec::with_capacity(db.airlines.len());
        let mut names: Vec<String> = Vec::with_capacity(db.airlines.len());
        let mut per: Vec<HashMap<String, f32>> = Vec::with_capacity(db.airlines.len());

        for a in &db.airlines {
            let mut tokens = HashMap::new();
            add_tokens(&mut tokens, &a.icao, W_ICAO);
            add_tokens(&mut tokens, &a.callsign, W_CALLSIGN);
            add_tokens(&mut tokens, &a.name, W_NAME);
            icaos.push(a.icao.clone());
            names.push(a.name.clone());
            per.push(tokens);
        }

        let mut postings: HashMap<String, Vec<Posting>> = HashMap::new();
        for (idx, tokens) in per.iter().enumerate() {
            for (token, weight) in tokens {
                postings.entry(token.clone()).or_default().push(Posting {
                    airline: idx as u32,
                    weight: *weight,
                });
            }
        }

        let n = icaos.len().max(1) as f32;
        let idf = postings
            .iter()
            .map(|(token, plist)| (token.clone(), (1.0 + n / plist.len() as f32).ln()))
            .collect();

        // 40% of a unique word's IDF: requires at least a moderately distinctive match.
        let min_score = 0.4 * (1.0 + n).ln();

        Self { icaos, names, postings, idf, min_score }
    }

    pub fn candidates(&self, query: &str, limit: usize) -> Vec<AirlineMatch> {
        let mut scores: HashMap<u32, f32> = HashMap::new();
        let mut seen: HashSet<String> = HashSet::new();

        for token in tokenize(query) {
            if !seen.insert(token.clone()) {
                continue;
            }
            if let (Some(plist), Some(&idf)) = (self.postings.get(&token), self.idf.get(&token)) {
                for posting in plist {
                    *scores.entry(posting.airline).or_insert(0.0) += idf * posting.weight;
                }
            }
        }

        let mut matches: Vec<AirlineMatch> = scores
            .into_iter()
            .map(|(idx, score)| AirlineMatch {
                icao: self.icaos[idx as usize].clone(),
                name: self.names[idx as usize].clone(),
                score,
            })
            .collect();
        matches.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| a.icao.cmp(&b.icao))
        });
        matches.truncate(limit);
        matches
    }

    /// Best airline match for `query`, or `None` if nothing scored above the threshold.
    pub fn find(&self, query: &str) -> Option<AirlineMatch> {
        self.candidates(query, 1)
            .into_iter()
            .next()
            .filter(|m| m.score >= self.min_score)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::airlines::Airline;

    fn al(icao: &str, name: &str, callsign: &str) -> Airline {
        Airline { icao: icao.to_string(), name: name.to_string(), callsign: callsign.to_string() }
    }

    fn test_db() -> AirlinesDatabase {
        AirlinesDatabase {
            airlines: vec![
                al("UAL", "United Airlines", "UNITED"),
                al("DLH", "Lufthansa", "LUFTHANSA"),
                al("BAW", "British Airways", "SPEEDBIRD"),
                al("RYR", "Ryanair", "RYANAIR"),
                al("AAL", "American Airlines", "AMERICAN"),
                al("AFR", "Air France", "AIRFRANS"),
            ],
        }
    }

    fn top(idx: &AirlineIndex, q: &str) -> String {
        idx.find(q).map(|m| m.icao).unwrap_or_default()
    }

    #[test]
    fn matches_airline_name_in_title() {
        let idx = AirlineIndex::build(&test_db());
        assert_eq!(top(&idx, "Boeing 737-800 Ryanair"), "RYR");
        assert_eq!(top(&idx, "Airbus A320 Lufthansa"), "DLH");
        assert_eq!(top(&idx, "Asobo Boeing 747-400 United"), "UAL");
        assert_eq!(top(&idx, "Air France A320"), "AFR");
    }

    #[test]
    fn matches_callsign_in_livery() {
        let idx = AirlineIndex::build(&test_db());
        assert_eq!(top(&idx, ".../liveries/SPEEDBIRD retro"), "BAW");
    }

    #[test]
    fn no_airline_in_plain_model_title() {
        let idx = AirlineIndex::build(&test_db());
        // Manufacturer + model only: no operator should be inferred.
        assert!(idx.find("Cessna 172 Skyhawk").is_none());
        assert!(idx.find("Airbus A320").is_none());
    }
}
