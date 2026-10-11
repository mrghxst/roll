//! An offline gazetteer from GeoNames (CC BY 4.0): `cities500.txt`,
//! `admin1CodesASCII.txt` and `countryInfo.txt`. Roll stores it as one
//! compact TSV file (`places.tsv`) and loads it into memory for reverse
//! lookups ("Riva del Garda, Trentino, Italy"), search ("Set location…")
//! and place names in folder names ("Italy 2019").

use std::collections::HashMap;
use std::io::{BufRead, Write};
use std::path::Path;

use serde::Serialize;

use crate::distance_m;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    City,
    Region,
    Country,
}

impl Kind {
    fn code(self) -> &'static str {
        match self {
            Kind::City => "c",
            Kind::Region => "r",
            Kind::Country => "n",
        }
    }

    /// Accuracy radius used for a place of this kind (section 4.6).
    pub fn accuracy_m(self) -> f64 {
        match self {
            Kind::City => 5_000.0,
            Kind::Region => 50_000.0,
            Kind::Country => 200_000.0,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Entry {
    pub kind: Kind,
    pub name: String,
    /// Folded names for search: ASCII name and alternate Latin names.
    pub keys: Vec<String>,
    pub lat: f64,
    pub lon: f64,
    pub cc: String,
    pub region: String,
    pub country: String,
    pub population: u64,
}

impl Entry {
    /// "Riva del Garda, Trentino-Alto Adige, Italy".
    pub fn label(&self) -> String {
        match self.kind {
            Kind::Country => self.name.clone(),
            Kind::Region => {
                [self.name.as_str(), self.country.as_str()].iter().filter(|s| !s.is_empty()).copied().collect::<Vec<_>>().join(", ")
            }
            Kind::City => [self.name.as_str(), self.region.as_str(), self.country.as_str()]
                .iter()
                .filter(|s| !s.is_empty())
                .copied()
                .collect::<Vec<_>>()
                .join(", "),
        }
    }
}

/// Lowercase ASCII with accents removed and punctuation turned to spaces.
pub fn fold(s: &str) -> String {
    let a = deunicode::deunicode(s).to_lowercase();
    a.split(|c: char| !c.is_ascii_alphanumeric()).filter(|w| !w.is_empty()).collect::<Vec<_>>().join(" ")
}

/// A cell in the 0.5° grid used for reverse lookups.
fn cell(lat: f64, lon: f64) -> (i32, i32) {
    ((lat * 2.0).floor() as i32, (lon * 2.0).floor() as i32)
}

#[derive(Default)]
pub struct Gazetteer {
    pub entries: Vec<Entry>,
    grid: HashMap<(i32, i32), Vec<u32>>,
    /// Folded name → entries, for exact and prefix matches.
    names: HashMap<String, Vec<u32>>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Hit {
    pub label: String,
    pub name: String,
    pub kind: Kind,
    pub lat: f64,
    pub lon: f64,
    pub accuracy_m: f64,
    pub cc: String,
}

impl Gazetteer {
    pub fn from_entries(entries: Vec<Entry>) -> Self {
        let mut g = Gazetteer { entries, ..Default::default() };
        for (i, e) in g.entries.iter().enumerate() {
            if e.kind == Kind::City {
                g.grid.entry(cell(e.lat, e.lon)).or_default().push(i as u32);
            }
            for k in &e.keys {
                g.names.entry(k.clone()).or_default().push(i as u32);
            }
        }
        g
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    fn hit(&self, i: u32) -> Hit {
        let e = &self.entries[i as usize];
        Hit {
            label: e.label(),
            name: e.name.clone(),
            kind: e.kind,
            lat: e.lat,
            lon: e.lon,
            accuracy_m: e.kind.accuracy_m(),
            cc: e.cc.clone(),
        }
    }

    /// The nearest city within 40 km, else the nearest within 150 km as a
    /// region-level answer.
    pub fn reverse(&self, lat: f64, lon: f64) -> Option<&Entry> {
        let (cy, cx) = cell(lat, lon);
        let mut best: Option<(f64, u32)> = None;
        for dy in -1..=1 {
            for dx in -1..=1 {
                for &i in self.grid.get(&(cy + dy, cx + dx)).into_iter().flatten() {
                    let e = &self.entries[i as usize];
                    // Bigger places win over villages at similar distance.
                    let d = distance_m(lat, lon, e.lat, e.lon) / (1.0 + (e.population as f64).log10().max(0.0) * 0.05);
                    if best.is_none_or(|(b, _)| d < b) {
                        best = Some((d, i));
                    }
                }
            }
        }
        let (_, i) = best?;
        let e = &self.entries[i as usize];
        (distance_m(lat, lon, e.lat, e.lon) < 40_000.0).then_some(e)
    }

    /// Search by name: exact names first, then prefixes, bigger places first.
    pub fn search(&self, query: &str, limit: usize) -> Vec<Hit> {
        let q = fold(query);
        if q.is_empty() {
            return Vec::new();
        }
        // "Rome, Italy": the part after the comma narrows by region/country.
        let (head, ctx) = match query.split_once(',') {
            Some((h, c)) => (fold(h), fold(c)),
            None => (q.clone(), String::new()),
        };
        let mut scored: Vec<(i64, u32)> = Vec::new();
        for (name, ids) in &self.names {
            let rank = if *name == head {
                3
            } else if name.starts_with(&head) {
                2
            } else if head.len() >= 4 && name.contains(&head) {
                1
            } else {
                continue;
            };
            for &i in ids {
                let e = &self.entries[i as usize];
                if !ctx.is_empty() && !fold(&e.country).starts_with(&ctx) && !fold(&e.region).starts_with(&ctx) && fold(&e.cc) != ctx {
                    continue;
                }
                // Countries first; a city beats the region of the same name
                // ("Zurich" the city, not the canton).
                let kind_bonus = match e.kind {
                    Kind::Country => 2,
                    Kind::City => 1,
                    Kind::Region => 0,
                };
                scored.push((rank * 1_000_000_000_000 + kind_bonus * 100_000_000_000 + e.population as i64, i));
            }
        }
        scored.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
        scored.dedup_by_key(|s| s.1);
        scored.into_iter().take(limit).map(|(_, i)| self.hit(i)).collect()
    }

    /// The best place named in a folder or file name: "2019 Italy",
    /// "Lake Garda 2019", "Trip to Rome". Countries and regions match any
    /// word run; cities need 15,000 people or a name of two words or more,
    /// so ordinary words that happen to be village names don't match.
    pub fn in_text(&self, text: &str) -> Option<Hit> {
        let words: Vec<String> =
            fold(text).split(' ').filter(|w| !w.is_empty() && !w.chars().all(|c| c.is_ascii_digit())).map(String::from).collect();
        let mut best: Option<(i64, u32)> = None;
        for len in (1..=4.min(words.len())).rev() {
            for start in 0..=words.len() - len {
                let phrase = words[start..start + len].join(" ");
                if phrase.len() < 3 {
                    continue;
                }
                for &i in self.names.get(&phrase).into_iter().flatten() {
                    let e = &self.entries[i as usize];
                    let ok = match e.kind {
                        Kind::Country | Kind::Region => true,
                        Kind::City => e.population >= 15_000 || len >= 2,
                    };
                    if !ok {
                        continue;
                    }
                    let score = len as i64 * 1_000_000_000_000 + e.population as i64;
                    if best.is_none_or(|(b, _)| score > b) {
                        best = Some((score, i));
                    }
                }
            }
        }
        best.map(|(_, i)| self.hit(i))
    }

    // Storage: one entry per line, tab separated.
    // kind, name, keys (|), lat, lon, cc, region, country, population

    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        let tmp = path.with_extension("tmp");
        {
            let mut w = std::io::BufWriter::new(std::fs::File::create(&tmp)?);
            writeln!(w, "# Roll places from GeoNames (CC BY 4.0), https://www.geonames.org")?;
            for e in &self.entries {
                writeln!(
                    w,
                    "{}\t{}\t{}\t{:.5}\t{:.5}\t{}\t{}\t{}\t{}",
                    e.kind.code(),
                    e.name,
                    e.keys.join("|"),
                    e.lat,
                    e.lon,
                    e.cc,
                    e.region,
                    e.country,
                    e.population
                )?;
            }
        }
        std::fs::rename(tmp, path)
    }

    pub fn load(path: &Path) -> std::io::Result<Self> {
        let f = std::io::BufReader::new(std::fs::File::open(path)?);
        let mut entries = Vec::new();
        for line in f.lines() {
            let line = line?;
            if line.starts_with('#') || line.is_empty() {
                continue;
            }
            let c: Vec<&str> = line.split('\t').collect();
            if c.len() < 9 {
                continue;
            }
            let kind = match c[0] {
                "n" => Kind::Country,
                "r" => Kind::Region,
                _ => Kind::City,
            };
            entries.push(Entry {
                kind,
                name: c[1].to_string(),
                keys: c[2].split('|').filter(|k| !k.is_empty()).map(String::from).collect(),
                lat: c[3].parse().unwrap_or(0.0),
                lon: c[4].parse().unwrap_or(0.0),
                cc: c[5].to_string(),
                region: c[6].to_string(),
                country: c[7].to_string(),
                population: c[8].parse().unwrap_or(0),
            });
        }
        Ok(Self::from_entries(entries))
    }
}

/// Build from the GeoNames files. Regions and countries get the
/// population-weighted centre of their cities.
pub fn build(cities: impl BufRead, admin1: impl BufRead, countries: impl BufRead) -> std::io::Result<Gazetteer> {
    let mut country_names: HashMap<String, String> = HashMap::new();
    for line in countries.lines() {
        let line = line?;
        if line.starts_with('#') {
            continue;
        }
        let c: Vec<&str> = line.split('\t').collect();
        if c.len() > 4 {
            country_names.insert(c[0].to_string(), c[4].to_string());
        }
    }
    let mut region_names: HashMap<String, String> = HashMap::new();
    for line in admin1.lines() {
        let line = line?;
        let c: Vec<&str> = line.split('\t').collect();
        if c.len() > 1 {
            region_names.insert(c[0].to_string(), c[1].to_string());
        }
    }
    let mut entries = Vec::new();
    // (sum lat*w, sum lon*w, sum w, population) per region and country.
    let mut regions: HashMap<String, (f64, f64, f64, u64)> = HashMap::new();
    let mut nations: HashMap<String, (f64, f64, f64, u64)> = HashMap::new();
    for line in cities.lines() {
        let line = line?;
        let c: Vec<&str> = line.split('\t').collect();
        if c.len() < 15 {
            continue;
        }
        let (Ok(lat), Ok(lon)) = (c[4].parse::<f64>(), c[5].parse::<f64>()) else { continue };
        let cc = c[8].to_string();
        let region_code = format!("{cc}.{}", c[10]);
        let population: u64 = c[14].parse().unwrap_or(0);
        let mut keys = vec![fold(c[1]), fold(c[2])];
        // Alternate names only for larger places, and only Latin ones.
        if population >= 50_000 {
            for alt in c[3].split(',').take(60) {
                if alt.chars().all(|ch| ch.is_ascii() || ch.is_alphabetic() && (ch as u32) < 0x250) && alt.len() > 2 {
                    keys.push(fold(alt));
                }
            }
        }
        keys.retain(|k| !k.is_empty());
        keys.sort();
        keys.dedup();
        let w = (population as f64).max(1.0);
        for (map, key) in [(&mut regions, region_code.clone()), (&mut nations, cc.clone())] {
            let a = map.entry(key).or_default();
            a.0 += lat * w;
            a.1 += lon * w;
            a.2 += w;
            a.3 += population;
        }
        entries.push(Entry {
            kind: Kind::City,
            name: c[1].to_string(),
            keys,
            lat,
            lon,
            cc: cc.clone(),
            region: region_names.get(&region_code).cloned().unwrap_or_default(),
            country: country_names.get(&cc).cloned().unwrap_or_default(),
            population,
        });
    }
    for (code, (la, lo, w, pop)) in regions {
        let Some(name) = region_names.get(&code) else { continue };
        let cc = code.split('.').next().unwrap_or_default().to_string();
        entries.push(Entry {
            kind: Kind::Region,
            name: name.clone(),
            keys: vec![fold(name)],
            lat: la / w,
            lon: lo / w,
            country: country_names.get(&cc).cloned().unwrap_or_default(),
            cc,
            region: String::new(),
            population: pop,
        });
    }
    for (cc, (la, lo, w, pop)) in nations {
        let Some(name) = country_names.get(&cc) else { continue };
        entries.push(Entry {
            kind: Kind::Country,
            name: name.clone(),
            keys: vec![fold(name)],
            lat: la / w,
            lon: lo / w,
            cc: cc.clone(),
            region: String::new(),
            country: name.clone(),
            population: pop,
        });
    }
    Ok(Gazetteer::from_entries(entries))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[allow(clippy::too_many_arguments)]
    fn city(id: u32, name: &str, alt: &str, lat: f64, lon: f64, cc: &str, a1: &str, pop: u64) -> String {
        format!(
            "{id}\t{name}\t{}\t{alt}\t{lat}\t{lon}\tP\tPPL\t{cc}\t\t{a1}\t\t\t\t{pop}\t\t0\tEurope/Rome\t2024-01-01",
            deunicode::deunicode(name)
        )
    }

    fn sample() -> Gazetteer {
        let cities = [
            city(1, "Riva del Garda", "", 45.885, 10.841, "IT", "17", 16_000),
            city(2, "Malcesine", "", 45.764, 10.809, "IT", "20", 3_700),
            city(3, "Rome", "Roma,Rom,Rzym", 41.894, 12.483, "IT", "07", 2_318_895),
            city(4, "Zürich", "Zurich,Zurigo", 47.367, 8.55, "CH", "ZH", 341_730),
            city(5, "Party", "", 52.0, 4.0, "NL", "01", 200),
            city(6, "Paris", "", 48.853, 2.349, "FR", "11", 2_138_551),
        ]
        .join("\n");
        let admin1 = "IT.17\tTrentino-Alto Adige\tTrentino-Alto Adige\t1\nIT.20\tVeneto\tVeneto\t2\nIT.07\tLazio\tLazio\t3\nCH.ZH\tZurich\tZurich\t4\nFR.11\tÎle-de-France\tIle-de-France\t5";
        let countries = "#ISO\tISO3\tnum\tfips\tCountry\nIT\tITA\t380\tIT\tItaly\nCH\tCHE\t756\tSZ\tSwitzerland\nFR\tFRA\t250\tFR\tFrance\nNL\tNLD\t528\tNL\tNetherlands";
        build(cities.as_bytes(), admin1.as_bytes(), countries.as_bytes()).unwrap()
    }

    #[test]
    fn reverse_lookup() {
        let g = sample();
        let e = g.reverse(45.88, 10.84).unwrap();
        assert_eq!(e.label(), "Riva del Garda, Trentino-Alto Adige, Italy");
        assert!(g.reverse(0.0, -30.0).is_none());
    }

    #[test]
    fn search_ranks_and_folds() {
        let g = sample();
        assert_eq!(g.search("zurich", 5)[0].label, "Zürich, Zurich, Switzerland");
        assert_eq!(g.search("Roma", 5)[0].name, "Rome");
        assert_eq!(g.search("ital", 5)[0].kind, Kind::Country);
        assert_eq!(g.search("Rome, Italy", 5)[0].name, "Rome");
        assert!(g.search("Rome, France", 5).is_empty());
        assert!(g.search("", 5).is_empty());
    }

    #[test]
    fn names_in_folders() {
        let g = sample();
        let h = g.in_text("Italy 2019").unwrap();
        assert_eq!((h.kind, h.name.as_str()), (Kind::Country, "Italy"));
        assert_eq!(h.accuracy_m, 200_000.0);
        assert_eq!(g.in_text("2019-08 Riva del Garda").unwrap().name, "Riva del Garda");
        assert_eq!(g.in_text("Trip to Rome").unwrap().name, "Rome");
        assert!(g.in_text("Party pics").is_none(), "small towns named like words are ignored");
        assert!(g.in_text("DCIM 100CANON").is_none());
    }

    #[test]
    fn save_and_load() {
        let g = sample();
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("places.tsv");
        g.save(&p).unwrap();
        let h = Gazetteer::load(&p).unwrap();
        assert_eq!(h.len(), g.len());
        assert_eq!(h.search("Zurigo", 1)[0].name, "Zürich");
    }
}
