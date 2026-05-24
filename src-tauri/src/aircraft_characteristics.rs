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

        if let (Some(icao_i), Some(mfg_i), Some(model_faa_i), Some(model_bada_i), Some(engine_type_i), Some(num_engines_i), Some(wtc_i), Some(class_i)) = 
            (icao_idx, mfg_idx, model_faa_idx, model_bada_idx, engine_type_idx, num_engines_idx, wtc_idx, class_idx) {
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
                        
                        map.insert(icao_upper.clone(), AircraftCharacteristic {
                            icao_code: icao_upper,
                            manufacturer,
                            model_faa,
                            model_bada,
                            engine_type,
                            num_engines,
                            wtc,
                            class,
                        });
                    }
                }
            }
        }
        Ok(Self { characteristics: map })
    }

    pub fn resolve_title_characteristics(&self, title: &str) -> Option<AircraftCharacteristic> {
        let lower = title.to_lowercase();
        
        let mut candidates: Vec<&AircraftCharacteristic> = self.characteristics.values().collect();
        candidates.sort_by_key(|c| std::cmp::Reverse(c.icao_code.len()));
        
        for candidate in &candidates {
            if !candidate.icao_code.is_empty() {
                let icao_lower = candidate.icao_code.to_lowercase();
                if lower.contains(&icao_lower) {
                    return Some((*candidate).clone());
                }
            }
        }
        
        for candidate in &candidates {
            let mfg = candidate.manufacturer.to_lowercase();
            if !mfg.is_empty() && lower.contains(&mfg) {
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
        
        score
    }
}
