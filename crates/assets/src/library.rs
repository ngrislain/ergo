//! Deterministic synthesis of the single Ergo part library.
//!
//! `pull` downloads the official LDraw archive and (optionally) a Rebrickable
//! catalogue snapshot. Every input is stored content-addressed, so `synthesize`
//! is a pure function of input bytes: the same inputs always give byte-identical
//! library JSON and mesh objects. `publish` writes the objects, the library and
//! finally the `current` pointer; `check` re-synthesizes a published revision
//! from its stored inputs and compares bytes.
use crate::Credit;
use anyhow::{Context, Result, bail, ensure};
use ergo_world::{
    derive,
    geometry::Mesh,
    ldraw,
    part_library::{FORMAT, Library, LibraryPart, Listed, MassCalibration, Part, Review, Reviews},
};
use futures::StreamExt;
use object_store::{ObjectStore, PutPayload, path::Path};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    cell::RefCell,
    collections::{BTreeMap, BTreeSet, HashMap},
    io::{Cursor, Read, Write},
    sync::Arc,
};

pub const LDRAW_ARCHIVE_URL: &str = "https://library.ldraw.org/library/updates/complete.zip";
pub const LDRAW_AUTHORITY: &str = "https://library.ldraw.org/library/official";
const REBRICKABLE_API: &str = "https://rebrickable.com/api/v3/lego";
/// Identifies the baker and derivation code that part revisions depend on.
pub const SYNTHESIS: &str = "ergo-part-synthesis-1";
pub const POINTER: &str = "catalog/part-library/current.json";
pub const REVIEWS: &str = include_str!("../../world/library/reviews.json");
const MAX_ARCHIVE: usize = 300 * 1024 * 1024;
const MAX_SOURCE: usize = 1024 * 1024;

pub fn library_key(revision: &str) -> String {
    format!("catalog/part-library/{revision}/library.json.gz")
}
pub fn input_key(sha256: &str) -> String {
    format!("catalog/inputs/{sha256}")
}
pub fn object_key(mesh_hash: &str) -> String {
    format!("objects/{mesh_hash}.cbor")
}

/// Named synthesis inputs. Names are stable keys in `Library::inputs`.
#[derive(Clone, Default)]
pub struct Inputs {
    pub files: BTreeMap<String, Vec<u8>>,
}
pub const LDRAW_INPUT: &str = "ldraw/complete.zip";
pub const REBRICKABLE_PARTS: &str = "rebrickable/parts.json";
pub const REBRICKABLE_CATEGORIES: &str = "rebrickable/part_categories.json";
pub const REVIEWS_INPUT: &str = "ergo/reviews.json";

impl Inputs {
    pub fn hashes(&self) -> BTreeMap<String, String> {
        self.files
            .iter()
            .map(|(name, bytes)| (name.clone(), sha256(bytes)))
            .collect()
    }
}

pub struct Synthesis {
    pub library: Library,
    /// Library JSON exactly as published (before transport compression).
    pub json: Vec<u8>,
    pub report: Report,
}

/// Receives each part's native Mesh CBOR as soon as it is baked, so the
/// complete object set (gigabytes) is never held in memory. Called from
/// synthesis worker threads.
pub type ObjectSink<'a> = dyn Fn(&LibraryPart, Vec<u8>) -> Result<()> + Sync + 'a;

#[derive(Debug, Default, Serialize)]
pub struct Report {
    pub revision: String,
    pub reviewed: usize,
    pub derived: usize,
    pub listed: usize,
    pub aliases: usize,
    pub derived_with_studs: usize,
    pub derived_with_sockets: usize,
    pub mass_calibration: Option<MassCalibration>,
    /// Official parts that could not be baked or profiled, with reasons.
    pub excluded: BTreeMap<String, String>,
    pub objects_written: usize,
}

fn sha256(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

// ---------------------------------------------------------------- pulling

/// Download the official archive and, with a key, the Rebrickable catalogue.
pub async fn pull(rebrickable_key: Option<&str>) -> Result<Inputs> {
    let http = reqwest::Client::builder()
        .user_agent(concat!(
            "Ergo/",
            env!("CARGO_PKG_VERSION"),
            " part-library synthesis"
        ))
        .build()?;
    let mut inputs = Inputs::default();
    let archive = http
        .get(LDRAW_ARCHIVE_URL)
        .send()
        .await?
        .error_for_status()?;
    ensure!(
        archive
            .url()
            .as_str()
            .starts_with("https://library.ldraw.org/"),
        "Unexpected LDraw archive redirect"
    );
    let bytes = archive.bytes().await?;
    ensure!(bytes.len() <= MAX_ARCHIVE, "LDraw archive exceeds 300 MiB");
    inputs.files.insert(LDRAW_INPUT.into(), bytes.to_vec());
    if let Some(key) = rebrickable_key {
        let parts =
            rebrickable_pages(&http, key, "parts/?page_size=1000&inc_part_details=1").await?;
        let parts: BTreeMap<String, RebrickablePart> = parts
            .into_iter()
            .map(|value| {
                let part: RebrickablePart = serde_json::from_value(value)?;
                Ok((part.part_num.clone(), part))
            })
            .collect::<Result<_>>()?;
        inputs
            .files
            .insert(REBRICKABLE_PARTS.into(), serde_json::to_vec(&parts)?);
        let categories = rebrickable_pages(&http, key, "part_categories/?page_size=1000").await?;
        let categories: BTreeMap<u32, String> = categories
            .into_iter()
            .map(|value| {
                let category: RebrickableCategory = serde_json::from_value(value)?;
                Ok((category.id, category.name))
            })
            .collect::<Result<_>>()?;
        inputs.files.insert(
            REBRICKABLE_CATEGORIES.into(),
            serde_json::to_vec(&categories)?,
        );
    }
    inputs
        .files
        .insert(REVIEWS_INPUT.into(), REVIEWS.as_bytes().to_vec());
    Ok(inputs)
}

async fn rebrickable_pages(
    http: &reqwest::Client,
    key: &str,
    query: &str,
) -> Result<Vec<serde_json::Value>> {
    #[derive(Deserialize)]
    struct Page {
        next: Option<String>,
        results: Vec<serde_json::Value>,
    }
    let mut url = format!("{REBRICKABLE_API}/{query}");
    let mut out = Vec::new();
    loop {
        ensure!(
            url.starts_with("https://rebrickable.com/api/v3/"),
            "Unexpected Rebrickable page URL"
        );
        let page: Page = serde_json::from_slice(
            &http
                .get(&url)
                .header("Authorization", format!("key {key}"))
                .send()
                .await?
                .error_for_status()?
                .bytes()
                .await?,
        )?;
        out.extend(page.results);
        ensure!(
            out.len() <= 500_000,
            "Rebrickable catalogue is unexpectedly large"
        );
        match page.next {
            Some(next) => url = next,
            None => return Ok(out),
        }
        // Rebrickable allows about one request per second per key.
        tokio::time::sleep(std::time::Duration::from_millis(1100)).await;
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct RebrickablePart {
    pub part_num: String,
    pub name: String,
    #[serde(default)]
    pub part_cat_id: Option<u32>,
    #[serde(default)]
    pub external_ids: RebrickableExternal,
}
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct RebrickableExternal {
    #[serde(default, rename = "BrickLink", skip_serializing_if = "Vec::is_empty")]
    pub bricklink: Vec<String>,
    #[serde(default, rename = "LDraw", skip_serializing_if = "Vec::is_empty")]
    pub ldraw: Vec<String>,
}
#[derive(Deserialize)]
struct RebrickableCategory {
    id: u32,
    name: String,
}

// ---------------------------------------------------------------- archive

/// Random access to the official archive's `parts/` and `p/` text files.
#[derive(Clone)]
struct Archive {
    bytes: Arc<[u8]>,
    /// Lower-case path relative to `ldraw/` -> zip index.
    names: Arc<HashMap<String, usize>>,
}
impl Archive {
    fn open(bytes: &[u8]) -> Result<Self> {
        let bytes: Arc<[u8]> = bytes.into();
        let mut zip = zip::ZipArchive::new(Cursor::new(bytes.clone()))?;
        let mut names = HashMap::new();
        for index in 0..zip.len() {
            let file = zip.by_index_raw(index)?;
            if file.is_dir() {
                continue;
            }
            let name = file.name().replace('\\', "/").to_lowercase();
            ensure!(
                !name.split('/').any(|s| s == ".."),
                "Archive path traversal"
            );
            if let Some(rest) = name.strip_prefix("ldraw/")
                && (rest.starts_with("parts/") || rest.starts_with("p/"))
            {
                names.insert(rest.to_owned(), index);
            }
        }
        ensure!(!names.is_empty(), "Not an official LDraw archive");
        Ok(Self {
            bytes,
            names: Arc::new(names),
        })
    }
    fn reader(&self) -> Result<Reader> {
        Ok(Reader {
            archive: self.clone(),
            zip: RefCell::new(zip::ZipArchive::new(Cursor::new(self.bytes.clone()))?),
            texts: RefCell::new(Default::default()),
        })
    }
    /// Top-level part files (`parts/x.dat`), sorted.
    fn top_parts(&self) -> Vec<String> {
        let mut parts: Vec<String> = self
            .names
            .keys()
            .filter_map(|path| path.strip_prefix("parts/"))
            .filter(|name| !name.contains('/') && name.ends_with(".dat"))
            .map(str::to_owned)
            .collect();
        parts.sort();
        parts
    }
}

struct Reader {
    archive: Archive,
    zip: RefCell<zip::ZipArchive<Cursor<Arc<[u8]>>>>,
    /// Decompressed sources (mostly shared primitives), cleared at 32 MiB so
    /// a worker's memory stays bounded across the whole library.
    texts: RefCell<(usize, HashMap<String, Option<(String, String)>>)>,
}
const READER_CACHE: usize = 32 * 1024 * 1024;
impl Reader {
    /// Normalized text and archive path for an LDraw reference name, using
    /// the official search order: `parts/` for top parts and subparts,
    /// otherwise primitives in `p/` before `parts/`.
    fn file(&self, name: &str, top: bool) -> Option<(String, String)> {
        if let Some(hit) = self.texts.borrow().1.get(name) {
            return hit.clone();
        }
        let candidates = if top || name.starts_with("s/") {
            vec![format!("parts/{name}")]
        } else {
            vec![format!("p/{name}"), format!("parts/{name}")]
        };
        let found = candidates.into_iter().find_map(|path| {
            let index = *self.archive.names.get(&path)?;
            let mut zip = self.zip.borrow_mut();
            let mut file = zip.by_index(index).ok()?;
            if file.size() as usize > MAX_SOURCE {
                return None;
            }
            let mut text = String::new();
            file.read_to_string(&mut text).ok()?;
            Some((normalize(&text), path))
        });
        let mut texts = self.texts.borrow_mut();
        let bytes = found
            .as_ref()
            .map_or(0, |(text, path)| text.len() + path.len());
        if texts.0 + bytes > READER_CACHE {
            *texts = Default::default();
        }
        texts.0 += bytes;
        texts.1.insert(name.to_owned(), found.clone());
        found
    }
}

/// UTF-8 without BOM, CRLF/CR to LF, all other text preserved.
fn normalize(text: &str) -> String {
    text.trim_start_matches('\u{feff}')
        .replace("\r\n", "\n")
        .replace('\r', "\n")
}

// ---------------------------------------------------------------- headers

#[derive(Debug, Default)]
struct Header {
    description: String,
    kind: String,
    qualifiers: Vec<String>,
    category: Option<String>,
    keywords: Vec<String>,
}
fn header(text: &str) -> Header {
    let mut header = Header::default();
    for (n, line) in text.lines().enumerate() {
        let line = line.trim();
        let Some(rest) = line.strip_prefix('0') else {
            if line.starts_with('1') || line.starts_with('3') || line.starts_with('4') {
                break;
            }
            continue;
        };
        let rest = rest.trim();
        if n == 0 {
            header.description = rest.to_owned();
        } else if let Some(org) = rest.strip_prefix("!LDRAW_ORG") {
            let tokens: Vec<&str> = org.split_whitespace().collect();
            header.kind = tokens.first().unwrap_or(&"").to_string();
            header.qualifiers = tokens
                .iter()
                .skip(1)
                .take_while(|t| !["ORIGINAL", "UPDATE"].contains(&t.to_ascii_uppercase().as_str()))
                .map(|t| t.to_string())
                .collect();
        } else if let Some(category) = rest.strip_prefix("!CATEGORY") {
            header.category = Some(category.trim().to_owned()).filter(|c| !c.is_empty());
        } else if let Some(keywords) = rest.strip_prefix("!KEYWORDS") {
            header.keywords.extend(
                keywords
                    .split(',')
                    .map(|k| k.trim().to_owned())
                    .filter(|k| !k.is_empty()),
            );
        }
    }
    header
}

/// The target of a file whose only geometry is one identity placement of
/// another part (official moved and alias files). An origin-changing alias
/// would need a pose migration and is not treated as an identity.
fn identity_target(text: &str) -> Option<String> {
    let mut target = None;
    for line in text.lines() {
        let tokens: Vec<&str> = line.split_whitespace().collect();
        match tokens.first().copied() {
            Some("1") => {
                let reference = ldraw::reference(&tokens).ok()?;
                if target.is_some()
                    || !matches!(reference.color, 16)
                    || reference.position.length() > 1e-9
                    || (reference.transform - glam::DMat3::IDENTITY)
                        .to_cols_array()
                        .iter()
                        .any(|v| v.abs() > 1e-9)
                {
                    return None;
                }
                target = Some(reference.name);
            }
            Some("2" | "3" | "4" | "5") => return None,
            _ => {}
        }
    }
    target
}

// ------------------------------------------------------------- synthesis

#[derive(Debug)]
struct Candidate {
    id: String,
    header: Header,
    obsolete: bool,
}

/// Pure synthesis from input bytes. `workers` only affects speed; every
/// geometry object goes to `sink`.
pub fn synthesize(inputs: &Inputs, workers: usize, sink: &ObjectSink) -> Result<Synthesis> {
    let archive = Archive::open(
        inputs
            .files
            .get(LDRAW_INPUT)
            .context("Missing LDraw archive input")?,
    )?;
    let reviews: Reviews = serde_json::from_slice(
        inputs
            .files
            .get(REVIEWS_INPUT)
            .context("Missing reviews input")?,
    )?;
    ensure!(
        reviews.format == "ergo-part-reviews-1",
        "Unsupported reviews format"
    );
    let reviews: BTreeMap<String, Review> = reviews
        .parts
        .into_iter()
        .map(|review| (review.id.clone(), review))
        .collect();
    let rebrickable: BTreeMap<String, RebrickablePart> = inputs
        .files
        .get(REBRICKABLE_PARTS)
        .map(|bytes| serde_json::from_slice(bytes))
        .transpose()?
        .unwrap_or_default();
    let categories: BTreeMap<u32, String> = inputs
        .files
        .get(REBRICKABLE_CATEGORIES)
        .map(|bytes| serde_json::from_slice(bytes))
        .transpose()?
        .unwrap_or_default();

    // 1. Classify every official top-level part file.
    let reader = archive.reader()?;
    let mut candidates = BTreeMap::new();
    let mut redirects: BTreeMap<String, String> = BTreeMap::new();
    for id in archive.top_parts() {
        let Some((text, _)) = reader.file(&id, true) else {
            continue;
        };
        let header = header(&text);
        let qualifiers: Vec<String> = header.qualifiers.iter().map(|q| q.to_lowercase()).collect();
        if header.description.starts_with("~Moved to") || qualifiers.iter().any(|q| q == "alias") {
            if let Some(target) = identity_target(&text) {
                redirects.insert(id, target);
            }
            continue;
        }
        let kind = header.kind.to_lowercase();
        if !["part", "shortcut"].contains(&kind.as_str())
            || qualifiers
                .iter()
                .any(|q| q == "physical_colour" || q == "flexible_section")
        {
            continue;
        }
        let obsolete = header.description.starts_with('~') || header.description.starts_with('_');
        candidates.insert(
            id.clone(),
            Candidate {
                id,
                header,
                obsolete,
            },
        );
    }
    let canonical = |id: &str| -> Option<String> {
        let mut id = id.to_owned();
        for _ in 0..16 {
            if candidates.contains_key(&id) {
                return Some(id);
            }
            id = redirects.get(&id)?.clone();
        }
        None
    };

    // 2. Map Rebrickable parts onto canonical LDraw geometry.
    let mut referenced = BTreeSet::new();
    let mut bricklink: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    let mut rebrickable_of: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    let mut listed = Vec::new();
    for (num, part) in &rebrickable {
        let mut targets: BTreeSet<String> = part
            .external_ids
            .ldraw
            .iter()
            .chain(std::iter::once(num))
            .filter_map(|id| ldraw::normalize_name(&format!("{id}.dat")).ok())
            .filter_map(|id| canonical(&id))
            .collect();
        if targets.is_empty() {
            if let Some(first) = part.external_ids.bricklink.first() {
                listed.push(Listed {
                    bricklink: first.clone(),
                    name: part.name.clone(),
                    category: part.part_cat_id.and_then(|c| categories.get(&c).cloned()),
                    rebrickable: Some(num.clone()),
                });
            }
            continue;
        }
        // An explicit LDraw mapping wins over a coincidental number match.
        if targets.len() > 1 && !part.external_ids.ldraw.is_empty() {
            targets = part
                .external_ids
                .ldraw
                .iter()
                .filter_map(|id| ldraw::normalize_name(&format!("{id}.dat")).ok())
                .filter_map(|id| canonical(&id))
                .collect();
        }
        for target in targets {
            referenced.insert(target.clone());
            bricklink
                .entry(target.clone())
                .or_default()
                .extend(part.external_ids.bricklink.iter().cloned());
            rebrickable_of
                .entry(target)
                .or_default()
                .insert(num.clone());
        }
    }
    listed.sort_by(|a, b| a.bricklink.cmp(&b.bricklink));
    listed.dedup_by(|a, b| a.bricklink == b.bricklink);

    // 3. Universe: current official parts, obsolete files only when reviewed
    //    or still referenced by the BrickLink/Rebrickable catalogue.
    for id in reviews.keys() {
        ensure!(
            candidates.contains_key(id),
            "Reviewed part {id} is not an official part file"
        );
    }
    let universe: Vec<&Candidate> = candidates
        .values()
        .filter(|c| !c.obsolete || reviews.contains_key(&c.id) || referenced.contains(&c.id))
        .collect();
    let in_universe: BTreeSet<&str> = universe.iter().map(|c| c.id.as_str()).collect();

    // 4. Identity aliases: official redirects, then non-conflicting BrickLink numbers.
    let mut aliases: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for alias in redirects.keys() {
        if let Some(target) = canonical(alias)
            && in_universe.contains(target.as_str())
        {
            aliases.entry(target).or_default().insert(alias.clone());
        }
    }
    let mut claimed: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for (target, numbers) in &bricklink {
        if !in_universe.contains(target.as_str()) {
            continue;
        }
        for number in numbers {
            if let Ok(id) = ldraw::normalize_name(&format!("{number}.dat"))
                && ergo_world::catalog::valid_part(&id)
                && &id != target
            {
                claimed.entry(id).or_default().insert(target.clone());
            }
        }
    }
    let official: BTreeSet<&str> = candidates
        .keys()
        .chain(redirects.keys())
        .map(String::as_str)
        .collect();
    for (alias, targets) in claimed {
        // Ambiguous numbers and real LDraw file names never become aliases.
        if targets.len() == 1 && !official.contains(alias.as_str()) {
            aliases
                .entry(targets.into_iter().next().unwrap())
                .or_default()
                .insert(alias);
        }
    }

    // 5. Calibrate the ABS voxel mass estimate against reviewed catalogue masses.
    let workers = workers.max(1);
    let reviewed: Vec<&Candidate> = universe
        .iter()
        .copied()
        .filter(|c| reviews.contains_key(&c.id))
        .collect();
    let mut ratios = Vec::new();
    for (id, baked) in parallel(&archive, &reviewed, workers, |reader, c| {
        bake(reader, &c.id, true)
    })? {
        let baked = baked.with_context(|| format!("Reviewed part {id} failed to bake"))?;
        if baked.abs_mass_kg > 0. {
            ratios.push(reviews[&id].mass_kg / baked.abs_mass_kg);
        }
    }
    ensure!(
        ratios.len() >= 20,
        "Too few reviewed parts to calibrate mass"
    );
    ratios.sort_by(f64::total_cmp);
    let factor = round(ratios[ratios.len() / 2], 1e-4);
    let mut errors: Vec<f64> = ratios.iter().map(|r| (factor / r - 1.).abs()).collect();
    errors.sort_by(f64::total_cmp);
    let calibration = MassCalibration {
        factor,
        samples: ratios.len(),
        p50: round(errors[errors.len() / 2], 1e-4),
        p90: round(errors[errors.len() * 9 / 10], 1e-4),
    };

    // 6. Bake, profile and emit every part in parallel; results are ordered by ID.
    let context = Records {
        reviews: &reviews,
        rebrickable: &rebrickable,
        rebrickable_of: &rebrickable_of,
        categories: &categories,
        bricklink: &bricklink,
        aliases: &aliases,
        calibration: &calibration,
    };
    let records = parallel(&archive, &universe, workers, |reader, candidate| {
        let baked = match bake(reader, &candidate.id, reviews.contains_key(&candidate.id)) {
            Ok(baked) => baked,
            Err(error) => {
                ensure!(
                    !reviews.contains_key(&candidate.id),
                    "Reviewed part {} failed to bake: {error:#}",
                    candidate.id
                );
                return Ok(Err(format!("{error:#}")));
            }
        };
        let (entry, cbor) = context.record(candidate, baked)?;
        sink(&entry, cbor)?;
        Ok(Ok(entry))
    })?;
    let mut report = Report::default();
    let mut parts = BTreeMap::new();
    for (id, record) in records {
        match record? {
            Ok(entry) => {
                let catalogue = &entry.catalogue;
                if catalogue.derivation.is_none() {
                    report.reviewed += 1;
                } else {
                    let nominal = catalogue.nominal.as_ref().expect("derived nominal");
                    report.derived += 1;
                    report.derived_with_studs += usize::from(!nominal.studs_ldu.is_empty());
                    report.derived_with_sockets += usize::from(nominal.cavity.is_some());
                }
                report.aliases += catalogue.aliases.len();
                parts.insert(id, entry);
            }
            Err(reason) => {
                report.excluded.insert(id, reason);
            }
        }
    }
    // A Rebrickable item mapped only onto an excluded part stays listed.
    for (id, nums) in &rebrickable_of {
        if !parts.contains_key(id) {
            for num in nums {
                if let Some(first) = rebrickable[num].external_ids.bricklink.first() {
                    listed.push(Listed {
                        bricklink: first.clone(),
                        name: rebrickable[num].name.clone(),
                        category: rebrickable[num]
                            .part_cat_id
                            .and_then(|c| categories.get(&c).cloned()),
                        rebrickable: Some(num.clone()),
                    });
                }
            }
        }
    }
    listed.sort_by(|a, b| a.bricklink.cmp(&b.bricklink));
    listed.dedup_by(|a, b| a.bricklink == b.bricklink);
    report.listed = listed.len();
    let mut library = Library {
        format: FORMAT.into(),
        revision: String::new(),
        inputs: inputs.hashes(),
        derivation: derive::DERIVATION.into(),
        mass_calibration: calibration.clone(),
        parts,
        listed,
    };
    library.revision = ergo_world::part_library::content_revision(&library)?;
    ergo_world::part_library::validate(&library)?;
    let json = serde_json::to_vec(&library)?;
    report.revision = library.revision.clone();
    report.mass_calibration = Some(calibration);
    Ok(Synthesis {
        library,
        json,
        report,
    })
}

/// Run `f` over candidates on `workers` threads, each with its own archive
/// reader; results come back ordered by part ID.
fn parallel<T: Send>(
    archive: &Archive,
    candidates: &[&Candidate],
    workers: usize,
    f: impl Fn(&Reader, &Candidate) -> T + Sync,
) -> Result<Vec<(String, T)>> {
    let f = &f;
    let mut out = std::thread::scope(|scope| {
        let handles: Vec<_> = (0..workers)
            .map(|w| {
                let archive = archive.clone();
                let chunk: Vec<&Candidate> = candidates
                    .iter()
                    .skip(w)
                    .step_by(workers)
                    .copied()
                    .collect();
                scope.spawn(move || -> Result<Vec<(String, T)>> {
                    let reader = archive.reader()?;
                    Ok(chunk
                        .into_iter()
                        .map(|c| (c.id.clone(), f(&reader, c)))
                        .collect())
                })
            })
            .collect();
        let mut out = Vec::new();
        for handle in handles {
            out.extend(
                handle
                    .join()
                    .map_err(|_| anyhow::anyhow!("Synthesis worker panicked"))??,
            );
        }
        Ok::<_, anyhow::Error>(out)
    })?;
    out.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(out)
}

/// Shared read-only inputs for building one part's catalogue record.
struct Records<'a> {
    reviews: &'a BTreeMap<String, Review>,
    rebrickable: &'a BTreeMap<String, RebrickablePart>,
    rebrickable_of: &'a BTreeMap<String, BTreeSet<String>>,
    categories: &'a BTreeMap<u32, String>,
    bricklink: &'a BTreeMap<String, BTreeSet<String>>,
    aliases: &'a BTreeMap<String, BTreeSet<String>>,
    calibration: &'a MassCalibration,
}

impl Records<'_> {
    /// The library entry and native Mesh CBOR of one baked part.
    fn record(&self, candidate: &Candidate, baked: Baked) -> Result<(LibraryPart, Vec<u8>)> {
        let id = &candidate.id;
        let header = &candidate.header;
        let rebrickable_ids = self.rebrickable_of.get(id);
        let category = header
            .category
            .clone()
            .or_else(|| {
                rebrickable_ids
                    .and_then(|ids| ids.iter().next())
                    .and_then(|num| self.rebrickable[num].part_cat_id)
                    .and_then(|c| self.categories.get(&c).cloned())
            })
            .or_else(|| {
                header
                    .description
                    .trim_start_matches(['~', '_', '='])
                    .split_whitespace()
                    .next()
                    .map(str::to_owned)
            });
        let mut catalogue = Part {
            id: id.clone(),
            name: display_name(&header.description),
            category,
            keywords: header.keywords.clone(),
            bricklink: self
                .bricklink
                .get(id)
                .map(|b| b.iter().cloned().collect())
                .unwrap_or_default(),
            rebrickable: rebrickable_ids.and_then(|ids| ids.iter().next().cloned()),
            mass_kg: 0.,
            mass_source: String::new(),
            size: None,
            studs: None,
            aliases: self
                .aliases
                .get(id)
                .map(|a| a.iter().cloned().collect())
                .unwrap_or_default(),
            set_count: None,
            nominal: None,
            connectors: Vec::new(),
            derivation: None,
        };
        if let Some(review) = self.reviews.get(id) {
            catalogue.mass_kg = review.mass_kg;
            catalogue.mass_source = review.mass_source.clone();
            catalogue.size = review.size;
            catalogue.studs = review.studs;
            catalogue.set_count = review.set_count;
            catalogue.nominal = review.nominal.clone();
            catalogue.connectors = review.connectors.clone();
        } else {
            let factor = self.calibration.factor;
            catalogue.mass_kg = round(baked.abs_mass_kg * factor, 1e-6).max(1e-6);
            catalogue.mass_source = format!(
                "estimated:voxel-v1 ABS {} g/cm3 x calibration {factor} (reviewed-set p50 {:.0}%, p90 {:.0}%)",
                derive::ABS_DENSITY,
                self.calibration.p50 * 100.,
                self.calibration.p90 * 100.
            );
            catalogue.nominal = Some(baked.nominal.clone());
            catalogue.derivation = Some(derive::DERIVATION.into());
        }
        let mut mesh = baked.mesh;
        let nominal = ergo_world::geometry::catalogue_body_bounds(&catalogue)
            .context("Catalogue record without body bounds")?;
        mesh.body_bounds = nominal;
        if let Some(size) = catalogue.size {
            let studs = if catalogue.studs == Some(true) {
                1. / 6.
            } else {
                0.
            };
            ensure!(
                nominal
                    .union(ergo_world::Aabb::new(
                        nominal.max,
                        nominal.max + glam::DVec3::Y * studs
                    ))
                    .expanded(0.05)
                    .contains(mesh.bounds),
                "Official geometry of {id} does not match its reviewed {size:?} dimensions"
            );
        }
        let mut cbor = Vec::new();
        ciborium::into_writer(&mesh, &mut cbor)?;
        let revision = {
            let mut digest = Sha256::new();
            digest.update(SYNTHESIS.as_bytes());
            for (name, hash) in &baked.closure {
                digest.update(name.as_bytes());
                digest.update(b"\0");
                digest.update(hash.as_bytes());
            }
            digest.update(serde_json::to_vec(&catalogue)?);
            hex::encode(digest.finalize())
        };
        Ok((
            LibraryPart {
                revision,
                vertex_hash: baked.vertex_hash,
                mesh_hash: sha256(&cbor),
                catalogue,
            },
            cbor,
        ))
    }
}

/// Official description with LDraw column padding collapsed and dimension
/// separators typeset: `Brick  2 x  4` becomes `Brick 2 × 4`.
fn display_name(description: &str) -> String {
    let tokens: Vec<&str> = description.split_whitespace().collect();
    let numeric = |t: Option<&&str>| t.is_some_and(|t| t.starts_with(|c: char| c.is_ascii_digit()));
    tokens
        .iter()
        .enumerate()
        .map(|(i, token)| {
            if *token == "x" && i > 0 && numeric(tokens.get(i - 1)) && numeric(tokens.get(i + 1)) {
                "×"
            } else {
                token
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn round(value: f64, step: f64) -> f64 {
    (value / step).round() * step
}

struct Baked {
    mesh: Mesh,
    nominal: ergo_world::part_profiles::Nominal,
    abs_mass_kg: f64,
    vertex_hash: String,
    /// Every resolved source file and the SHA-256 of its normalized text.
    closure: BTreeMap<String, String>,
}

fn bake(reader: &Reader, id: &str, reviewed: bool) -> Result<Baked> {
    let closure = RefCell::new(BTreeMap::new());
    let credits = RefCell::new(Vec::new());
    let resolve = |name: &str| {
        let (text, path) = reader.file(name, name == id)?;
        if !closure.borrow().contains_key(name) {
            closure
                .borrow_mut()
                .insert(name.to_owned(), sha256(text.as_bytes()));
            let field = |prefix: &str, default: &str| {
                text.lines()
                    .find_map(|l| l.strip_prefix(prefix))
                    .unwrap_or(default)
                    .trim()
                    .to_owned()
            };
            credits.borrow_mut().push(Credit {
                file: name.to_owned(),
                source_url: format!("{LDRAW_AUTHORITY}/{path}"),
                author: field("0 Author:", "LDraw contributors"),
                license: field("0 !LICENSE", "See upstream part license"),
            });
        }
        Some(text)
    };
    let derived = derive::derive(id, &resolve)?;
    let abs_mass_kg = derived.abs_mass_kg();
    let mut mesh = derived.mesh;
    let nominal = derived.nominal;
    ensure!(
        reviewed || abs_mass_kg > 0.,
        "No enclosed volume for a mass estimate"
    );
    mesh.credits = credits.into_inner();
    let mut hash = Sha256::new();
    for value in &mesh.vertices {
        hash.update(ergo_world::geometry::vertex_bytes(*value));
    }
    Ok(Baked {
        mesh,
        nominal,
        abs_mass_kg,
        vertex_hash: hex::encode(hash.finalize()),
        closure: closure.into_inner(),
    })
}

// ------------------------------------------------------------- publishing

fn path(prefix: &str, key: &str) -> Path {
    Path::from(if prefix.is_empty() {
        key.to_owned()
    } else {
        format!("{prefix}/{key}")
    })
}

async fn get(store: &dyn ObjectStore, prefix: &str, key: &str) -> Result<Option<Vec<u8>>> {
    match store.get(&path(prefix, key)).await {
        Ok(result) => Ok(Some(result.bytes().await?.to_vec())),
        Err(object_store::Error::NotFound { .. }) => Ok(None),
        Err(error) => Err(error.into()),
    }
}

async fn put_absent(
    store: &dyn ObjectStore,
    prefix: &str,
    key: String,
    bytes: Vec<u8>,
) -> Result<bool> {
    let location = path(prefix, &key);
    match store.head(&location).await {
        Ok(_) => Ok(false),
        Err(object_store::Error::NotFound { .. }) => {
            store.put(&location, PutPayload::from(bytes)).await?;
            Ok(true)
        }
        Err(error) => Err(error.into()),
    }
}

fn gzip(bytes: &[u8]) -> Result<Vec<u8>> {
    let mut encoder = flate2::GzBuilder::new()
        .mtime(0)
        .write(Vec::new(), flate2::Compression::best());
    encoder.write_all(bytes)?;
    Ok(encoder.finish()?)
}

#[derive(Deserialize, Serialize)]
struct Pointer {
    format: String,
    revision: String,
}

/// Synthesize from `inputs` and, unless `dry_run`, publish: inputs, then
/// every object streamed to the store while baking (memory stays bounded),
/// then the library document, and finally the `current` pointer.
pub async fn sync(
    store: Arc<dyn ObjectStore>,
    prefix: &str,
    inputs: Inputs,
    workers: usize,
    dry_run: bool,
) -> Result<Synthesis> {
    if !dry_run {
        for bytes in inputs.files.values() {
            put_absent(&*store, prefix, input_key(&sha256(bytes)), bytes.clone()).await?;
        }
    }
    let (sender, mut receiver) = tokio::sync::mpsc::channel::<(String, Vec<u8>)>(64);
    let writer = {
        let (store, prefix) = (store.clone(), prefix.to_owned());
        tokio::spawn(async move {
            let mut pending = futures::stream::FuturesUnordered::new();
            let mut written = 0;
            loop {
                tokio::select! {
                    next = receiver.recv(), if pending.len() < 32 => match next {
                        Some((hash, bytes)) => pending.push(put_absent(&*store, &prefix, object_key(&hash), bytes)),
                        None => break,
                    },
                    Some(done) = pending.next(), if !pending.is_empty() => {
                        written += usize::from(done?);
                    }
                }
            }
            while let Some(done) = pending.next().await {
                written += usize::from(done?);
            }
            Ok::<_, anyhow::Error>(written)
        })
    };
    let mut synthesis = tokio::task::spawn_blocking(move || {
        synthesize(&inputs, workers, &|entry, cbor| {
            if !dry_run {
                sender
                    .blocking_send((entry.mesh_hash.clone(), cbor))
                    .map_err(|_| anyhow::anyhow!("Object writer stopped"))?;
            }
            Ok(())
        })
    })
    .await??;
    synthesis.report.objects_written = writer.await??;
    if !dry_run {
        put_absent(
            &*store,
            prefix,
            library_key(&synthesis.library.revision),
            gzip(&synthesis.json)?,
        )
        .await?;
        store
            .put(
                &path(prefix, POINTER),
                PutPayload::from(serde_json::to_vec(&Pointer {
                    format: "ergo-part-library-pointer-1".into(),
                    revision: synthesis.library.revision.clone(),
                })?),
            )
            .await?;
    }
    Ok(synthesis)
}

/// The current published library and its exact JSON bytes, verified.
pub async fn load_current(
    store: &dyn ObjectStore,
    prefix: &str,
) -> Result<Option<(Library, Vec<u8>)>> {
    let Some(pointer) = get(store, prefix, POINTER).await? else {
        return Ok(None);
    };
    let pointer: Pointer = serde_json::from_slice(&pointer)?;
    ensure!(
        pointer.format == "ergo-part-library-pointer-1" && pointer.revision.len() == 64,
        "Unsupported part-library pointer"
    );
    let compressed = get(store, prefix, &library_key(&pointer.revision))
        .await?
        .with_context(|| format!("Missing part library {}", pointer.revision))?;
    let mut json = Vec::new();
    flate2::read::GzDecoder::new(compressed.as_slice())
        .take(512 * 1024 * 1024)
        .read_to_end(&mut json)?;
    let library = verify(&json)?;
    ensure!(
        library.revision == pointer.revision,
        "Part library pointer mismatch"
    );
    Ok(Some((library, json)))
}

/// Parse library JSON and verify its content revision.
pub fn verify(json: &[u8]) -> Result<Library> {
    ergo_world::part_library::verify(json)
}

/// Re-synthesize the current revision from its stored inputs.
pub async fn check(store: Arc<dyn ObjectStore>, prefix: &str, workers: usize) -> Result<Report> {
    let (library, json) = load_current(&*store, prefix)
        .await?
        .context("No part library has been published")?;
    let mut inputs = Inputs::default();
    for (name, hash) in &library.inputs {
        let bytes = get(&*store, prefix, &input_key(hash))
            .await?
            .with_context(|| format!("Missing stored input {name}"))?;
        ensure!(&sha256(&bytes) == hash, "Stored input {name} is corrupt");
        inputs.files.insert(name.clone(), bytes);
    }
    let synthesis = sync(store.clone(), prefix, inputs, workers, true).await?;
    if synthesis.json != json {
        bail!(
            "Re-synthesis produced revision {} instead of {}",
            synthesis.library.revision,
            library.revision
        );
    }
    let missing = futures::stream::iter(library.parts.values())
        .map(|entry| {
            let store = store.clone();
            async move {
                store
                    .head(&path(prefix, &object_key(&entry.mesh_hash)))
                    .await
                    .is_err()
                    .then(|| entry.catalogue.id.clone())
            }
        })
        .buffer_unordered(64)
        .filter_map(|missing| async move { missing })
        .collect::<Vec<_>>()
        .await;
    ensure!(missing.is_empty(), "Missing mesh objects for {missing:?}");
    Ok(synthesis.report)
}

/// True for parts whose objects belong in the committed test fixture.
pub fn in_fixture(entry: &LibraryPart) -> bool {
    entry.catalogue.derivation.is_none() || FIXTURE_DERIVED.contains(&entry.catalogue.id.as_str())
}

/// Derived parts included in the committed test fixture, so tests and
/// isolated browsers exercise automatically derived profiles too.
pub const FIXTURE_DERIVED: &[&str] = &[
    "2357.dat",
    "2412b.dat",
    "3040b.dat",
    "3794b.dat",
    "4070.dat",
];

/// The committed test fixture: the reviewed tier plus [`FIXTURE_DERIVED`],
/// as a self-consistent library, and its mesh objects as one deterministic
/// gzip CBOR map of mesh hash to bytes.
pub fn fixture(
    synthesis: &Synthesis,
    objects: &BTreeMap<String, Vec<u8>>,
) -> Result<(Vec<u8>, Vec<u8>)> {
    let mut library = synthesis.library.clone();
    library.parts.retain(|id, entry| {
        entry.catalogue.derivation.is_none() || FIXTURE_DERIVED.contains(&id.as_str())
    });
    ensure!(
        library
            .parts
            .values()
            .filter(|e| e.catalogue.derivation.is_some())
            .count()
            == FIXTURE_DERIVED.len(),
        "A fixture derived part is missing from the synthesis"
    );
    library.listed.truncate(3);
    library.revision = ergo_world::part_library::content_revision(&library)?;
    ergo_world::part_library::validate(&library)?;
    let objects: BTreeMap<&String, serde_bytes::ByteBuf> = library
        .parts
        .values()
        .map(|entry| {
            (
                &entry.mesh_hash,
                serde_bytes::ByteBuf::from(objects[&entry.mesh_hash].clone()),
            )
        })
        .collect();
    let mut cbor = Vec::new();
    ciborium::into_writer(&objects, &mut cbor)?;
    Ok((serde_json::to_vec_pretty(&library)?, gzip(&cbor)?))
}

/// Mirror the hosted origin's current library into a local store (MCP).
/// Falls back to the last verified local mirror when the origin is offline.
pub async fn mirror(
    store: &dyn ObjectStore,
    http: &reqwest::Client,
    origin: &str,
) -> Result<(Library, Vec<u8>)> {
    #[derive(Deserialize)]
    struct Config {
        part_library: String,
    }
    let origin = origin.trim_end_matches('/');
    let remote = async {
        let config: Config = http
            .get(format!("{origin}/api/config"))
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        if let Some((library, json)) = load_current(store, "").await.ok().flatten()
            && library.revision == config.part_library
        {
            return Ok::<_, anyhow::Error>((library, json));
        }
        let json = http
            .get(format!(
                "{origin}/api/library/{}/library.json",
                config.part_library
            ))
            .send()
            .await?
            .error_for_status()?
            .bytes()
            .await?
            .to_vec();
        let library = verify(&json)?;
        ensure!(
            library.revision == config.part_library,
            "Origin served another part library revision"
        );
        put_absent(store, "", library_key(&library.revision), gzip(&json)?).await?;
        store
            .put(
                &path("", POINTER),
                PutPayload::from(serde_json::to_vec(&Pointer {
                    format: "ergo-part-library-pointer-1".into(),
                    revision: library.revision.clone(),
                })?),
            )
            .await?;
        Ok((library, json))
    };
    match remote.await {
        Ok(found) => Ok(found),
        Err(error) => load_current(store, "")
            .await?
            .with_context(|| format!("Cannot fetch the part library from {origin}: {error:#}")),
    }
}

/// Publish a prepared library document with its objects (a gzip CBOR map of
/// mesh hash to bytes, as written by [`fixture`]). Every referenced
/// object must be present and match its hash; the pointer moves last.
/// Objects use the map format written by [`fixture`].
pub async fn publish_bundle(
    store: &dyn ObjectStore,
    prefix: &str,
    json: &[u8],
    objects: &[u8],
) -> Result<Library> {
    let library = verify(json)?;
    ergo_world::part_library::validate(&library)?;
    let mut cbor = Vec::new();
    flate2::read::GzDecoder::new(objects)
        .take(8 * 1024 * 1024 * 1024)
        .read_to_end(&mut cbor)?;
    let objects: BTreeMap<String, serde_bytes::ByteBuf> = ciborium::from_reader(cbor.as_slice())?;
    for entry in library.parts.values() {
        let bytes = objects
            .get(&entry.mesh_hash)
            .with_context(|| format!("Bundle lacks the geometry of {}", entry.catalogue.id))?;
        ensure!(
            sha256(bytes) == entry.mesh_hash,
            "Bundle geometry of {} is corrupt",
            entry.catalogue.id
        );
        put_absent(store, prefix, object_key(&entry.mesh_hash), bytes.to_vec()).await?;
    }
    put_absent(store, prefix, library_key(&library.revision), gzip(json)?).await?;
    store
        .put(
            &path(prefix, POINTER),
            PutPayload::from(serde_json::to_vec(&Pointer {
                format: "ergo-part-library-pointer-1".into(),
                revision: library.revision.clone(),
            })?),
        )
        .await?;
    Ok(library)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn names_collapse_padding_and_typeset_dimensions() {
        assert_eq!(display_name("Brick  2 x  4"), "Brick 2 × 4");
        assert_eq!(
            display_name("Support  2 x  2 x  7 Lamppost"),
            "Support 2 × 2 × 7 Lamppost"
        );
        assert_eq!(display_name("Minifig Hair x Style"), "Minifig Hair x Style");
    }
    #[test]
    fn identity_targets_require_one_untransformed_placement() {
        assert_eq!(
            identity_target("0 ~Moved to 3023b\n1 16 0 0 0 1 0 0 0 1 0 0 0 1 3023b.dat\n")
                .as_deref(),
            Some("3023b.dat")
        );
        assert!(identity_target("1 16 0 -8 0 1 0 0 0 1 0 0 0 1 3023b.dat\n").is_none());
        assert!(identity_target("1 4 0 0 0 1 0 0 0 1 0 0 0 1 3023b.dat\n").is_none());
        assert!(
            identity_target(
                "1 16 0 0 0 1 0 0 0 1 0 0 0 1 a.dat\n1 16 0 0 0 1 0 0 0 1 0 0 0 1 b.dat\n"
            )
            .is_none()
        );
    }
    #[test]
    fn headers_classify_official_parts() {
        let h = header(
            "0 Brick  2 x  4\n0 Name: 3001.dat\n0 !LDRAW_ORG Part UPDATE 2004-03\n0 !CATEGORY Brick\n0 !KEYWORDS a, b\n0 !KEYWORDS c\n1 16 0 0 0 1 0 0 0 1 0 0 0 1 s/3001s01.dat\n",
        );
        assert_eq!(h.kind, "Part");
        assert!(h.qualifiers.is_empty());
        assert_eq!(h.category.as_deref(), Some("Brick"));
        assert_eq!(h.keywords, ["a", "b", "c"]);
        let alias = header("0 =Brick  2 x  4\n0 !LDRAW_ORG Part Alias UPDATE 2010-01\n");
        assert_eq!(alias.qualifiers, ["Alias"]);
    }
}
