use std::collections::HashMap;
use std::error::Error;
use std::fs::File;
use std::path::Path;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AircraftCharacteristic {
    pub icao_code: String,
    pub manufacturer: String,
    pub model_faa: String,
    pub model_bada: String,
    pub engine_type: String, // jet, turboprop, piston, unknown
    pub num_engines: i32,
    pub wtc: String,        // Medium, Heavy, Super, Light
    pub class: String,      // Fixed-wing, Amphibian, etc.
    pub wingspan: f64,
}

impl AircraftCharacteristic {
    pub fn is_bizjet(&self) -> bool {
        let mfg = self.manufacturer.to_lowercase();
        let model = self.model_faa.to_lowercase();
        mfg.contains("gulfstream") 
            || mfg.contains("learjet") 
            || mfg.contains("dassault")
            || mfg.contains("hawker")
            || (mfg.contains("cessna") && model.contains("citation"))
            || (mfg.contains("bombardier") && (model.contains("global") || model.contains("challenger")))
            || (mfg.contains("embraer") && (model.contains("phenom") || model.contains("legacy") || model.contains("praetor")))
    }

    pub fn is_fighter_jet(&self) -> bool {
        let icao = self.icao_code.to_uppercase();
        let model = self.model_faa.to_lowercase();
        let engine = self.engine_type.to_lowercase();
        
        if engine != "jet" {
            return false;
        }
        
        let is_military_icao = icao.starts_with('F') && icao.chars().nth(1).map_or(false, |c| c.is_ascii_digit())
            || icao == "A10"
            || icao == "T38"
            || icao == "T45"
            || icao == "L39"
            || icao.starts_with("MIG")
            || icao.starts_with("SU")
            || icao.starts_with("MIR")
            || icao.starts_with("EFA")
            || icao.starts_with("JAS");

        is_military_icao
            || model.contains("f-15")
            || model.contains("f-16")
            || model.contains("f-18")
            || model.contains("f-22")
            || model.contains("f-35")
            || model.contains("f/a-18")
            || model.contains("fa-18")
            || model.contains("a-10")
            || model.contains("t-38")
            || model.contains("l-39")
            || model.contains("raptor")
            || model.contains("hornet")
            || model.contains("fighting falcon")
            || model.contains("eagle")
            || model.contains("phantom")
            || model.contains("tomcat")
            || model.contains("harrier")
            || model.contains("talon")
            || model.contains("albatros")
            || model.contains("gripen")
            || model.contains("mirage")
            || model.contains("rafale")
            || model.contains("eurofighter")
            || model.contains("typhoon")
            || model.contains("sukhoi")
            || model.contains("mikoyan")
    }
}

pub struct CharacteristicsDatabase {
    pub characteristics: HashMap<String, AircraftCharacteristic>,
}

impl CharacteristicsDatabase {
    pub fn load_from_csv<P: AsRef<Path>>(path: P) -> Result<Self, Box<dyn Error>> {
        let file = File::open(path)?;
        let mut reader = csv::Reader::from_reader(file);
        let mut map = HashMap::new();
        
        let headers = reader.headers()?.clone();
        
        let icao_idx = headers.iter().position(|h| h == "ICAO_Code");
        let mfg_idx = headers.iter().position(|h| h == "Manufacturer");
        let model_faa_idx = headers.iter().position(|h| h == "Model_FAA");
        let model_bada_idx = headers.iter().position(|h| h == "Model_BADA");
        let engine_type_idx = headers.iter().position(|h| h == "Physical_Class_Engine");
        let num_engines_idx = headers.iter().position(|h| h == "Num_Engines");
        let wtc_idx = headers.iter().position(|h| h == "ICAO_WTC");
        let class_idx = headers.iter().position(|h| h == "Class");
        let wingspan_idx = headers.iter().position(|h| h == "Wingspan_ft_without_winglets_sharklets");

        if let (Some(icao_i), Some(mfg_i), Some(model_faa_i), Some(model_bada_i), Some(engine_type_i), Some(num_engines_i), Some(wtc_i), Some(class_i), Some(wingspan_i)) = 
            (icao_idx, mfg_idx, model_faa_idx, model_bada_idx, engine_type_idx, num_engines_idx, wtc_idx, class_idx, wingspan_idx) {
            for result in reader.records() {
                if let Ok(record) = result {
                    if let Some(icao) = record.get(icao_i) {
                        let icao_upper = icao.to_uppercase();
                        let manufacturer = record.get(mfg_i).unwrap_or("").trim().to_string();
                        let model_faa = record.get(model_faa_i).unwrap_or("").trim().to_string();
                        let model_bada = record.get(model_bada_i).unwrap_or("").trim().to_string();
                        let engine_type = record.get(engine_type_i).unwrap_or("").trim().to_lowercase();
                        let num_engines = record.get(num_engines_i).unwrap_or("0").parse::<i32>().unwrap_or(0);
                        let wtc = record.get(wtc_i).unwrap_or("").trim().to_string();
                        let class = record.get(class_i).unwrap_or("").trim().to_string();
                        let wingspan = record.get(wingspan_i).unwrap_or("0").parse::<f64>().unwrap_or(0.0);
                        
                        map.insert(icao_upper.clone(), AircraftCharacteristic {
                            icao_code: icao_upper,
                            manufacturer,
                            model_faa,
                            model_bada,
                            engine_type,
                            num_engines,
                            wtc,
                            class,
                            wingspan,
                        });
                    }
                }
            }
        }
        Ok(Self { characteristics: map })
    }

    pub fn resolve_title_characteristics(&self, title: &str) -> Option<AircraftCharacteristic> {
        let mut candidates: Vec<&AircraftCharacteristic> = self.characteristics.values().collect();
        candidates.sort_by_key(|c| std::cmp::Reverse(c.icao_code.len()));
        Self::resolve_against_sorted(&candidates, title)
    }

    /// Resolve characteristics for many titles at once. Sorting the candidate list is the
    /// expensive part of `resolve_title_characteristics`; doing it a single time here (rather
    /// than once per title) is what makes multiplayer model matching cheap when many peers
    /// resolve against the same installed library. Keys are the titles as passed in.
    pub fn build_title_index(&self, titles: &[String]) -> HashMap<String, Option<AircraftCharacteristic>> {
        let mut candidates: Vec<&AircraftCharacteristic> = self.characteristics.values().collect();
        candidates.sort_by_key(|c| std::cmp::Reverse(c.icao_code.len()));

        let mut index: HashMap<String, Option<AircraftCharacteristic>> = HashMap::with_capacity(titles.len());
        for title in titles {
            if index.contains_key(title) {
                continue;
            }
            let resolved = Self::resolve_against_sorted(&candidates, title);
            index.insert(title.clone(), resolved);
        }
        index
    }

    /// Shared matching pass over a candidate list already sorted longest-ICAO-first. Kept
    /// identical to the per-call logic so the prebuilt index and ad-hoc resolution agree.
    fn resolve_against_sorted(
        candidates: &[&AircraftCharacteristic],
        title: &str,
    ) -> Option<AircraftCharacteristic> {
        let lower = title.to_lowercase();

        for candidate in candidates {
            if !candidate.icao_code.is_empty() {
                let icao_lower = candidate.icao_code.to_lowercase();
                if lower.contains(&icao_lower) {
                    return Some((*candidate).clone());
                }
            }
        }

        for candidate in candidates {
            // Match on the manufacturer's brand token rather than its full name: the DB often
            // stores multi-word manufacturers ("GULFSTREAM AEROSPACE", "GULFSTREAM AEROSPACE-
            // ROCKWELL") while sim titles carry only the brand ("Gulfstream G650"), so the full
            // string would never appear. The full model-name match below is the strong signal;
            // the brand token just guards against matching a model that happens to be a
            // substring of an unrelated manufacturer's title.
            let mfg = candidate.manufacturer.to_lowercase();
            let brand = mfg.split(|c: char| c.is_whitespace() || c == '-').next().unwrap_or("");
            if !brand.is_empty() && lower.contains(brand) {
                let model_faa = candidate.model_faa.to_lowercase();
                if !model_faa.is_empty() && lower.contains(&model_faa) {
                    return Some((*candidate).clone());
                }
                let model_bada = candidate.model_bada.to_lowercase();
                if !model_bada.is_empty() && lower.contains(&model_bada) {
                    return Some((*candidate).clone());
                }
            }
        }

        None
    }

    pub fn calculate_similarity_score(&self, a: &AircraftCharacteristic, b: &AircraftCharacteristic) -> i32 {
        let mut score = 0;
        
        if a.class.to_lowercase() == b.class.to_lowercase() {
            score += 100;
        }
        
        if !a.manufacturer.is_empty() && !b.manufacturer.is_empty() && a.manufacturer.eq_ignore_ascii_case(&b.manufacturer) {
            score += 20;
        }
        
        if a.engine_type.to_lowercase() == b.engine_type.to_lowercase() {
            score += 50;
        }
        
        if a.wtc.to_lowercase() == b.wtc.to_lowercase() {
            score += 30;
        }
        
        if a.num_engines == b.num_engines {
            score += 10;
        } else if (a.num_engines - b.num_engines).abs() == 1 {
            score += 5;
        }
        
        // Categorize business jets
        if a.is_bizjet() == b.is_bizjet() {
            score += 50;
        }

        // Categorize fighter jets
        if a.is_fighter_jet() == b.is_fighter_jet() {
            score += 50;
        }

        // Compare wingspans (proxy for size)
        if a.wingspan > 0.0 && b.wingspan > 0.0 {
            let diff = (a.wingspan - b.wingspan).abs();
            let avg = (a.wingspan + b.wingspan) / 2.0;
            let rel_diff = diff / avg;
            if rel_diff < 0.1 {
                score += 40;
            } else if rel_diff < 0.25 {
                score += 20;
            } else if rel_diff < 0.5 {
                score += 10;
            }
        }
        
        score
    }
}
