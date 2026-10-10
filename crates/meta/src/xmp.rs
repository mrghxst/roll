//! A tolerant reader for the XMP fields Roll cares about. Values can appear
//! as attributes of `rdf:Description` or as child elements, and lists as
//! `rdf:Bag`, `rdf:Seq` or `rdf:Alt`.

use quick_xml::events::Event;
use quick_xml::reader::Reader;

use crate::model::{DateSource, Gps, Taken, parse_datetime};

#[derive(Debug, Clone, Default, PartialEq)]
pub struct XmpData {
    pub subjects: Vec<String>,
    pub tags_list: Vec<String>,
    pub hierarchical: Vec<String>,
    pub date_original: Option<String>,
    pub date_created: Option<String>,
    pub create_date: Option<String>,
    pub lat: Option<String>,
    pub lon: Option<String>,
    pub alt: Option<String>,
    pub alt_ref: Option<String>,
    pub h_error: Option<String>,
    pub rating: Option<String>,
    pub description: Option<String>,
    pub user_comment: Option<String>,
    pub make: Option<String>,
    pub model: Option<String>,
    pub creator_tool: Option<String>,
}

impl XmpData {
    /// Hierarchical tags in `A/B` form: TagsList, else HierarchicalSubject,
    /// else the flat subjects (the same order Immich uses).
    pub fn tags(&self) -> Vec<String> {
        if !self.tags_list.is_empty() {
            return self.tags_list.clone();
        }
        if !self.hierarchical.is_empty() {
            return self.hierarchical.iter().map(|t| t.replace('|', "/")).collect();
        }
        self.subjects.clone()
    }

    pub fn taken(&self) -> Option<Taken> {
        [&self.date_original, &self.date_created, &self.create_date]
            .into_iter()
            .flatten()
            .find_map(|s| parse_datetime(s))
            .map(|(local, off)| Taken::new(local, off, DateSource::Xmp))
    }

    pub fn gps(&self) -> Option<Gps> {
        let lat = parse_xmp_coord(self.lat.as_deref()?)?;
        let lon = parse_xmp_coord(self.lon.as_deref()?)?;
        let alt = self.alt.as_deref().and_then(parse_rational).map(|a| if self.alt_ref.as_deref() == Some("1") { -a } else { a });
        let accuracy_m = self.h_error.as_deref().and_then(parse_rational);
        let g = Gps { lat, lon, alt, accuracy_m };
        g.is_valid().then_some(g)
    }

    pub fn rating(&self) -> Option<u8> {
        let r: f64 = self.rating.as_deref()?.trim().parse().ok()?;
        (r >= 0.0).then(|| r.round().min(5.0) as u8)
    }

    pub fn is_screenshot(&self) -> bool {
        self.user_comment.as_deref().is_some_and(|c| c.trim().eq_ignore_ascii_case("screenshot"))
    }
}

/// Find an XMP packet inside arbitrary file bytes (JPEG APP1, PNG iTXt,
/// HEIF `mime` items, TIFF). Returns the `x:xmpmeta` element text.
pub fn find_packet(bytes: &[u8]) -> Option<&str> {
    let start = find(bytes, b"<x:xmpmeta")?;
    let end_tag = b"</x:xmpmeta>";
    let end = find(&bytes[start..], end_tag)? + start + end_tag.len();
    std::str::from_utf8(&bytes[start..end]).ok()
}

fn find(hay: &[u8], needle: &[u8]) -> Option<usize> {
    hay.windows(needle.len()).position(|w| w == needle)
}

/// Which field a property maps to, by local name with namespace prefix.
fn field<'a>(d: &'a mut XmpData, qname: &str) -> Option<FieldRef<'a>> {
    Some(match qname {
        "dc:subject" => FieldRef::List(&mut d.subjects),
        "digiKam:TagsList" => FieldRef::List(&mut d.tags_list),
        "lr:hierarchicalSubject" => FieldRef::List(&mut d.hierarchical),
        "exif:DateTimeOriginal" => FieldRef::One(&mut d.date_original),
        "photoshop:DateCreated" => FieldRef::One(&mut d.date_created),
        "xmp:CreateDate" => FieldRef::One(&mut d.create_date),
        "exif:GPSLatitude" => FieldRef::One(&mut d.lat),
        "exif:GPSLongitude" => FieldRef::One(&mut d.lon),
        "exif:GPSAltitude" => FieldRef::One(&mut d.alt),
        "exif:GPSAltitudeRef" => FieldRef::One(&mut d.alt_ref),
        "exif:GPSHPositioningError" | "exifEX:GPSHPositioningError" => FieldRef::One(&mut d.h_error),
        "xmp:Rating" => FieldRef::One(&mut d.rating),
        "dc:description" => FieldRef::One(&mut d.description),
        "exif:UserComment" => FieldRef::One(&mut d.user_comment),
        "tiff:Make" => FieldRef::One(&mut d.make),
        "tiff:Model" => FieldRef::One(&mut d.model),
        "xmp:CreatorTool" => FieldRef::One(&mut d.creator_tool),
        _ => return None,
    })
}

enum FieldRef<'a> {
    List(&'a mut Vec<String>),
    One(&'a mut Option<String>),
}

fn store(d: &mut XmpData, qname: &str, value: String) {
    let value = value.trim().to_string();
    if value.is_empty() {
        return;
    }
    match field(d, qname) {
        Some(FieldRef::List(v)) if !v.contains(&value) => v.push(value),
        Some(FieldRef::One(o)) if o.is_none() => *o = Some(value),
        _ => {}
    }
}

fn entity(name: &str) -> Option<char> {
    Some(match name {
        "amp" => '&',
        "lt" => '<',
        "gt" => '>',
        "quot" => '"',
        "apos" => '\'',
        _ => return None,
    })
}

pub fn parse(xml: &str) -> XmpData {
    let mut d = XmpData::default();
    let mut reader = Reader::from_str(xml);
    // Element stack of qualified names.
    let mut stack: Vec<String> = Vec::new();
    let mut text = String::new();
    while let Ok(ev) = reader.read_event() {
        match ev {
            Event::Start(e) => {
                let name = e.name().as_ref().to_string();
                if name == "rdf:Description" {
                    read_attrs(&mut d, &e);
                }
                stack.push(name);
                text.clear();
            }
            Event::Empty(e) => {
                if e.name().as_ref() == "rdf:Description" {
                    read_attrs(&mut d, &e);
                }
            }
            Event::Text(t) => text.push_str(&t.xml10_content()),
            Event::CData(t) => text.push_str(&t.into_inner()),
            Event::GeneralRef(r) => {
                if let Ok(Some(c)) = r.resolve_char_ref() {
                    text.push(c);
                } else if let Some(c) = entity(&r.xml10_content()) {
                    text.push(c);
                }
            }
            Event::End(_) => {
                let name = stack.pop().unwrap_or_default();
                if let Some(prop) = property_for(&stack, &name) {
                    store(&mut d, &prop, std::mem::take(&mut text));
                }
                text.clear();
            }
            Event::Eof => break,
            _ => {}
        }
    }
    d
}

/// The property a closing element's text belongs to: the element itself, or
/// for `rdf:li` the property two levels up (`prop > rdf:Bag > rdf:li`).
fn property_for(stack: &[String], name: &str) -> Option<String> {
    if name == "rdf:li" {
        let n = stack.len();
        if n >= 2 && matches!(stack[n - 1].as_str(), "rdf:Bag" | "rdf:Seq" | "rdf:Alt") {
            return Some(stack[n - 2].clone());
        }
        return None;
    }
    if name.starts_with("rdf:") || name.starts_with("x:") {
        return None;
    }
    Some(name.to_string())
}

fn read_attrs(d: &mut XmpData, e: &quick_xml::events::BytesStart) {
    for a in e.attributes().flatten() {
        let key = a.key.as_ref().to_string();
        if key.starts_with("xmlns") || key.starts_with("rdf:") {
            continue;
        }
        if let Ok(v) = a.normalized_value(quick_xml::XmlVersion::Implicit1_0) {
            store(d, &key, v.into_owned());
        }
    }
}

/// XMP coordinates are `DDD,MM.mmmmR`, `DDD,MM,SSR`, or signed decimals.
pub fn parse_xmp_coord(s: &str) -> Option<f64> {
    let s = s.trim();
    if let Ok(v) = s.parse::<f64>() {
        return Some(v);
    }
    let last = s.chars().last()?;
    let sign = match last.to_ascii_uppercase() {
        'N' | 'E' => 1.0,
        'S' | 'W' => -1.0,
        _ => return None,
    };
    let body = &s[..s.len() - 1];
    let parts: Vec<f64> = body.split(',').map(|p| p.trim().parse::<f64>()).collect::<Result<_, _>>().ok()?;
    let v = match parts.as_slice() {
        [d] => *d,
        [d, m] => d + m / 60.0,
        [d, m, s] => d + m / 60.0 + s / 3600.0,
        _ => return None,
    };
    Some(sign * v)
}

pub fn parse_rational(s: &str) -> Option<f64> {
    let s = s.trim();
    if let Some((a, b)) = s.split_once('/') {
        let (a, b): (f64, f64) = (a.trim().parse().ok()?, b.trim().parse().ok()?);
        return (b != 0.0).then(|| a / b);
    }
    s.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"<x:xmpmeta xmlns:x="adobe:ns:meta/"><rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#">
<rdf:Description rdf:about="" xmlns:exif="http://ns.adobe.com/exif/1.0/" xmlns:xmp="http://ns.adobe.com/xap/1.0/"
  exif:DateTimeOriginal="2019-08-12T14:03:22+02:00" xmp:Rating="4" exif:GPSLatitude="45,38.5N" exif:GPSLongitude="10,41.25E">
 <dc:subject xmlns:dc="http://purl.org/dc/elements/1.1/"><rdf:Bag><rdf:li>Memes</rdf:li><rdf:li>Lake &amp; Garda</rdf:li></rdf:Bag></dc:subject>
 <digiKam:TagsList xmlns:digiKam="http://www.digikam.org/ns/1.0/"><rdf:Seq><rdf:li>Category/Memes</rdf:li></rdf:Seq></digiKam:TagsList>
 <lr:hierarchicalSubject xmlns:lr="http://ns.adobe.com/lightroom/1.0/"><rdf:Bag><rdf:li>Category|Memes</rdf:li></rdf:Bag></lr:hierarchicalSubject>
 <exif:UserComment><rdf:Alt><rdf:li xml:lang="x-default">Screenshot</rdf:li></rdf:Alt></exif:UserComment>
 <dc:description><rdf:Alt><rdf:li xml:lang="x-default">A cat</rdf:li></rdf:Alt></dc:description>
</rdf:Description></rdf:RDF></x:xmpmeta>"#;

    #[test]
    fn parses_attributes_and_lists() {
        let d = parse(SAMPLE);
        assert_eq!(d.subjects, vec!["Memes", "Lake & Garda"]);
        assert_eq!(d.tags(), vec!["Category/Memes"]);
        assert_eq!(d.rating(), Some(4));
        assert!(d.is_screenshot());
        assert_eq!(d.description.as_deref(), Some("A cat"));
        let t = d.taken().unwrap();
        assert_eq!(t.offset_min, Some(120));
        let g = d.gps().unwrap();
        assert!((g.lat - 45.641_666).abs() < 1e-4);
        assert!((g.lon - 10.6875).abs() < 1e-4);
    }

    #[test]
    fn hierarchical_fallback() {
        let d = XmpData { hierarchical: vec!["Event|2019-08 Lake Garda".into()], ..Default::default() };
        assert_eq!(d.tags(), vec!["Event/2019-08 Lake Garda"]);
    }

    #[test]
    fn finds_packet_in_bytes() {
        let mut bytes = b"\xff\xd8junk".to_vec();
        bytes.extend_from_slice(SAMPLE.as_bytes());
        bytes.extend_from_slice(b"tail");
        assert!(find_packet(&bytes).unwrap().ends_with("</x:xmpmeta>"));
    }

    #[test]
    fn coords() {
        assert_eq!(parse_xmp_coord("-12.5"), Some(-12.5));
        assert!((parse_xmp_coord("41,24,30W").unwrap() + 41.408_333).abs() < 1e-5);
    }
}
