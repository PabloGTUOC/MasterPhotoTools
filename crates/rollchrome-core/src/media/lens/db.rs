//! The lensfun database, shipped with the application and parsed once (DV-9).
//!
//! The XML is lensfun's own (`data/lensfun/`, CC BY-SA 3.0, see its README), so a newer copy drops in
//! unchanged. Only what Develop uses is kept: cameras (mount, crop factor), mounts and their
//! compatibility, and lenses with their distortion, lateral CA and vignetting calibrations.

use std::sync::OnceLock;

use quick_xml::events::Event;
use quick_xml::Reader;

use super::model::{Distortion, Tca, VignettingCal};

/// Every database file, embedded at build time.
static FILES: &[(&str, &str)] = &[
    ("6x6.xml", include_str!("../../../data/lensfun/6x6.xml")),
    (
        "actioncams.xml",
        include_str!("../../../data/lensfun/actioncams.xml"),
    ),
    (
        "compact-canon.xml",
        include_str!("../../../data/lensfun/compact-canon.xml"),
    ),
    (
        "compact-casio.xml",
        include_str!("../../../data/lensfun/compact-casio.xml"),
    ),
    (
        "compact-fujifilm.xml",
        include_str!("../../../data/lensfun/compact-fujifilm.xml"),
    ),
    (
        "compact-kodak.xml",
        include_str!("../../../data/lensfun/compact-kodak.xml"),
    ),
    (
        "compact-konica-minolta.xml",
        include_str!("../../../data/lensfun/compact-konica-minolta.xml"),
    ),
    (
        "compact-leica.xml",
        include_str!("../../../data/lensfun/compact-leica.xml"),
    ),
    (
        "compact-nikon.xml",
        include_str!("../../../data/lensfun/compact-nikon.xml"),
    ),
    (
        "compact-olympus.xml",
        include_str!("../../../data/lensfun/compact-olympus.xml"),
    ),
    (
        "compact-panasonic.xml",
        include_str!("../../../data/lensfun/compact-panasonic.xml"),
    ),
    (
        "compact-pentax.xml",
        include_str!("../../../data/lensfun/compact-pentax.xml"),
    ),
    (
        "compact-ricoh.xml",
        include_str!("../../../data/lensfun/compact-ricoh.xml"),
    ),
    (
        "compact-samsung.xml",
        include_str!("../../../data/lensfun/compact-samsung.xml"),
    ),
    (
        "compact-sigma.xml",
        include_str!("../../../data/lensfun/compact-sigma.xml"),
    ),
    (
        "compact-sony.xml",
        include_str!("../../../data/lensfun/compact-sony.xml"),
    ),
    (
        "contax.xml",
        include_str!("../../../data/lensfun/contax.xml"),
    ),
    (
        "generic.xml",
        include_str!("../../../data/lensfun/generic.xml"),
    ),
    (
        "mil-canon.xml",
        include_str!("../../../data/lensfun/mil-canon.xml"),
    ),
    (
        "mil-fujifilm.xml",
        include_str!("../../../data/lensfun/mil-fujifilm.xml"),
    ),
    (
        "mil-hasselblad.xml",
        include_str!("../../../data/lensfun/mil-hasselblad.xml"),
    ),
    (
        "mil-leica.xml",
        include_str!("../../../data/lensfun/mil-leica.xml"),
    ),
    (
        "mil-nikon.xml",
        include_str!("../../../data/lensfun/mil-nikon.xml"),
    ),
    (
        "mil-olympus.xml",
        include_str!("../../../data/lensfun/mil-olympus.xml"),
    ),
    (
        "mil-panasonic.xml",
        include_str!("../../../data/lensfun/mil-panasonic.xml"),
    ),
    (
        "mil-pentax.xml",
        include_str!("../../../data/lensfun/mil-pentax.xml"),
    ),
    (
        "mil-samsung.xml",
        include_str!("../../../data/lensfun/mil-samsung.xml"),
    ),
    (
        "mil-samyang.xml",
        include_str!("../../../data/lensfun/mil-samyang.xml"),
    ),
    (
        "mil-sigma.xml",
        include_str!("../../../data/lensfun/mil-sigma.xml"),
    ),
    (
        "mil-sony.xml",
        include_str!("../../../data/lensfun/mil-sony.xml"),
    ),
    (
        "mil-tamron.xml",
        include_str!("../../../data/lensfun/mil-tamron.xml"),
    ),
    (
        "mil-tokina.xml",
        include_str!("../../../data/lensfun/mil-tokina.xml"),
    ),
    (
        "mil-zeiss.xml",
        include_str!("../../../data/lensfun/mil-zeiss.xml"),
    ),
    ("misc.xml", include_str!("../../../data/lensfun/misc.xml")),
    (
        "om-system.xml",
        include_str!("../../../data/lensfun/om-system.xml"),
    ),
    (
        "rf-leica.xml",
        include_str!("../../../data/lensfun/rf-leica.xml"),
    ),
    (
        "slr-canon.xml",
        include_str!("../../../data/lensfun/slr-canon.xml"),
    ),
    (
        "slr-hasselblad.xml",
        include_str!("../../../data/lensfun/slr-hasselblad.xml"),
    ),
    (
        "slr-konica-minolta.xml",
        include_str!("../../../data/lensfun/slr-konica-minolta.xml"),
    ),
    (
        "slr-leica.xml",
        include_str!("../../../data/lensfun/slr-leica.xml"),
    ),
    (
        "slr-nikon.xml",
        include_str!("../../../data/lensfun/slr-nikon.xml"),
    ),
    (
        "slr-olympus.xml",
        include_str!("../../../data/lensfun/slr-olympus.xml"),
    ),
    (
        "slr-panasonic.xml",
        include_str!("../../../data/lensfun/slr-panasonic.xml"),
    ),
    (
        "slr-pentax.xml",
        include_str!("../../../data/lensfun/slr-pentax.xml"),
    ),
    (
        "slr-ricoh.xml",
        include_str!("../../../data/lensfun/slr-ricoh.xml"),
    ),
    (
        "slr-samsung.xml",
        include_str!("../../../data/lensfun/slr-samsung.xml"),
    ),
    (
        "slr-samyang.xml",
        include_str!("../../../data/lensfun/slr-samyang.xml"),
    ),
    (
        "slr-schneider.xml",
        include_str!("../../../data/lensfun/slr-schneider.xml"),
    ),
    (
        "slr-sigma.xml",
        include_str!("../../../data/lensfun/slr-sigma.xml"),
    ),
    (
        "slr-soligor.xml",
        include_str!("../../../data/lensfun/slr-soligor.xml"),
    ),
    (
        "slr-sony.xml",
        include_str!("../../../data/lensfun/slr-sony.xml"),
    ),
    (
        "slr-tamron.xml",
        include_str!("../../../data/lensfun/slr-tamron.xml"),
    ),
    (
        "slr-tokina.xml",
        include_str!("../../../data/lensfun/slr-tokina.xml"),
    ),
    (
        "slr-ussr.xml",
        include_str!("../../../data/lensfun/slr-ussr.xml"),
    ),
    (
        "slr-vivitar.xml",
        include_str!("../../../data/lensfun/slr-vivitar.xml"),
    ),
    (
        "slr-zeiss.xml",
        include_str!("../../../data/lensfun/slr-zeiss.xml"),
    ),
];

/// A camera body as lensfun knows it.
#[derive(Debug, Clone, Default)]
pub struct Camera {
    /// The maker's names, the plain one and any `lang` aliases.
    pub makers: Vec<String>,
    /// The model's names, the plain one and any `lang` aliases.
    pub models: Vec<String>,
    pub mount: String,
    pub crop: f32,
}

/// A lens and its calibrations.
#[derive(Debug, Clone, Default)]
pub struct Lens {
    pub makers: Vec<String>,
    pub models: Vec<String>,
    pub mounts: Vec<String>,
    /// The crop factor of the sensor the calibrations were made on.
    pub crop: f32,
    /// Long side over short side of that sensor; lensfun's default is 3:2.
    pub aspect: f32,
    /// Fisheyes and other projections are not rectilinear; their distortion needs a remap this module
    /// does not do, so they are never matched.
    pub rectilinear: bool,
    pub distortion: Vec<(f32, Distortion)>,
    pub tca: Vec<(f32, Tca)>,
    pub vignetting: Vec<VignettingCal>,
}

impl Lens {
    pub fn is_calibrated(&self) -> bool {
        !(self.distortion.is_empty() && self.tca.is_empty() && self.vignetting.is_empty())
    }
}

/// A mount and the mounts whose lenses also fit it.
#[derive(Debug, Clone, Default)]
pub struct Mount {
    pub name: String,
    pub compat: Vec<String>,
}

#[derive(Debug, Default)]
pub struct Database {
    pub cameras: Vec<Camera>,
    pub lenses: Vec<Lens>,
    pub mounts: Vec<Mount>,
}

/// The database, parsed on first use.
pub fn database() -> Result<&'static Database, String> {
    static DB: OnceLock<Result<Database, String>> = OnceLock::new();
    DB.get_or_init(|| {
        let mut db = Database::default();
        for (name, xml) in FILES {
            parse_into(xml, &mut db).map_err(|e| format!("lens database {name}: {e}"))?;
        }
        Ok(db)
    })
    .as_ref()
    .map_err(Clone::clone)
}

fn attr(e: &quick_xml::events::BytesStart<'_>, key: &str) -> Option<String> {
    e.attributes()
        .flatten()
        .find(|a| a.key.as_ref() == key.as_bytes())
        .and_then(|a| {
            a.normalized_value(quick_xml::XmlVersion::Implicit1_0)
                .ok()
                .map(|v| v.into_owned())
        })
}

fn num(e: &quick_xml::events::BytesStart<'_>, key: &str) -> Option<f32> {
    attr(e, key).and_then(|v| v.trim().parse().ok())
}

/// `4:3`, `3:2` or a plain number.
fn parse_aspect(text: &str) -> Option<f32> {
    let t = text.trim();
    if let Some((a, b)) = t.split_once(':') {
        let (a, b): (f32, f32) = (a.trim().parse().ok()?, b.trim().parse().ok()?);
        (b > 0.0).then(|| (a / b).max(b / a))
    } else {
        t.parse::<f32>().ok().map(|v| v.max(1.0 / v))
    }
}

enum Record {
    None,
    Camera(Camera),
    Lens(Lens),
    Mount(Mount),
}

/// The calibration elements are empty tags whose meaning is all in their attributes.
fn calibration(name: &str, e: &quick_xml::events::BytesStart<'_>, lens: &mut Lens) {
    let focal = num(e, "focal").unwrap_or(0.0);
    match name {
        "distortion" => {
            let model = attr(e, "model").unwrap_or_default();
            let d = match model.as_str() {
                "ptlens" => Some(Distortion::Ptlens {
                    a: num(e, "a").unwrap_or(0.0),
                    b: num(e, "b").unwrap_or(0.0),
                    c: num(e, "c").unwrap_or(0.0),
                }),
                "poly3" => Some(Distortion::Poly3 {
                    k1: num(e, "k1").unwrap_or(0.0),
                }),
                "poly5" => Some(Distortion::Poly5 {
                    k1: num(e, "k1").unwrap_or(0.0),
                    k2: num(e, "k2").unwrap_or(0.0),
                }),
                // Adobe's model and anything newer: not read, so not applied.
                _ => None,
            };
            if let Some(d) = d {
                lens.distortion.push((focal, d));
            }
        }
        "tca" => {
            let model = attr(e, "model").unwrap_or_default();
            let t = match model.as_str() {
                "linear" => Some(Tca {
                    red: [num(e, "kr").unwrap_or(1.0), 0.0, 0.0],
                    blue: [num(e, "kb").unwrap_or(1.0), 0.0, 0.0],
                }),
                "poly3" => Some(Tca {
                    red: [
                        num(e, "vr").unwrap_or(1.0),
                        num(e, "cr").unwrap_or(0.0),
                        num(e, "br").unwrap_or(0.0),
                    ],
                    blue: [
                        num(e, "vb").unwrap_or(1.0),
                        num(e, "cb").unwrap_or(0.0),
                        num(e, "bb").unwrap_or(0.0),
                    ],
                }),
                _ => None,
            };
            if let Some(t) = t {
                lens.tca.push((focal, t));
            }
        }
        "vignetting" if attr(e, "model").as_deref() == Some("pa") => {
            lens.vignetting.push(VignettingCal {
                focal,
                aperture: num(e, "aperture").unwrap_or(0.0),
                distance: num(e, "distance").unwrap_or(1000.0),
                k: [
                    num(e, "k1").unwrap_or(0.0),
                    num(e, "k2").unwrap_or(0.0),
                    num(e, "k3").unwrap_or(0.0),
                ],
            });
        }
        _ => {}
    }
}

fn parse_into(xml: &str, db: &mut Database) -> Result<(), String> {
    let mut reader = Reader::from_str(xml);
    // Not trimmed per event: a name like `XZ-1 &amp; compatibles` arrives as text, an entity and
    // text, and trimming each piece would drop the spaces around the `&`. Values are trimmed whole.
    let mut record = Record::None;
    let mut path: Vec<String> = Vec::new();
    let mut text = String::new();

    loop {
        match reader.read_event().map_err(|e| e.to_string())? {
            Event::Start(e) => {
                let name = String::from_utf8_lossy(e.name().as_ref()).to_string();
                match name.as_str() {
                    "camera" if path.len() == 1 => record = Record::Camera(Camera::default()),
                    "lens" if path.len() == 1 => {
                        record = Record::Lens(Lens {
                            aspect: 1.5,
                            rectilinear: true,
                            ..Lens::default()
                        })
                    }
                    "mount" if path.len() == 1 => record = Record::Mount(Mount::default()),
                    _ => {}
                }
                path.push(name);
                text.clear();
            }
            Event::Empty(e) => {
                let name = String::from_utf8_lossy(e.name().as_ref()).to_string();
                if let Record::Lens(lens) = &mut record {
                    if path.last().map(String::as_str) == Some("calibration") {
                        calibration(&name, &e, lens);
                    }
                }
            }
            Event::Text(t) => text.push_str(&t.decode().map_err(|e| e.to_string())?),
            Event::GeneralRef(r) => {
                let entity = r.decode().map_err(|e| e.to_string())?;
                text.push_str(match entity.as_ref() {
                    "amp" => "&",
                    "lt" => "<",
                    "gt" => ">",
                    "quot" => "\"",
                    "apos" => "'",
                    _ => "",
                });
            }
            Event::End(_) => {
                let Some(name) = path.pop() else { break };
                let value = text.trim().to_string();
                text.clear();
                let depth = path.len();
                match (&mut record, name.as_str()) {
                    // A record's own fields are its direct children (depth 2 once popped to 2).
                    (Record::Camera(c), "maker") if depth == 2 => c.makers.push(value),
                    (Record::Camera(c), "model") if depth == 2 => c.models.push(value),
                    (Record::Camera(c), "mount") if depth == 2 => c.mount = value,
                    (Record::Camera(c), "cropfactor") if depth == 2 => {
                        c.crop = value.parse().unwrap_or(0.0)
                    }
                    (Record::Lens(l), "maker") if depth == 2 => l.makers.push(value),
                    (Record::Lens(l), "model") if depth == 2 => l.models.push(value),
                    (Record::Lens(l), "mount") if depth == 2 => l.mounts.push(value),
                    (Record::Lens(l), "cropfactor") if depth == 2 => {
                        l.crop = value.parse().unwrap_or(0.0)
                    }
                    (Record::Lens(l), "aspect-ratio") if depth == 2 => {
                        if let Some(a) = parse_aspect(&value) {
                            l.aspect = a;
                        }
                    }
                    (Record::Lens(l), "type") if depth == 2 => {
                        l.rectilinear = value.is_empty() || value == "rectilinear"
                    }
                    (Record::Mount(m), "name") if depth == 2 => m.name = value,
                    (Record::Mount(m), "compat") if depth == 2 => m.compat.push(value),
                    (_, "camera" | "lens" | "mount") if depth == 1 => {
                        match std::mem::replace(&mut record, Record::None) {
                            Record::Camera(c) => db.cameras.push(c),
                            Record::Lens(l) => db.lenses.push(l),
                            Record::Mount(m) => db.mounts.push(m),
                            Record::None => {}
                        }
                    }
                    _ => {}
                }
            }
            Event::Eof => break,
            _ => {}
        }
    }
    Ok(())
}
