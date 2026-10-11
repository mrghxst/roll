//! A small gazetteer in GeoNames' file formats (`cities500.txt`,
//! `admin1CodesASCII.txt`, `countryInfo.txt`) with the places the fixtures
//! use, so tests and development need no download. Coordinates and
//! populations are rounded public figures.

use std::path::Path;

use anyhow::Result;

/// (name, ascii name, alternate names, lat, lon, country, admin1, population, zone)
type City = (&'static str, &'static str, &'static str, f64, f64, &'static str, &'static str, u64, &'static str);

const CITIES: &[City] = &[
    ("Riva del Garda", "Riva del Garda", "Riva", 45.8858, 10.8410, "IT", "17", 16_000, "Europe/Rome"),
    ("Arco", "Arco", "", 45.9177, 10.8867, "IT", "17", 17_500, "Europe/Rome"),
    ("Nago-Torbole", "Nago-Torbole", "Torbole", 45.8710, 10.8740, "IT", "17", 2_900, "Europe/Rome"),
    ("Malcesine", "Malcesine", "", 45.7633, 10.8094, "IT", "20", 3_700, "Europe/Rome"),
    ("Limone sul Garda", "Limone sul Garda", "", 45.8130, 10.7900, "IT", "09", 1_150, "Europe/Rome"),
    ("Trento", "Trento", "Trient,Trent", 46.0679, 11.1211, "IT", "17", 117_000, "Europe/Rome"),
    ("Verona", "Verona", "Bern,Vérone", 45.4384, 10.9916, "IT", "20", 257_000, "Europe/Rome"),
    ("Rome", "Rome", "Roma,Rom,Rzym,Rome", 41.8919, 12.5113, "IT", "07", 2_318_895, "Europe/Rome"),
    ("Milan", "Milan", "Milano,Mailand", 45.4643, 9.1895, "IT", "09", 1_371_498, "Europe/Rome"),
    ("Bolzano", "Bolzano", "Bozen", 46.4983, 11.3548, "IT", "17", 107_000, "Europe/Rome"),
    ("Ortisei", "Ortisei", "St. Ulrich in Groeden", 46.5747, 11.6717, "IT", "17", 4_600, "Europe/Rome"),
    ("Selva di Val Gardena", "Selva di Val Gardena", "Wolkenstein", 46.5555, 11.7600, "IT", "17", 2_600, "Europe/Rome"),
    ("Canazei", "Canazei", "", 46.4769, 11.7706, "IT", "17", 1_900, "Europe/Rome"),
    ("New York City", "New York City", "New York,NYC", 40.7143, -74.0060, "US", "NY", 8_804_190, "America/New_York"),
    ("Brooklyn", "Brooklyn", "", 40.6501, -73.9496, "US", "NY", 2_559_903, "America/New_York"),
    ("Lisbon", "Lisbon", "Lisboa,Lissabon", 38.7167, -9.1333, "PT", "14", 517_802, "Europe/Lisbon"),
    ("Porto", "Porto", "Oporto", 41.1496, -8.6110, "PT", "17", 249_633, "Europe/Lisbon"),
    ("Zürich", "Zurich", "Zurich,Zurigo,Zuerich", 47.3667, 8.5500, "CH", "ZH", 341_730, "Europe/Zurich"),
    ("Bern", "Bern", "Berne,Berna", 46.9481, 7.4474, "CH", "BE", 121_631, "Europe/Zurich"),
    ("Geneva", "Geneva", "Genève,Genf,Ginevra", 46.2022, 6.1457, "CH", "GE", 183_981, "Europe/Zurich"),
    ("Paris", "Paris", "Parigi", 48.8534, 2.3488, "FR", "11", 2_138_551, "Europe/Paris"),
    ("London", "London", "Londres,Londra", 51.5085, -0.1257, "GB", "ENG", 8_961_989, "Europe/London"),
    ("Berlin", "Berlin", "Berlino", 52.5244, 13.4105, "DE", "16", 3_426_354, "Europe/Berlin"),
    ("Prague", "Prague", "Praha,Prag", 50.0880, 14.4208, "CZ", "52", 1_165_581, "Europe/Prague"),
    ("Barcelona", "Barcelona", "Barcelone", 41.3888, 2.1590, "ES", "56", 1_620_343, "Europe/Madrid"),
    ("Tokyo", "Tokyo", "Tokio,Tōkyō", 35.6895, 139.6917, "JP", "40", 8_336_599, "Asia/Tokyo"),
];

const ADMIN1: &[(&str, &str)] = &[
    ("IT.17", "Trentino-Alto Adige"),
    ("IT.20", "Veneto"),
    ("IT.09", "Lombardy"),
    ("IT.07", "Latium"),
    ("US.NY", "New York"),
    ("PT.14", "Lisbon"),
    ("PT.17", "Porto"),
    ("CH.ZH", "Zurich"),
    ("CH.BE", "Bern"),
    ("CH.GE", "Geneva"),
    ("FR.11", "Île-de-France"),
    ("GB.ENG", "England"),
    ("DE.16", "Berlin"),
    ("CZ.52", "Prague"),
    ("ES.56", "Catalonia"),
    ("JP.40", "Tokyo"),
];

const COUNTRIES: &[(&str, &str, &str)] = &[
    ("IT", "ITA", "Italy"),
    ("US", "USA", "United States"),
    ("PT", "PRT", "Portugal"),
    ("CH", "CHE", "Switzerland"),
    ("FR", "FRA", "France"),
    ("GB", "GBR", "United Kingdom"),
    ("DE", "DEU", "Germany"),
    ("CZ", "CZE", "Czechia"),
    ("ES", "ESP", "Spain"),
    ("JP", "JPN", "Japan"),
];

pub fn write(dir: &Path) -> Result<()> {
    std::fs::create_dir_all(dir)?;
    let mut cities = String::new();
    for (i, (name, ascii, alt, lat, lon, cc, a1, pop, zone)) in CITIES.iter().enumerate() {
        cities.push_str(&format!(
            "{}\t{name}\t{ascii}\t{alt}\t{lat}\t{lon}\tP\tPPL\t{cc}\t\t{a1}\t\t\t\t{pop}\t\t0\t{zone}\t2024-01-01\n",
            100_000 + i
        ));
    }
    std::fs::write(dir.join("cities500.txt"), cities)?;
    let admin1: String = ADMIN1.iter().enumerate().map(|(i, (code, name))| format!("{code}\t{name}\t{name}\t{}\n", 200_000 + i)).collect();
    std::fs::write(dir.join("admin1CodesASCII.txt"), admin1)?;
    let mut countries = String::from("#ISO\tISO3\tISO-Numeric\tfips\tCountry\tCapital\n");
    for (i, (cc, iso3, name)) in COUNTRIES.iter().enumerate() {
        countries.push_str(&format!("{cc}\t{iso3}\t{i}\t{cc}\t{name}\t\n"));
    }
    std::fs::write(dir.join("countryInfo.txt"), countries)?;
    Ok(())
}
