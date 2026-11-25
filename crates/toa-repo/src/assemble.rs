//! `toa assemble`: a testbench and the circuit tested, with its expanded tree, made into one
//! directory a simulator runs directly, for one set of conditions (7.6, "Assembling a run").
//!
//! What is done, in order; anything that cannot be done or told stops it, and nothing is written:
//!
//! 1. The testbench and the circuit are verified from their files (7.4); the tree is expanded by
//!    `uses`, each circuit fetched and verified once; it measures the interface of the circuit,
//!    and each interface has one circuit in the tree (`evd`, check 5).
//! 2. The conditions: a value for every parameter of the testbench and no other, every value
//!    single. The probes: a net for every probe the testbench lists and no other, each net found
//!    in the circuit tested (a net of its subcircuit, or instances down to it and the net).
//! 3. The local file map (`toa: local-files/1`): every process the tree names is in it; every
//!    model file of the conditions is found under its process, with `match` and `sha256`
//!    checked; every `external` of the tree and the testbench is matched by `name` (and
//!    `process`), with `match` and `sha256` checked.
//! 4. Subcircuits: the one of the circuit tested takes the name `socket`; the one of each circuit
//!    used takes the name its user instantiates (the key of `uses`); others are renamed where
//!    they clash with these, with each other or with those the testbench defines.
//! 5. The netlists of the testbench declare none of its parameters and set none of the options
//!    of the conditions; a Spectre netlist is not used in a SPICE run.
//! 6. Written: `tb/`, `ckt/<object>/`, the entry file (`input.scs` or `input.sp`) and
//!    `lock.json`, into a temporary directory beside the place, then renamed into place.
//!
//! Where the conditions go in the entry file is one constant, [`CONDITIONS_AT`]: whether Spectre
//! takes parameters declared after they are used, and options after the analyses, is still being
//! tried (B36); if not, the standard will put them before the testbench, and only that constant
//! changes.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use serde_json::{json, Value};
use toa_core::manifest::{self, Node};
use toa_core::problem::{has_errors, Problem};
use toa_core::{hash, name, tree};
use toa_types::reading::{self, Lang};

/// The format of the local file map.
pub const LOCAL_FILES: &str = "local-files/1";
/// The name a tool looks for in the directory of the workspace list (7.6).
pub const LOCAL_FILES_NAME: &str = "local-files.yaml";

/// Where the conditions of the run stand in the entry file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConditionsAt {
    /// After the netlist of the testbench, as 7.6 says now (item 5).
    AfterTestbench,
    /// Before the netlists of the circuits: what the standard will say if Spectre does not take
    /// parameters declared after they are used.
    BeforeNetlists,
}

/// Where this tool writes them: 7.6 as it stands.
pub const CONDITIONS_AT: ConditionsAt = ConditionsAt::AfterTestbench;

// ------------------------------------------------------------------ the local file map (7.6)

/// One model file of a process in the local file map.
#[derive(Debug, Clone)]
pub struct MapModel {
    pub file: PathBuf,
    /// The section names of the conditions to those of this file.
    pub sections: BTreeMap<String, String>,
}

/// A process in the local file map.
#[derive(Debug, Clone, Default)]
pub struct MapProcess {
    pub release: Option<String>,
    pub models: Vec<MapModel>,
}

/// An external file in the local file map.
#[derive(Debug, Clone)]
pub struct MapExternal {
    pub name: String,
    pub process: Option<String>,
    pub path: PathBuf,
}

/// The local file map.
#[derive(Debug, Clone, Default)]
pub struct LocalFiles {
    pub file: PathBuf,
    pub process: BTreeMap<String, MapProcess>,
    pub external: Vec<MapExternal>,
}

/// Read a local file map. Relative paths in it are relative to its directory.
pub fn read_local_files(file: &Path) -> Result<LocalFiles, String> {
    let bytes = std::fs::read(file).map_err(|e| format!("{}: {e}", file.display()))?;
    let root = manifest::parse(&bytes).map_err(|p| format!("{}: {}", file.display(), p[0]))?;
    if root.get("toa").and_then(Node::as_str) != Some(LOCAL_FILES) {
        return Err(format!("{} is not a local file map: its first key is toa: {LOCAL_FILES}", file.display()));
    }
    // the entry file is read in another directory: paths are made absolute
    let base = std::path::absolute(file)
        .ok()
        .and_then(|f| f.parent().map(Path::to_path_buf))
        .unwrap_or_else(|| file.parent().map(Path::to_path_buf).unwrap_or_default());
    let path_of = |s: &str| {
        let p = PathBuf::from(s);
        if p.is_absolute() {
            p
        } else {
            base.join(p)
        }
    };
    let mut out = LocalFiles { file: file.to_path_buf(), ..Default::default() };
    for (k, p) in root.get("process").and_then(Node::as_map).unwrap_or(&[]) {
        let mut mp =
            MapProcess { release: p.get("release").and_then(Node::as_str).map(str::to_string), models: Vec::new() };
        for m in p.get("models").and_then(Node::as_seq).unwrap_or(&[]) {
            let Some(f) = m.get("file").and_then(Node::as_str) else {
                return Err(format!("{}: a model of {} has no file", file.display(), k.text));
            };
            let sections = m
                .get("sections")
                .and_then(Node::as_map)
                .unwrap_or(&[])
                .iter()
                .filter_map(|(a, b)| b.as_str().map(|b| (a.text.clone(), b.to_string())))
                .collect();
            mp.models.push(MapModel { file: path_of(f), sections });
        }
        out.process.insert(k.text.clone(), mp);
    }
    for e in root.get("external").and_then(Node::as_seq).unwrap_or(&[]) {
        let (Some(n), Some(p)) = (e.get("name").and_then(Node::as_str), e.get("path").and_then(Node::as_str)) else {
            return Err(format!("{}: an external entry has no name or no path", file.display()));
        };
        out.external.push(MapExternal {
            name: n.to_string(),
            process: e.get("process").and_then(Node::as_str).map(str::to_string),
            path: path_of(p),
        });
    }
    Ok(out)
}

// ------------------------------------------------------------------ objects

/// An object read and verified.
#[derive(Debug, Clone)]
pub struct Loaded {
    pub name: String,
    pub dir: PathBuf,
    pub manifest: Node,
    pub files: Vec<(String, Vec<u8>)>,
}

impl Loaded {
    fn bytes(&self, p: &str) -> Option<&[u8]> {
        self.files.iter().find(|(q, _)| q == p).map(|(_, b)| b.as_slice())
    }
    fn str(&self, k: &str) -> Option<&str> {
        self.manifest.get(k).and_then(Node::as_str)
    }
    /// The paths of a role of `files`.
    fn role(&self, role: &str) -> Vec<String> {
        self.manifest
            .get("files")
            .and_then(|f| f.get(role))
            .and_then(Node::as_seq)
            .unwrap_or(&[])
            .iter()
            .filter_map(|p| p.as_str().map(str::to_string))
            .collect()
    }
    /// Every path of `files`, in the order of the roles, each once.
    fn listed(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        for r in ["netlist", "va", "rtl", "packages", "extras"] {
            for p in self.role(r) {
                if !out.contains(&p) {
                    out.push(p);
                }
            }
        }
        out
    }
}

/// Read an object directory, verified from its files (7.4).
pub fn load(dir: &Path) -> Result<Loaded, Vec<Problem>> {
    let name = dir.file_name().and_then(|n| n.to_str()).unwrap_or("").to_string();
    let io = |e: std::io::Error| {
        vec![Problem::error("E_ASSEMBLE_READ", "7.6", name.clone(), format!("{}: {e}", dir.display()))]
    };
    let v = crate::fs::verify_dir(dir).map_err(io)?;
    if has_errors(&v.problems) {
        return Err(v.problems);
    }
    if !matches!(name::form(&name), name::Form::Object) {
        return Err(vec![Problem::error("E_ASSEMBLE_NAME", "7.6", name, "a draft is not assembled: seal it first")]);
    }
    let c = crate::fs::read_dir(dir).map_err(io)?;
    let Some(manifest) = v.manifest else {
        return Err(vec![Problem::error("E_ASSEMBLE_READ", "7.6", name, "no manifest")]);
    };
    Ok(Loaded { name, dir: dir.to_path_buf(), manifest, files: c.files })
}

fn sha256(b: &[u8]) -> String {
    hash::hex(&hash::digest(hash::Algorithm::Sha256, b).unwrap_or_default())
}

// ------------------------------------------------------------------ the input and the result

/// One run to assemble.
#[derive(Debug, Clone)]
pub struct Input {
    /// The testbench and the circuit tested, as object directories.
    pub tb: PathBuf,
    pub dut: PathBuf,
    /// One set of conditions: `{parameters, options, models}`, as `evd` writes `conditions`.
    pub conditions: Value,
    /// The net of the circuit tested for each probe the testbench lists.
    pub probes: BTreeMap<String, String>,
    pub map: LocalFiles,
    /// The directory to write; it must not exist, or be empty.
    pub out: PathBuf,
    pub dry_run: bool,
    /// For a Monte Carlo simulation (the kind `mc`): the samples this run simulates.
    pub monte_carlo: Option<Samples>,
}

/// The name of the statement that holds the analyses of a Monte Carlo simulation (7.6).
pub const MC_STATEMENT: &str = "toaMonteCarlo";

/// The samples one simulation of a Monte Carlo simulation runs: all of them with `run: batch`,
/// one with `run: each` (7.6).
#[derive(Debug, Clone, PartialEq)]
pub struct Samples {
    /// `method`, `sampling`, `seed` and `points` of `statistics`.
    pub method: String,
    pub sampling: String,
    pub seed: i64,
    pub points: i64,
    /// The number of the first sample of this simulation, and how many it runs.
    pub first: i64,
    pub count: i64,
}

impl Samples {
    /// From `statistics` of the conditions: all the samples (`first`, `points`), or the one
    /// sample `one`.
    pub fn of(statistics: &Value, one: Option<i64>) -> Result<Samples, String> {
        let s = |k: &str| statistics.get(k).and_then(Value::as_str).map(str::to_string);
        let i = |k: &str| statistics.get(k).and_then(Value::as_i64);
        let method = s("method").filter(|m| toa_types::METHODS.contains(&m.as_str()));
        let sampling = s("sampling").filter(|m| toa_types::SAMPLINGS.contains(&m.as_str()));
        let (Some(method), Some(sampling), Some(seed), Some(points)) = (method, sampling, i("seed"), i("points"))
        else {
            return Err("statistics has method, points, sampling and seed (evd, the kind mc)".into());
        };
        if !(1..=toa_types::SEED_MAX).contains(&seed) || points < 1 {
            return Err("statistics: points is 1 or more, seed from 1 to 2147483647".into());
        }
        let first = i("first").unwrap_or(1);
        if first < 1 || (sampling == "lhs" && first != 1) {
            return Err("statistics: first is 1 or more, and 1 with lhs".into());
        }
        Ok(match one {
            Some(k) if k < first || k >= first + points => {
                return Err(format!("sample {k} is not one of {first} to {}", first + points - 1))
            }
            Some(k) => Samples { method, sampling, seed, points, first: k, count: 1 },
            None => Samples { method, sampling, seed, points, first, count: points },
        })
    }

    /// The statement, before its braces (7.6): `dut` lists the instances of the socket, for the
    /// mismatch to be applied to the block measured only.
    pub fn statement(&self, dut: &[String]) -> String {
        let variations = match self.method.as_str() {
            "mismatch" => "mismatch",
            "process" => "process",
            _ => "all",
        };
        let sampling = match self.sampling.as_str() {
            "random" => "standard",
            x => x,
        };
        let mut s = format!(
            "{MC_STATEMENT} montecarlo variations={variations} sampling={sampling} seed={} numruns={} firstrun={} donominal=no savefamilyplots=yes",
            self.seed, self.count, self.first
        );
        if variations != "process" && !dut.is_empty() {
            s.push_str(&format!(" dut=[{}]", dut.join(" ")));
        }
        if self.sampling == "lhs" {
            // the bins follow the whole run, so that sample k is the same however it is cut
            s.push_str(&format!(" numbins={}", self.points));
        }
        s
    }
}

/// A model file of the conditions as matched in the local file map.
#[derive(Debug, Clone)]
pub struct UsedModel {
    /// The item of `conditions.models`.
    pub item: Value,
    pub process: String,
    pub release: Option<String>,
    pub path: PathBuf,
    /// The sections written, as the file names them.
    pub sections: Vec<String>,
}

/// An external file of an object as matched in the local file map.
#[derive(Debug, Clone)]
pub struct UsedExternal {
    pub object: String,
    pub name: String,
    pub path: PathBuf,
}

/// What was (or would be) written.
#[derive(Debug, Clone)]
pub struct Assembled {
    pub dir: PathBuf,
    /// `input.scs` or `input.sp`.
    pub entry: String,
    pub lang: Lang,
    pub tb: Loaded,
    /// The circuits of the tree: (path, object, ifc), sorted by path.
    pub tree: Vec<(String, String, String)>,
    pub lock: Value,
    pub models: Vec<UsedModel>,
    pub externals: Vec<UsedExternal>,
    /// Subcircuits renamed: (object, from, to).
    pub renamed: Vec<(String, String, String)>,
    pub probes: BTreeMap<String, String>,
    pub conditions: Value,
    /// The objects of the tree as read and verified: name -> directory.
    pub objects: BTreeMap<String, PathBuf>,
}

/// Why nothing was written.
#[derive(Debug)]
pub enum AssembleError {
    /// What stops it.
    Problems(Vec<Problem>),
    /// Writing failed.
    Io(String),
}

impl std::fmt::Display for AssembleError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AssembleError::Problems(ps) => {
                write!(f, "{}", ps.iter().map(|p| p.message.clone()).collect::<Vec<_>>().join("; "))
            }
            AssembleError::Io(m) => write!(f, "{m}"),
        }
    }
}

fn stop(code: &'static str, at: impl Into<String>, m: impl Into<String>) -> AssembleError {
    AssembleError::Problems(vec![Problem::error(code, "7.6", at, m)])
}

/// Finds the circuits of the tree by name, as `toa fetch` does; the directory it gives is
/// verified again here.
pub trait Finder {
    fn find(&mut self, name: &str) -> Result<PathBuf, String>;
}

// ------------------------------------------------------------------ the tree

/// One circuit of the expanded tree.
#[derive(Debug, Clone)]
struct Row {
    path: String,
    object: String,
    ifc: String,
    /// The name its user instantiates: `socket` for the circuit tested, else the key of `uses`.
    called: String,
}

fn expand(
    dut: Loaded,
    socket: &str,
    finder: &mut dyn Finder,
) -> Result<(Vec<Row>, BTreeMap<String, Loaded>), AssembleError> {
    let mut objects: BTreeMap<String, Loaded> = BTreeMap::new();
    let mut rows: Vec<Row> = Vec::new();
    let mut todo: Vec<(String, String, String)> = vec![(String::new(), dut.name.clone(), socket.to_string())];
    objects.insert(dut.name.clone(), dut);
    while let Some((path, obj, called)) = todo.pop() {
        if path.split('.').count() > 64 {
            return Err(stop("E_ASSEMBLE_TREE", obj, "the tree is deeper than 64 levels"));
        }
        if !objects.contains_key(&obj) {
            let dir = finder.find(&obj).map_err(|m| stop("E_ASSEMBLE_FIND", obj.clone(), m))?;
            let l = load(&dir).map_err(AssembleError::Problems)?;
            objects.insert(obj.clone(), l);
        }
        let l = &objects[&obj];
        if l.str("type") != Some(toa_types::CKT) {
            return Err(stop("E_ASSEMBLE_TREE", obj.clone(), format!("{obj} at {path:?} is not a circuit")));
        }
        let ifc = l.str("ifc").unwrap_or("").to_string();
        for (k, u) in l.manifest.get("uses").and_then(Node::as_map).unwrap_or(&[]) {
            let Some(child) = u.get("ckt").and_then(Node::as_str) else { continue };
            let p = if path.is_empty() { k.text.clone() } else { format!("{path}.{}", k.text) };
            todo.push((p, child.to_string(), k.text.clone()));
        }
        rows.push(Row { path, object: obj, ifc, called });
    }
    rows.sort_by(|a, b| a.path.cmp(&b.path));
    // each interface has one circuit in the tree (evd, check 5)
    let mut by_ifc: BTreeMap<&str, BTreeSet<&str>> = BTreeMap::new();
    for r in &rows {
        by_ifc.entry(&r.ifc).or_default().insert(&r.object);
    }
    for (ifc, cs) in by_ifc {
        if cs.len() > 1 {
            return Err(stop(
                "E_TREE_TWO_CIRCUITS",
                ifc,
                format!("{ifc} has {} circuits in the tree; a simulation defines its subcircuit once", cs.len()),
            ));
        }
    }
    Ok((rows, objects))
}

// ------------------------------------------------------------------ values and lines

/// A value of the conditions as the entry file writes it: a number as a decimal number, a
/// boolean as `1` or `0`, a string as it is.
pub fn value_text(v: &Value) -> Option<String> {
    match v {
        Value::Bool(b) => Some(if *b { "1" } else { "0" }.into()),
        Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                Some(i.to_string())
            } else {
                n.as_f64().filter(|f| f.is_finite()).map(|f| format!("{f}"))
            }
        }
        Value::String(s) => Some(s.clone()),
        _ => None,
    }
}

/// The parts of the entry file, each a list of lines.
#[derive(Debug, Clone, Default)]
pub struct Entry {
    pub lang: Option<Lang>,
    pub title: String,
    /// Model files: path on this machine and the sections, as the file names them.
    pub models: Vec<(String, Vec<String>)>,
    /// Netlists of the circuits, relative to the directory.
    pub netlists: Vec<String>,
    /// The first netlist of the testbench, relative to the directory.
    pub testbench: String,
    pub parameters: Vec<(String, String)>,
    pub options: Vec<(String, String)>,
}

fn quoted(p: &str) -> String {
    format!("\"{}\"", p.replace('"', "\\\""))
}

/// The entry file (7.6): its lines in the order the standard gives, the conditions where
/// [`CONDITIONS_AT`] puts them.
pub fn entry_text(e: &Entry, at: ConditionsAt) -> String {
    let spectre = e.lang != Some(Lang::Spice);
    let mut lines: Vec<String> = Vec::new();
    // 1. the first line
    lines.push(if spectre { "simulator lang=spectre".into() } else { format!("* {}", e.title) });
    // 2. the model files
    for (path, sections) in &e.models {
        if sections.is_empty() {
            lines.push(if spectre {
                format!("include {}", quoted(path))
            } else {
                format!(".include {}", quoted(path))
            });
        }
        for s in sections {
            lines.push(if spectre {
                format!("include {} section={s}", quoted(path))
            } else {
                format!(".lib {} {s}", quoted(path))
            });
        }
    }
    let mut conditions: Vec<String> = Vec::new();
    if !e.parameters.is_empty() {
        let kv: Vec<String> = e.parameters.iter().map(|(k, v)| format!("{k}={v}")).collect();
        conditions.push(if spectre {
            format!("parameters {}", kv.join(" "))
        } else {
            format!(".param {}", kv.join(" "))
        });
    }
    if spectre {
        if !e.options.is_empty() {
            let kv: Vec<String> = e.options.iter().map(|(k, v)| format!("{k}={v}")).collect();
            conditions.push(format!("toaConditions options {}", kv.join(" ")));
        }
    } else {
        for (k, v) in e.options.iter().filter(|(k, _)| k == "temp") {
            let _ = k;
            conditions.push(format!(".temp {v}"));
        }
        let rest: Vec<String> =
            e.options.iter().filter(|(k, _)| k != "temp").map(|(k, v)| format!("{k}={v}")).collect();
        if !rest.is_empty() {
            conditions.push(format!(".options {}", rest.join(" ")));
        }
    }
    if at == ConditionsAt::BeforeNetlists {
        lines.extend(conditions.iter().cloned());
    }
    // 3. the netlists of the circuits, 4. the first netlist of the testbench
    let inc = |p: &str| if spectre { format!("include {}", quoted(p)) } else { format!(".include {}", quoted(p)) };
    lines.extend(e.netlists.iter().map(|p| inc(p)));
    lines.push(inc(&e.testbench));
    // 5. the conditions
    if at == ConditionsAt::AfterTestbench {
        lines.extend(conditions);
    }
    // 6. the end of a SPICE deck
    if !spectre {
        lines.push(".end".into());
    }
    lines.join("\n") + "\n"
}

// ------------------------------------------------------------------ assembling

/// The subcircuits an object's netlists define at their top level, and the one of its interface:
/// the only one the first netlist defines, or the only one of those no other instantiates.
fn defined(l: &Loaded) -> Result<(Vec<String>, String), AssembleError> {
    let mut all: Vec<String> = Vec::new();
    let mut first_defs: Vec<String> = Vec::new();
    let mut used: BTreeSet<String> = BTreeSet::new();
    for (i, p) in l.role("netlist").iter().enumerate() {
        let lang = Lang::of(p);
        let text = String::from_utf8_lossy(l.bytes(p).unwrap_or(&[])).to_string();
        let n = reading::read(&text, lang);
        for s in n.subckts.iter().filter(|s| s.parent.is_none()) {
            all.push(s.name.clone());
            if i == 0 {
                first_defs.push(s.name.clone());
            }
        }
        used.extend(n.instances.iter().filter(|x| x.parent.is_some()).map(|x| x.master.to_ascii_lowercase()));
    }
    let top = match first_defs.as_slice() {
        [one] => one.clone(),
        [] => {
            return Err(stop(
                "E_ASSEMBLE_SUBCKT",
                l.name.clone(),
                format!("the first netlist of {} defines no subcircuit", l.name),
            ))
        }
        many => {
            let tops: Vec<&String> = many.iter().filter(|s| !used.contains(&s.to_ascii_lowercase())).collect();
            match tops.as_slice() {
                [one] => (*one).clone(),
                _ => {
                    return Err(stop(
                        "E_ASSEMBLE_SUBCKT",
                        l.name.clone(),
                        format!(
                            "which subcircuit of the first netlist of {} is the block cannot be told: {}",
                            l.name,
                            many.join(", ")
                        ),
                    ))
                }
            }
        }
    };
    Ok((all, top))
}

/// The net a probe names, found in the circuit tested: a net of its subcircuit, or the instances
/// down to it and the net. `Err` says why it is not found or cannot be told.
fn find_net(
    net: &str,
    dut: &str,
    objects: &BTreeMap<String, Loaded>,
    tops: &BTreeMap<String, String>,
) -> Result<(), String> {
    let parts: Vec<&str> = net.split('.').collect();
    let mut obj = dut.to_string();
    let mut sub = tops.get(dut).cloned().ok_or("the subcircuit of the circuit tested is not known")?;
    for (i, part) in parts.iter().enumerate() {
        let l = &objects[&obj];
        let mut found_sub: Option<(reading::Subckt, Lang)> = None;
        let mut insts: Vec<reading::Instance> = Vec::new();
        for p in l.role("netlist") {
            let lang = Lang::of(&p);
            let n = reading::read(&String::from_utf8_lossy(l.bytes(&p).unwrap_or(&[])), lang);
            if let Some(s) = n.subckts.iter().find(|s| lang.same(&s.name, &sub)) {
                found_sub = Some((s.clone(), lang));
                insts.extend(
                    n.instances.into_iter().filter(|x| x.parent.as_deref().is_some_and(|q| lang.same(q, &sub))),
                );
            }
        }
        let Some((s, lang)) = found_sub else { return Err(format!("the subcircuit {sub} of {obj} is not found")) };
        if i + 1 == parts.len() {
            return if s.nets.iter().any(|n| lang.same(n, part)) {
                Ok(())
            } else {
                Err(format!("{sub} in {obj} has no net {part}"))
            };
        }
        let Some(inst) = insts.iter().find(|x| lang.same(&x.name, part)) else {
            return Err(format!("{sub} in {obj} has no instance {part}"));
        };
        // the master: a subcircuit of the same object, or a circuit it uses
        let (Ok((defs, _)), uses) = (defined(l), l.manifest.get("uses").and_then(Node::as_map).unwrap_or(&[])) else {
            return Err(format!("the subcircuits of {obj} cannot be read"));
        };
        if defs.iter().any(|d| d == &inst.master) {
            sub = inst.master.clone();
        } else if let Some((_, u)) = uses.iter().find(|(k, _)| k.text == inst.master) {
            let child = u.get("ckt").and_then(Node::as_str).unwrap_or("").to_string();
            sub = tops.get(&child).cloned().ok_or_else(|| format!("the subcircuit of {child} is not known"))?;
            obj = child;
        } else {
            return Err(format!(
                "the master {} of {part} in {obj} is neither a subcircuit there nor a circuit it uses",
                inst.master
            ));
        }
    }
    Err("an empty net".into())
}

/// Assemble one run.
pub fn assemble(input: &Input, finder: &mut dyn Finder) -> Result<Assembled, AssembleError> {
    // 1. the objects and the tree
    let tb = load(&input.tb).map_err(AssembleError::Problems)?;
    if tb.str("type") != Some(toa_types::TB) {
        return Err(stop("E_ASSEMBLE_TB", tb.name.clone(), format!("{} is not a testbench", tb.name)));
    }
    let Some(socket) = tb.str("socket").map(str::to_string) else {
        return Err(stop(
            "E_ASSEMBLE_TB",
            tb.name.clone(),
            format!("{} has no socket: a testbench written before B36 cannot be assembled", tb.name),
        ));
    };
    let dut = load(&input.dut).map_err(AssembleError::Problems)?;
    let dut_name = dut.name.clone();
    if dut.str("ifc") != tb.str("ifc") {
        return Err(stop(
            "E_TB_IFC",
            dut_name.clone(),
            format!(
                "{} measures {}, and {dut_name} implements {}",
                tb.name,
                tb.str("ifc").unwrap_or("?"),
                dut.str("ifc").unwrap_or("?")
            ),
        ));
    }
    let (rows, objects) = expand(dut, &socket, finder)?;

    // 2. the conditions and the probes
    let mut problems: Vec<Problem> = Vec::new();
    let mut p = |code: &'static str, at: &str, m: String| problems.push(Problem::error(code, "7.6", at.to_string(), m));
    let c = &input.conditions;
    let declared: Vec<String> = tb
        .manifest
        .get("parameters")
        .and_then(Node::as_map)
        .map(|m| m.iter().map(|(k, _)| k.text.clone()).collect())
        .unwrap_or_default();
    let mut parameters: Vec<(String, String)> = Vec::new();
    let given = c.get("parameters").and_then(Value::as_object).cloned().unwrap_or_default();
    for d in &declared {
        match given.get(d) {
            Some(v) => match value_text(v) {
                Some(t) => parameters.push((d.clone(), t)),
                None => {
                    p("E_CONDITIONS", &format!("parameters/{d}"), format!("{d}: a single number, string or boolean"))
                }
            },
            None => p("E_CONDITIONS", "parameters", format!("the parameter {d} of the testbench is given no value")),
        }
    }
    for g in given.keys().filter(|g| !declared.contains(g)) {
        p("E_CONDITIONS", &format!("parameters/{g}"), format!("{g} is not a parameter of the testbench"));
    }
    let mut options: Vec<(String, String)> = Vec::new();
    for (k, v) in c.get("options").and_then(Value::as_object).cloned().unwrap_or_default() {
        match value_text(&v) {
            Some(t) => options.push((k, t)),
            None => p("E_CONDITIONS", &format!("options/{k}"), format!("{k}: a single number, string or boolean")),
        }
    }
    match (c.get("statistics"), &input.monte_carlo) {
        (Some(_), None) => p(
            "E_ASSEMBLE_MC",
            "statistics",
            "conditions with statistics are a Monte Carlo simulation: say which samples to run".into(),
        ),
        (None, Some(_)) => {
            p("E_ASSEMBLE_MC", "statistics", "a Monte Carlo simulation needs statistics in its conditions".into())
        }
        _ => {}
    }
    let listed: Vec<String> = tb
        .manifest
        .get("probes")
        .and_then(Node::as_seq)
        .unwrap_or(&[])
        .iter()
        .filter_map(|x| x.as_str().map(str::to_string))
        .collect();
    for l in listed.iter().filter(|l| !input.probes.contains_key(*l)) {
        p("E_PROBES", "probes", format!("the testbench lists the probe {l}: give its net in the circuit tested"));
    }
    for k in input.probes.keys().filter(|k| !listed.contains(k)) {
        p("E_PROBES", &format!("probes/{k}"), format!("{k} is not a probe the testbench lists"));
    }

    // the subcircuits of each circuit, and the one of its interface
    let mut defs: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut tops: BTreeMap<String, String> = BTreeMap::new();
    for (n, l) in &objects {
        let (all, top) = defined(l)?;
        defs.insert(n.clone(), all);
        tops.insert(n.clone(), top);
    }
    for (probe, net) in &input.probes {
        if let Err(why) = find_net(net, &dut_name, &objects, &tops) {
            p(
                "E_PROBES",
                &format!("probes/{probe}"),
                format!("the net {net} of the probe {probe} is not found: {why}"),
            );
        }
    }

    // the testbench's netlists: the language of the run, and what they must not set
    let tb_netlists = tb.role("netlist");
    let lang = Lang::of(tb_netlists.first().map(String::as_str).unwrap_or("x.scs"));
    let mut tb_defs: Vec<String> = Vec::new();
    let mut socket_instances: Vec<String> = Vec::new();
    for path in &tb_netlists {
        let l = Lang::of(path);
        if lang == Lang::Spice && l == Lang::Spectre {
            p("E_ASSEMBLE_LANG", path, format!("{path} is in Spectre and the run is in SPICE"));
        }
        let n = reading::read(&String::from_utf8_lossy(tb.bytes(path).unwrap_or(&[])), l);
        tb_defs.extend(n.subckts.iter().filter(|s| s.parent.is_none()).map(|s| s.name.clone()));
        socket_instances.extend(n.instances.iter().filter(|i| l.same(&i.master, &socket)).map(|i| i.name.clone()));
        for (k, line) in &n.parameters {
            if declared.iter().any(|d| l.same(d, k)) {
                p(
                    "E_TB_CONDITIONS",
                    &format!("{path}:{line}"),
                    format!("the netlist declares the parameter {k} of the conditions"),
                );
            }
        }
        for (k, line) in &n.options {
            if options.iter().any(|(o, _)| l.same(o, k)) {
                p(
                    "E_TB_CONDITIONS",
                    &format!("{path}:{line}"),
                    format!("the netlist sets the option {k} of the conditions"),
                );
            }
        }
    }
    if socket_instances.is_empty() {
        p("E_TB_SOCKET", "", format!("the netlists of {} instantiate no {socket}", tb.name));
    }
    for r in &rows {
        for n in objects[&r.object].role("netlist") {
            if lang == Lang::Spice && Lang::of(&n) == Lang::Spectre {
                p(
                    "E_ASSEMBLE_LANG",
                    &format!("{}/{n}", r.object),
                    format!("{n} of {} is in Spectre and the run is in SPICE", r.object),
                );
            }
        }
    }

    // 3. the local file map
    let map = &input.map;
    let mut processes: BTreeSet<String> = BTreeSet::new();
    for r in &rows {
        if let Some(pr) =
            objects[&r.object].manifest.get("specifications").and_then(|s| s.get("process")).and_then(Node::as_str)
        {
            processes.insert(pr.to_string());
        }
    }
    for pr in &processes {
        if !map.process.contains_key(pr) {
            p(
                "E_LOCAL_FILES",
                pr,
                format!("the process {pr} of the tree is not in the local file map {}", map.file.display()),
            );
        }
    }
    let mut models: Vec<UsedModel> = Vec::new();
    for (i, m) in c.get("models").and_then(Value::as_array).cloned().unwrap_or_default().iter().enumerate() {
        let at = format!("models/{i}");
        let Some(file) = m.get("file").and_then(Value::as_str) else {
            p("E_CONDITIONS", &at, "a model file of the conditions has no file".into());
            continue;
        };
        let wanted: Vec<String> = match m.get("process").and_then(Value::as_str) {
            Some(x) => vec![x.to_string()],
            None => processes.iter().cloned().collect(),
        };
        let found: Vec<(String, &MapProcess, &MapModel)> = wanted
            .iter()
            .filter_map(|pr| map.process.get(pr).map(|mp| (pr, mp)))
            .flat_map(|(pr, mp)| {
                mp.models
                    .iter()
                    .filter(|mm| mm.file.file_name().and_then(|n| n.to_str()) == Some(file))
                    .map(move |mm| (pr.clone(), mp, mm))
            })
            .collect();
        let (pr, mp, mm) = match found.as_slice() {
            [one] => one.clone(),
            [] => {
                p(
                    "E_LOCAL_FILES",
                    &at,
                    format!("the model file {file} is not in the local file map under {}", wanted.join(", ")),
                );
                continue;
            }
            _ => {
                p("E_LOCAL_FILES", &at, format!("the model file {file} is in the local file map more than once"));
                continue;
            }
        };
        let bytes = match std::fs::read(&mm.file) {
            Ok(b) => b,
            Err(e) => {
                p("E_LOCAL_FILES", &at, format!("{}: {e}", mm.file.display()));
                continue;
            }
        };
        check_file(&mut p, &at, &mm.file, &bytes, m.get("match"), m.get("sha256"));
        let sections: Vec<String> = match m.get("section") {
            Some(Value::String(s)) => vec![s.clone()],
            Some(Value::Array(a)) => a.iter().filter_map(|x| x.as_str().map(str::to_string)).collect(),
            _ => Vec::new(),
        };
        let sections = sections.iter().map(|s| mm.sections.get(s).cloned().unwrap_or_else(|| s.clone())).collect();
        models.push(UsedModel {
            item: m.clone(),
            process: pr,
            release: mp.release.clone(),
            path: mm.file.clone(),
            sections,
        });
    }
    let mut externals: Vec<UsedExternal> = Vec::new();
    let mut with_external: Vec<&Loaded> = rows.iter().map(|r| &objects[&r.object]).collect();
    with_external.dedup_by(|a, b| a.name == b.name);
    with_external.push(&tb);
    for l in with_external {
        let own_process = l.manifest.get("specifications").and_then(|s| s.get("process")).and_then(Node::as_str);
        for (i, e) in l.manifest.get("external").and_then(Node::as_seq).unwrap_or(&[]).iter().enumerate() {
            let at = format!("{}/external/{i}", l.name);
            let Some(n) = e.get("name").and_then(Node::as_str) else { continue };
            let process = e.get("process").and_then(Node::as_str).or(own_process);
            let hit =
                map.external.iter().find(|x| x.name == n && (x.process.is_none() || x.process.as_deref() == process));
            let Some(hit) = hit else {
                p("E_LOCAL_FILES", &at, format!("the external file {n} of {} is not in the local file map", l.name));
                continue;
            };
            match std::fs::read(&hit.path) {
                Ok(b) => {
                    let j = manifest::to_json(e);
                    check_file(&mut p, &at, &hit.path, &b, j.get("match"), j.get("sha256"));
                }
                Err(err) => p("E_LOCAL_FILES", &at, format!("{}: {err}", hit.path.display())),
            }
            externals.push(UsedExternal { object: l.name.clone(), name: n.to_string(), path: hit.path.clone() });
        }
    }

    // 4. the names of subcircuits
    let key = |s: &str| s.to_ascii_lowercase();
    let mut used: BTreeMap<String, String> = BTreeMap::new(); // name -> who holds it
    for d in &tb_defs {
        used.insert(key(d), format!("the testbench {}", tb.name));
    }
    let mut maps: BTreeMap<String, BTreeMap<String, String>> = BTreeMap::new();
    let mut renamed: Vec<(String, String, String)> = Vec::new();
    let mut called_as: BTreeMap<String, String> = BTreeMap::new();
    for r in &rows {
        match called_as.get(&r.object) {
            Some(c) if key(c) != key(&r.called) => {
                p(
                    "E_ASSEMBLE_SUBCKT",
                    &r.object,
                    format!(
                        "{} is instantiated as {c} and as {}: one subcircuit cannot take both names",
                        r.object, r.called
                    ),
                );
            }
            Some(_) => {}
            None => {
                if let Some(holder) = used.get(&key(&r.called)) {
                    p(
                        "E_ASSEMBLE_SUBCKT",
                        &r.object,
                        format!("{} takes the name {}, which {holder} defines already", r.object, r.called),
                    );
                }
                used.insert(key(&r.called), r.object.clone());
                called_as.insert(r.object.clone(), r.called.clone());
                let top = &tops[&r.object];
                if top != &r.called {
                    maps.entry(r.object.clone()).or_default().insert(top.clone(), r.called.clone());
                    renamed.push((r.object.clone(), top.clone(), r.called.clone()));
                }
            }
        }
    }
    let mut done: BTreeSet<&str> = BTreeSet::new();
    for r in &rows {
        if !done.insert(&r.object) {
            continue;
        }
        let top = tops[&r.object].clone();
        for d in defs[&r.object].clone() {
            if d == top {
                continue;
            }
            if used.contains_key(&key(&d)) || maps.get(&r.object).is_some_and(|m| m.values().any(|v| key(v) == key(&d)))
            {
                let mut n = 2;
                let new = loop {
                    let c = format!("{d}_{n}");
                    if !used.contains_key(&key(&c)) {
                        break c;
                    }
                    n += 1;
                };
                used.insert(key(&new), r.object.clone());
                maps.entry(r.object.clone()).or_default().insert(d.clone(), new.clone());
                renamed.push((r.object.clone(), d, new));
            } else {
                used.insert(key(&d), r.object.clone());
            }
        }
    }
    if has_errors(&problems) {
        return Err(AssembleError::Problems(problems));
    }

    // 6. the files of the run
    let first_tb = tb_netlists.first().cloned().unwrap_or_default();
    let mut written: Vec<(String, Vec<u8>)> = Vec::new();
    for (path, b) in &tb.files {
        let mut b = if tb_netlists.contains(path) && !input.probes.is_empty() {
            let l = Lang::of(path);
            reading::place_probes(&String::from_utf8_lossy(b), l, &socket_instances, &input.probes).into_bytes()
        } else {
            b.clone()
        };
        if let (Some(mc), true) = (&input.monte_carlo, *path == first_tb) {
            if lang != Lang::Spectre {
                return Err(stop(
                    "E_ASSEMBLE_MC",
                    path.clone(),
                    "0.1 defines no way of running a Monte Carlo simulation in SPICE (7.6)",
                ));
            }
            // the instances of the socket in this netlist
            let original = String::from_utf8_lossy(tb.bytes(path).unwrap_or(&[])).to_string();
            let dut: Vec<String> = reading::read(&original, Lang::Spectre)
                .instances
                .iter()
                .filter(|i| i.parent.is_none() && i.master == socket)
                .map(|i| i.name.clone())
                .collect();
            match reading::wrap_analyses(&String::from_utf8_lossy(&b), &mc.statement(&dut)) {
                Ok(t) => b = t.into_bytes(),
                Err(m) => return Err(stop("E_ASSEMBLE_MC", path.clone(), format!("{path}: {m}"))),
            }
        }
        written.push((format!("tb/{path}"), b));
    }
    let mut netlists: Vec<String> = Vec::new();
    let mut seen: BTreeSet<&str> = BTreeSet::new();
    for r in &rows {
        if !seen.insert(&r.object) {
            continue;
        }
        let l = &objects[&r.object];
        let map = maps.get(&r.object).cloned().unwrap_or_default();
        let nl = l.role("netlist");
        for (path, b) in &l.files {
            let b = if nl.contains(path) && !map.is_empty() {
                reading::rename(&String::from_utf8_lossy(b), Lang::of(path), &map).into_bytes()
            } else {
                b.clone()
            };
            written.push((format!("ckt/{}/{path}", r.object), b));
        }
        netlists.extend(nl.iter().map(|n| format!("ckt/{}/{n}", r.object)));
    }
    let entry_name = if lang == Lang::Spectre { "input.scs" } else { "input.sp" };
    let entry = Entry {
        lang: Some(lang),
        title: format!("{} on {dut_name}", tb.name),
        models: models.iter().map(|m| (m.path.to_string_lossy().replace('\\', "/"), m.sections.clone())).collect(),
        netlists,
        testbench: format!("tb/{}", tb_netlists.first().cloned().unwrap_or_default()),
        parameters,
        options,
    };
    written.push((entry_name.to_string(), entry_text(&entry, CONDITIONS_AT).into_bytes()));

    // lock.json
    let files_of = |l: &Loaded, extra: bool| -> Value {
        let mut ps = l.listed();
        if extra {
            for k in ["entry", "requirements"] {
                if let Some(x) = l.manifest.get("analyze").and_then(|a| a.get(k)).and_then(Node::as_str) {
                    if !ps.iter().any(|q| q == x) {
                        ps.push(x.to_string());
                    }
                }
            }
        }
        let mut m = serde_json::Map::new();
        for q in ps {
            m.insert(q.clone(), Value::String(sha256(l.bytes(&q).unwrap_or(&[]))));
        }
        Value::Object(m)
    };
    let lock = json!({
        "toa": "lock/1",
        "dut": dut_name,
        "tbs": [{"object": tb.name, "files": files_of(&tb, true)}],
        "tree": rows.iter().map(|r| json!({"path": r.path, "object": r.object, "ifc": r.ifc,
            "files": files_of(&objects[&r.object], false)})).collect::<Vec<_>>(),
    });
    written
        .push((tree::LOCK.to_string(), (serde_json::to_string_pretty(&lock).unwrap_or_default() + "\n").into_bytes()));

    let out = Assembled {
        dir: input.out.clone(),
        entry: entry_name.into(),
        lang,
        tb: tb.clone(),
        tree: rows.iter().map(|r| (r.path.clone(), r.object.clone(), r.ifc.clone())).collect(),
        lock,
        models,
        externals,
        renamed,
        probes: input.probes.clone(),
        conditions: input.conditions.clone(),
        objects: objects.iter().map(|(n, l)| (n.clone(), l.dir.clone())).collect(),
    };
    if input.dry_run {
        return Ok(out);
    }
    write_dir(&input.out, &written)?;
    Ok(out)
}

/// `match` and `sha256` of a model or external file against the file at hand.
fn check_file(
    p: &mut dyn FnMut(&'static str, &str, String),
    at: &str,
    path: &Path,
    bytes: &[u8],
    matches: Option<&Value>,
    sum: Option<&Value>,
) {
    let text = String::from_utf8_lossy(bytes);
    for s in matches.and_then(Value::as_array).unwrap_or(&Vec::new()).iter().filter_map(Value::as_str) {
        if !text.contains(s) {
            p("E_LOCAL_FILES", at, format!("{} does not contain {s:?} (match)", path.display()));
        }
    }
    if let Some(want) = sum.and_then(Value::as_str) {
        if sha256(bytes) != want {
            p("E_LOCAL_FILES", at, format!("the SHA-256 of {} is not {want}", path.display()));
        }
    }
}

/// Write the files of a run into a temporary directory beside `dir`, then rename it into place.
fn write_dir(dir: &Path, files: &[(String, Vec<u8>)]) -> Result<(), AssembleError> {
    if dir.exists() && std::fs::read_dir(dir).map(|mut r| r.next().is_some()).unwrap_or(true) {
        return Err(stop(
            "E_ASSEMBLE_EXISTS",
            dir.display().to_string(),
            format!("{} is there already and not empty", dir.display()),
        ));
    }
    let parent =
        dir.parent().filter(|p| !p.as_os_str().is_empty()).map(Path::to_path_buf).unwrap_or_else(|| PathBuf::from("."));
    let leaf = dir.file_name().and_then(|n| n.to_str()).unwrap_or("run");
    let tmp = parent.join(format!(".toa-assemble-{}-{leaf}", std::process::id()));
    let write = || -> std::io::Result<()> {
        std::fs::create_dir_all(&tmp)?;
        for (rel, b) in files {
            let p = tmp.join(rel);
            if let Some(d) = p.parent() {
                std::fs::create_dir_all(d)?;
            }
            std::fs::write(&p, b)?;
        }
        if dir.exists() {
            std::fs::remove_dir(dir)?;
        }
        std::fs::rename(&tmp, dir)
    };
    write().map_err(|e| {
        let _ = std::fs::remove_dir_all(&tmp);
        AssembleError::Io(format!("writing {}: {e}", dir.display()))
    })
}

/// The sets of conditions of a preset: one set, or a list of sets.
pub fn preset_sets(tb: &Node, preset: &str) -> Option<Vec<Value>> {
    let p = tb.get("presets")?.get(preset)?;
    Some(match p.as_seq() {
        Some(items) => items.iter().map(manifest::to_json).collect(),
        None => vec![manifest::to_json(p)],
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_entry_file_in_both_languages() {
        let mut e = Entry {
            lang: Some(Lang::Spectre),
            title: "tb_offset on ckt_x".into(),
            models: vec![
                ("/pdk/models.scs".into(), vec!["tt".into(), "res_tt".into()]),
                ("/pdk/design.scs".into(), vec![]),
            ],
            netlists: vec!["ckt/ckt_a/a.scs".into(), "ckt/ckt_b/b.sp".into()],
            testbench: "tb/tb.scs".into(),
            parameters: vec![("vdd".into(), "1.8".into()), ("vin".into(), "0.0001".into())],
            options: vec![("temp".into(), "27".into()), ("reltol".into(), "0.001".into())],
        };
        assert_eq!(
            entry_text(&e, ConditionsAt::AfterTestbench),
            "simulator lang=spectre\ninclude \"/pdk/models.scs\" section=tt\ninclude \"/pdk/models.scs\" section=res_tt\n\
             include \"/pdk/design.scs\"\ninclude \"ckt/ckt_a/a.scs\"\ninclude \"ckt/ckt_b/b.sp\"\ninclude \"tb/tb.scs\"\n\
             parameters vdd=1.8 vin=0.0001\ntoaConditions options temp=27 reltol=0.001\n"
        );
        // the other place, should Spectre need it
        assert!(entry_text(&e, ConditionsAt::BeforeNetlists)
            .contains("res_tt\ninclude \"/pdk/design.scs\"\nparameters vdd=1.8"));
        e.lang = Some(Lang::Spice);
        e.netlists = vec!["ckt/ckt_a/a.sp".into()];
        e.testbench = "tb/tb.sp".into();
        assert_eq!(
            entry_text(&e, ConditionsAt::AfterTestbench),
            "* tb_offset on ckt_x\n.lib \"/pdk/models.scs\" tt\n.lib \"/pdk/models.scs\" res_tt\n.include \"/pdk/design.scs\"\n\
             .include \"ckt/ckt_a/a.sp\"\n.include \"tb/tb.sp\"\n.param vdd=1.8 vin=0.0001\n.temp 27\n.options reltol=0.001\n.end\n"
        );
    }

    #[test]
    fn values() {
        assert_eq!(value_text(&json!(27)).as_deref(), Some("27"));
        assert_eq!(value_text(&json!(1e-12)).as_deref(), Some("0.000000000001"));
        assert_eq!(value_text(&json!(true)).as_deref(), Some("1"));
        assert_eq!(value_text(&json!("conservative")).as_deref(), Some("conservative"));
        assert_eq!(value_text(&json!([1, 2])), None);
    }
}
