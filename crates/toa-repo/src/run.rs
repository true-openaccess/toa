//! `toa run`: an assembled run simulated, its results analysed, and an evidence record drafted
//! (7.6; `tb`; `evd`; chapter 11).
//!
//! - **The simulator** is called with the command of the testbench's `run`, which is only a
//!   reference: the tool writes the entry file in place of the netlist it names, and makes the
//!   simulator write its results in the format of `analyze.results` into `results/` (Spectre:
//!   `-format psfascii -raw results`, or `-format nutascii -raw results/results.raw`; ngspice:
//!   `-b -r results/results.raw` with ASCII raw files). The program must be a simulator this tool
//!   knows; otherwise the user gives the whole command. No shell is used.
//! - **`conditions.json`** is written into `results/` before the analysis: the conditions of the
//!   record, with its probes when it has any.
//! - **The analysis script is someone else's code** (chapter 11): it runs only with the user's
//!   agreement to that version (by the SHA-256 of its file), given once and remembered in the
//!   cache, after the tool has shown it and its difference from the version agreed to before for
//!   a testbench of the same name. It runs as `<python> <entry> <results> <output>` in a process
//!   and a directory of its own (`analysis/`). Nothing it needs is installed.
//! - **The draft** of the evidence record holds the conditions, the probes, the environment, the
//!   metrics of `metrics.json` by full name, the files of `evidence/` and the `lock.json` of the
//!   assembly. It is checked as `evd` asks before it is written; sealing it is left to the user.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde_json::{json, Value};
use toa_core::hash;
use toa_core::manifest::{self, Node};
use toa_types::mc::Stats;
use toa_types::reading::Lang;

use crate::assemble::Assembled;

/// The simulators whose command this tool knows how to point at the entry file and results.
pub const SIMULATORS: &[&str] = &["spectre", "ngspice"];

/// Split a command line into words: white space separates, double or single quotes hold
/// white space. No shell is involved.
pub fn words(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut quote: Option<char> = None;
    let mut any = false;
    for c in s.chars() {
        match (quote, c) {
            (Some(q), c) if c == q => quote = None,
            (Some(_), c) => cur.push(c),
            (None, '"' | '\'') => {
                quote = Some(c);
                any = true;
            }
            (None, c) if c.is_whitespace() => {
                if !cur.is_empty() || any {
                    out.push(std::mem::take(&mut cur));
                }
                any = false;
            }
            (None, c) => cur.push(c),
        }
    }
    if !cur.is_empty() || any {
        out.push(cur);
    }
    out
}

/// The name of a program without directory and `.exe`, in lower case.
pub fn program(w: &str) -> String {
    let leaf = w.rsplit(['/', '\\']).next().unwrap_or(w);
    leaf.strip_suffix(".exe").unwrap_or(leaf).to_ascii_lowercase()
}

/// A command: its words, and the environment to add.
pub type Command = (Vec<String>, Vec<(String, String)>);

/// The command that runs the simulator in the directory of the run, from the testbench's `run`:
/// its words, with the entry file in place of the netlist it names and the results written in
/// `format` into `results/`; and the environment to add. `Err` when the program is not a
/// simulator this tool knows, or cannot write that format.
pub fn simulator_command(run: &str, entry: &str, format: &str) -> Result<Command, String> {
    let w = words(run);
    let Some(first) = w.first() else { return Err("run is empty".into()) };
    let prog = program(first);
    match prog.as_str() {
        "spectre" => {
            let mut out = vec![first.clone()];
            let mut i = 1;
            let mut input_done = false;
            while i < w.len() {
                let a = &w[i];
                if a == "-format" || a == "-raw" || a == "=log" || a == "+log" || a == "-log" || a == "-outdir" {
                    i += 2;
                    continue;
                }
                if a.starts_with("-format=") || a.starts_with("-raw=") {
                    i += 1;
                    continue;
                }
                let low = a.to_ascii_lowercase();
                if !a.starts_with(['-', '+', '=']) && (low.ends_with(".scs") || low.ends_with(".sp") || low.ends_with(".net")) {
                    if !input_done {
                        out.push(entry.to_string());
                        input_done = true;
                    }
                    i += 1;
                    continue;
                }
                out.push(a.clone());
                i += 1;
            }
            if !input_done {
                out.insert(1, entry.to_string());
            }
            match format {
                "psfascii" => out.extend(["-format", "psfascii", "-raw", "results"].map(String::from)),
                "nutascii" => out.extend(["-format", "nutascii", "-raw", "results/results.raw"].map(String::from)),
                f => return Err(format!("this tool cannot make Spectre write {f}")),
            }
            Ok((out, Vec::new()))
        }
        "ngspice" => {
            if format != "nutascii" {
                return Err(format!("ngspice writes nutascii, not {format}"));
            }
            let out = vec![first.clone(), "-b".into(), "-r".into(), "results/results.raw".into(), entry.to_string()];
            Ok((out, vec![("SPICE_ASCIIRAWFILE".into(), "1".into())]))
        }
        other => Err(format!(
            "the command of run starts with {other}, not a simulator this tool knows ({}); give the whole command with --command",
            SIMULATORS.join(", ")
        )),
    }
}

/// The version of the simulator, as its log says it.
pub fn simulator_version(log: &str) -> Option<String> {
    for l in log.lines().take(200) {
        let t = l.trim();
        // Spectre: "Version 21.1.0.509.isr12 64bit -- 25 Jan 2024", or "spectre (ver. 21.1…)"
        if let Some(rest) = t.strip_prefix("Version ") {
            if let Some(v) = rest.split_whitespace().next() {
                return Some(v.to_string());
            }
        }
        if let Some(i) = t.find("(ver. ") {
            if let Some(v) = t[i + 6..].split([' ', ')']).next() {
                return Some(v.to_string());
            }
        }
        // ngspice: "******  ngspice-42 : Circuit level simulation program"
        if let Some(i) = t.find("ngspice-") {
            if let Some(v) = t[i + 8..].split_whitespace().next() {
                return Some(v.to_string());
            }
        }
    }
    None
}

/// `conditions.json`: the conditions of the record, with its probes when it has any (`tb`).
pub fn conditions_json(conditions: &Value, probes: &BTreeMap<String, String>) -> Value {
    let mut v = conditions.clone();
    if !v.is_object() {
        v = json!({});
    }
    if !probes.is_empty() {
        v["probes"] = json!(probes);
    }
    v
}

// ------------------------------------------------------------------ agreeing to an analysis script

/// What the user agreed to run, kept in the cache: the SHA-256 of a script's file, with the
/// testbench name and a copy of the script, so the next version can be shown as a difference.
#[derive(Debug, Default)]
pub struct Agreements {
    dir: PathBuf,
    list: Vec<Value>,
}

impl Agreements {
    /// Read them from `<cache>/analysis/`.
    pub fn open(cache: &Path) -> Agreements {
        let dir = cache.join("analysis");
        let list = std::fs::read(dir.join("agreed.json"))
            .ok()
            .and_then(|b| serde_json::from_slice::<Value>(&b).ok())
            .and_then(|v| v.get("agreed").and_then(Value::as_array).cloned())
            .unwrap_or_default();
        Agreements { dir, list }
    }

    pub fn agreed(&self, sha256: &str) -> bool {
        self.list.iter().any(|a| a.get("sha256").and_then(Value::as_str) == Some(sha256))
    }

    /// The script agreed to last for a testbench of this name, if any: (its SHA-256, its text).
    pub fn previous(&self, tb_name: &str) -> Option<(String, String)> {
        let a = self.list.iter().rev().find(|a| a.get("tb").and_then(Value::as_str) == Some(tb_name))?;
        let h = a.get("sha256")?.as_str()?.to_string();
        let text = std::fs::read_to_string(self.dir.join(&h)).ok()?;
        Some((h, text))
    }

    /// Remember an agreement.
    pub fn agree(&mut self, sha256: &str, tb_name: &str, script: &[u8]) -> std::io::Result<()> {
        std::fs::create_dir_all(&self.dir)?;
        std::fs::write(self.dir.join(sha256), script)?;
        if !self.agreed(sha256) {
            self.list.push(json!({"sha256": sha256, "tb": tb_name}));
        }
        let text = serde_json::to_string_pretty(&json!({"agreed": self.list})).unwrap_or_default() + "\n";
        std::fs::write(self.dir.join("agreed.json"), text)
    }
}

/// A difference of two texts by lines, as `diff -u` shows it without hunk headers: lines only
/// in `a` start with `-`, only in `b` with `+`, and up to 2 lines of context around them.
pub fn diff(a: &str, b: &str) -> String {
    let x: Vec<&str> = a.lines().collect();
    let y: Vec<&str> = b.lines().collect();
    // the longest common subsequence, by a table of lengths
    let (n, m) = (x.len(), y.len());
    let mut t = vec![vec![0u32; m + 1]; n + 1];
    for i in (0..n).rev() {
        for j in (0..m).rev() {
            t[i][j] = if x[i] == y[j] { t[i + 1][j + 1] + 1 } else { t[i + 1][j].max(t[i][j + 1]) };
        }
    }
    let mut ops: Vec<(char, &str)> = Vec::new();
    let (mut i, mut j) = (0, 0);
    while i < n || j < m {
        if i < n && j < m && x[i] == y[j] {
            ops.push((' ', x[i]));
            i += 1;
            j += 1;
        } else if i < n && (j == m || t[i + 1][j] >= t[i][j + 1]) {
            ops.push(('-', x[i]));
            i += 1;
        } else {
            ops.push(('+', y[j]));
            j += 1;
        }
    }
    let near = |k: usize| ops[k.saturating_sub(2)..(k + 3).min(ops.len())].iter().any(|o| o.0 != ' ');
    let mut out = String::new();
    let mut skipped = false;
    for (k, (c, l)) in ops.iter().enumerate() {
        if *c != ' ' || near(k) {
            if skipped {
                out.push_str("…\n");
                skipped = false;
            }
            out.push(*c);
            out.push_str(l);
            out.push('\n');
        } else {
            skipped = true;
        }
    }
    if !ops.iter().any(|o| o.0 != ' ') {
        return String::new();
    }
    if skipped {
        out.push_str("…\n");
    }
    out
}

pub fn sha256(b: &[u8]) -> String {
    hash::hex(&hash::digest(hash::Algorithm::Sha256, b).unwrap_or_default())
}

// ------------------------------------------------------------------ the evidence record

/// What the evidence record is made of, besides the assembly.
#[derive(Debug, Clone, Default)]
pub struct Record {
    pub license: String,
    pub author: Option<String>,
    /// The short name of the record label, such as `nom` or `vcm-3`.
    pub short: String,
    /// `YYYYMMDDTHHMMZ`, the UTC time of its creation.
    pub stamp: String,
    pub simulator: String,
    pub simulator_version: Option<String>,
    /// The words of the command that ran it, relative paths only.
    pub command: Vec<String>,
    pub python: Option<String>,
    pub system: Option<System>,
    pub metrics: BTreeMap<String, f64>,
    /// Metrics that were not obtained, and why.
    pub missing: Vec<(String, String)>,
    /// Files under `evidence/`: path, kind, label.
    pub evidence: Vec<(String, String, Option<String>)>,
    pub description: Option<String>,
    /// For a Monte Carlo simulation: a record of the kind `mc`.
    pub mc: Option<McRecord>,
}

/// What a record of the kind `mc` holds besides those of `sim`.
#[derive(Debug, Clone, Default)]
pub struct McRecord {
    /// `statistics` as run, with `run`.
    pub statistics: Value,
    /// The statistics of each metric, by its full name, in the order of `samples.csv`.
    pub stats: Vec<(String, Stats)>,
}

/// `statistics` as a record writes it: its fields in the order of `evd`, `first` only when
/// it is not 1, and `run`.
pub fn statistics_as_run(statistics: &Value, run: &str) -> Value {
    let mut m = serde_json::Map::new();
    for k in ["method", "points", "sampling", "seed", "first"] {
        if let Some(v) = statistics.get(k) {
            if k == "first" && v.as_i64() == Some(1) {
                continue;
            }
            m.insert(k.into(), v.clone());
        }
    }
    m.insert("run".into(), json!(run));
    Value::Object(m)
}

/// The statistics of a metric as `metrics` of a record of the kind `mc` writes them.
pub fn stats_json(s: &Stats) -> Value {
    let mut m = serde_json::Map::new();
    m.insert("n".into(), json!(s.n));
    m.insert("failed".into(), json!(s.failed));
    for (k, v) in [("mean", s.mean), ("sigma", s.sigma), ("min", s.min), ("max", s.max)] {
        if let Some(v) = v {
            m.insert(k.into(), json!(v));
        }
    }
    Value::Object(m)
}

/// The results of each sample of a Monte Carlo simulation in a directory of results (7.6): the
/// files Spectre names `toaMonteCarlo-<k>_<name>`, each with `<name>`. With `single`, a
/// simulation of that one sample, whose files may be numbered 1, or not at all.
pub fn split_samples(results: &Path, single: Option<i64>) -> BTreeMap<i64, Vec<(PathBuf, String)>> {
    let prefix = format!("{}-", crate::assemble::MC_STATEMENT);
    let mut found: BTreeMap<i64, Vec<(PathBuf, String)>> = BTreeMap::new();
    let mut plain: Vec<(PathBuf, String)> = Vec::new();
    let mut names: Vec<_> = std::fs::read_dir(results).map(|r| r.filter_map(Result::ok).collect()).unwrap_or_default();
    names.sort_by_key(|e: &std::fs::DirEntry| e.file_name());
    for e in names {
        if !e.file_type().is_ok_and(|t| t.is_file()) {
            continue;
        }
        let n = e.file_name().to_string_lossy().to_string();
        let numbered = n.strip_prefix(&prefix).and_then(|rest| {
            let (k, name) = rest.split_once('_')?;
            let k: i64 = k.parse().ok().filter(|_| !k.is_empty() && k.bytes().all(|b| b.is_ascii_digit()))?;
            (!name.is_empty()).then(|| (k, name.to_string()))
        });
        match numbered {
            Some((k, name)) => found.entry(k).or_default().push((e.path(), name)),
            None if !n.starts_with(crate::assemble::MC_STATEMENT) && n != "conditions.json" => {
                plain.push((e.path(), n))
            }
            None => {}
        }
    }
    if let Some(k) = single {
        if found.is_empty() && !plain.is_empty() {
            found.insert(k, plain);
        } else if found.len() == 1 && found.contains_key(&1) && k != 1 {
            let v = found.remove(&1).unwrap_or_default();
            found.insert(k, v);
        }
        found.retain(|x, _| *x == k);
    }
    found
}

/// `environment.system` of a record: what this tool can tell, each left out when it cannot.
#[derive(Debug, Clone, Default)]
pub struct System {
    pub os: Option<String>,
    pub cpu: Option<String>,
    pub threads: Option<usize>,
}

impl System {
    /// This machine: the system and architecture, the model of the processor where the system
    /// tells it without running a program (`/proc/cpuinfo` on Linux), and the threads available.
    pub fn here() -> System {
        let cpu = std::fs::read_to_string("/proc/cpuinfo").ok().and_then(|t| {
            t.lines()
                .find(|l| l.starts_with("model name"))
                .and_then(|l| l.split_once(':'))
                .map(|(_, v)| v.split_whitespace().collect::<Vec<_>>().join(" "))
                .filter(|v| !v.is_empty())
        });
        System {
            os: Some(format!("{} {}", std::env::consts::OS, std::env::consts::ARCH)),
            cpu,
            threads: std::thread::available_parallelism().ok().map(|n| n.get()),
        }
    }

    fn json(&self) -> Option<Value> {
        let mut m = serde_json::Map::new();
        if let Some(o) = self.os.as_ref().filter(|o| !o.is_empty()) {
            m.insert("os".into(), json!(o));
        }
        if let Some(c) = self.cpu.as_ref().filter(|c| !c.is_empty()) {
            m.insert("cpu".into(), json!(c));
        }
        if let Some(t) = self.threads.filter(|t| *t >= 1) {
            m.insert("threads".into(), json!(t));
        }
        (!m.is_empty()).then_some(Value::Object(m))
    }
}

/// A string as TOA YAML writes it: plain when it reads back as itself, else double-quoted.
pub fn yaml_str(s: &str) -> String {
    let plain_ok = !s.is_empty()
        && !s.starts_with([
            ' ', '-', '?', ':', ',', '[', ']', '{', '}', '#', '&', '*', '!', '|', '>', '\'', '"', '%', '@', '`',
        ])
        && !s.ends_with(' ')
        && !s.contains(": ")
        && !s.contains(" #")
        && !s.contains(['\n', '\t', ',', '[', ']', '{', '}'])
        && matches!(manifest::plain_value(s), Ok(manifest::Value::Str(ref v)) if v == s);
    if plain_ok {
        s.to_string()
    } else {
        serde_json::to_string(s).unwrap_or_default()
    }
}

/// A JSON value as flow YAML.
pub fn yaml_flow(v: &Value) -> String {
    match v {
        Value::Null => "null".into(),
        Value::Bool(b) => b.to_string(),
        Value::Number(n) => match n.as_f64() {
            Some(f) if n.is_f64() => {
                let s = format!("{f:?}");
                if s.contains('e') {
                    // TOA YAML floats: a dot before the exponent, a sign in it
                    let (m, e) = s.split_once('e').unwrap_or((&s, "0"));
                    let m = if m.contains('.') { m.to_string() } else { format!("{m}.0") };
                    let e = if e.starts_with('-') { e.to_string() } else { format!("+{e}") };
                    format!("{m}e{e}")
                } else {
                    s
                }
            }
            _ => n.to_string(),
        },
        Value::String(s) => yaml_str(s),
        Value::Array(a) => format!("[{}]", a.iter().map(yaml_flow).collect::<Vec<_>>().join(", ")),
        Value::Object(o) => {
            format!(
                "{{{}}}",
                o.iter().map(|(k, x)| format!("{}: {}", yaml_str(k), yaml_flow(x))).collect::<Vec<_>>().join(", ")
            )
        }
    }
}

/// The manifest of the record (`evd`, kind `sim`).
pub fn evd_manifest(a: &Assembled, r: &Record) -> String {
    let dut = a.tree.first().map(|t| t.1.clone()).unwrap_or_default();
    let tb_name = a.tb.manifest.get("name").and_then(Node::as_str).unwrap_or("tb");
    let mut y = String::new();
    y.push_str("type: evd\n");
    y.push_str(&format!("license: {}\n", yaml_str(&r.license)));
    y.push_str(if r.mc.is_some() { "kind: mc\n" } else { "kind: sim\n" });
    if let Some(au) = &r.author {
        y.push_str(&format!("author: {}\n", yaml_str(au)));
    }
    y.push_str(&format!("dut: {dut}\ntbs: [{}]\n", a.tb.name));
    if let Some(d) = &r.description {
        y.push_str(&format!("description: {}\n", yaml_str(d)));
    }
    if !a.probes.is_empty() {
        y.push_str(&format!("probes: {}\n", yaml_flow(&json!(a.probes))));
    }
    // the conditions as run; each model with the process it was matched under
    y.push_str("conditions:\n");
    let c = &a.conditions;
    for k in ["parameters", "options"] {
        if let Some(v) = c.get(k).filter(|v| v.as_object().is_some_and(|o| !o.is_empty())) {
            y.push_str(&format!("  {k}: {}\n", yaml_flow(v)));
        }
    }
    if !a.models.is_empty() {
        y.push_str("  models:\n");
        for m in &a.models {
            let mut item = m.item.clone();
            if item.get("process").is_none() {
                item["process"] = json!(m.process);
            }
            y.push_str(&format!("  - {}\n", yaml_flow(&item)));
        }
    }
    if let Some(mc) = &r.mc {
        y.push_str(&format!("  statistics: {}\n", yaml_flow(&mc.statistics)));
    }
    y.push_str("environment:\n");
    let mut sim = serde_json::Map::new();
    sim.insert("name".into(), json!(r.simulator));
    sim.insert("version".into(), json!(r.simulator_version.clone().unwrap_or_else(|| "unknown".into())));
    if !r.command.is_empty() {
        sim.insert("command".into(), json!(r.command.join(" ")));
    }
    y.push_str(&format!("  simulator: {}\n", yaml_flow(&Value::Object(sim))));
    let processes: std::collections::BTreeSet<(&str, Option<&str>)> =
        a.models.iter().map(|m| (m.process.as_str(), m.release.as_deref())).collect();
    if let [(p, rel)] = processes.into_iter().collect::<Vec<_>>().as_slice() {
        let mut pdk = serde_json::Map::new();
        pdk.insert("process".into(), json!(p));
        if let Some(rel) = rel {
            pdk.insert("release".into(), json!(rel));
        }
        y.push_str(&format!("  pdk: {}\n", yaml_flow(&Value::Object(pdk))));
    }
    if let Some(py) = &r.python {
        y.push_str(&format!("  analysis: {}\n", yaml_flow(&json!({"python": py}))));
    }
    if let Some(sys) = r.system.as_ref().and_then(System::json) {
        y.push_str(&format!("  system: {}\n", yaml_flow(&sys)));
    }
    if let Some(mc) = &r.mc {
        if mc.stats.is_empty() {
            y.push_str("metrics: {}\n");
        } else {
            y.push_str("metrics:\n");
            for (k, st) in &mc.stats {
                y.push_str(&format!("  {k}: {}\n", yaml_flow(&stats_json(st))));
            }
        }
    } else if r.metrics.is_empty() {
        y.push_str("metrics: {}\n");
    } else {
        y.push_str("metrics:\n");
        for (k, v) in &r.metrics {
            y.push_str(&format!("  {tb_name}.{k}: {}\n", yaml_flow(&json!(v))));
        }
    }
    if !r.evidence.is_empty() {
        y.push_str("evidence:\n");
        for (p, k, l) in &r.evidence {
            let mut e = serde_json::Map::new();
            e.insert("path".into(), json!(p));
            e.insert("kind".into(), json!(k));
            if let Some(l) = l {
                e.insert("label".into(), json!(l));
            }
            y.push_str(&format!("- {}\n", yaml_flow(&Value::Object(e))));
        }
    }
    y
}

/// The kinds of evidence files `evd` names.
pub const EVIDENCE_KINDS: &[&str] = &["spectrum", "curve", "waveform", "table", "json"];

/// The files of `<output>/evidence/`: from `evidence/index.json` when the script wrote one
/// (`{"evidence": [{path, kind, label}], "errors": {metric: why}}`, as the scripts toa-virtuoso
/// writes do), else every file, `.json` as `json` and others as `table`. Returns the files, the
/// errors the index gives, and notes on what was left out.
#[allow(clippy::type_complexity)]
pub fn evidence_of(output: &Path) -> (Vec<(String, String, Option<String>)>, Vec<(String, String)>, Vec<String>) {
    let dir = output.join("evidence");
    let mut files = Vec::new();
    let mut errors = Vec::new();
    let mut notes = Vec::new();
    if let Some(index) =
        std::fs::read(dir.join("index.json")).ok().and_then(|b| serde_json::from_slice::<Value>(&b).ok())
    {
        for e in index.get("evidence").and_then(Value::as_array).unwrap_or(&Vec::new()) {
            let (Some(p), Some(k)) = (e.get("path").and_then(Value::as_str), e.get("kind").and_then(Value::as_str))
            else {
                continue;
            };
            if !p.starts_with("evidence/") || p.contains("..") || !output.join(p).is_file() {
                notes.push(format!("evidence/index.json names {p}, not a file under evidence/: left out"));
                continue;
            }
            if !EVIDENCE_KINDS.contains(&k) {
                notes.push(format!("{p}: the kind {k} is not one evd names: left out"));
                continue;
            }
            files.push((p.to_string(), k.to_string(), e.get("label").and_then(Value::as_str).map(str::to_string)));
        }
        for (k, v) in index.get("errors").and_then(Value::as_object).cloned().unwrap_or_default() {
            errors.push((k, v.as_str().map(str::to_string).unwrap_or_else(|| v.to_string())));
        }
        return (files, errors, notes);
    }
    fn walk(d: &Path, rel: &str, out: &mut Vec<String>) {
        let Ok(rd) = std::fs::read_dir(d) else { return };
        let mut names: Vec<_> = rd.filter_map(Result::ok).collect();
        names.sort_by_key(|e| e.file_name());
        for e in names {
            let n = e.file_name().to_string_lossy().to_string();
            let r = format!("{rel}/{n}");
            if e.file_type().is_ok_and(|t| t.is_dir()) {
                walk(&e.path(), &r, out);
            } else {
                out.push(r);
            }
        }
    }
    let mut all = Vec::new();
    walk(&dir, "evidence", &mut all);
    for p in all {
        let k = if p.to_ascii_lowercase().ends_with(".json") { "json" } else { "table" };
        files.push((p, k.to_string(), None));
    }
    (files, errors, notes)
}

/// The language a run is in, as a word.
pub fn lang_word(l: Lang) -> &'static str {
    match l {
        Lang::Spectre => "spectre",
        Lang::Spice => "spice",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_simulator_command() {
        let (argv, env) =
            simulator_command("spectre input.scs +escchars +aps -format psfxl -raw psf", "input.scs", "psfascii")
                .unwrap();
        assert_eq!(argv.join(" "), "spectre input.scs +escchars +aps -format psfascii -raw results");
        assert!(env.is_empty());
        let (argv, _) = simulator_command("spectre tb.scs +aps", "input.scs", "nutascii").unwrap();
        assert_eq!(argv.join(" "), "spectre input.scs +aps -format nutascii -raw results/results.raw");
        let (argv, env) = simulator_command("ngspice -b tb.sp", "input.sp", "nutascii").unwrap();
        assert_eq!(argv.join(" "), "ngspice -b -r results/results.raw input.sp");
        assert_eq!(env, [("SPICE_ASCIIRAWFILE".to_string(), "1".to_string())]);
        assert!(simulator_command("rm -rf ~", "input.scs", "psfascii").is_err());
        assert!(simulator_command("ngspice x.sp", "input.sp", "psfascii").is_err());
        assert_eq!(words("a 'b c' \"d\" ''"), ["a", "b c", "d", ""]);
    }

    #[test]
    fn versions_from_logs() {
        let spectre =
            "\nCadence (R) Virtuoso (R) Spectre (R) Circuit Simulator\nVersion 21.1.0.509.isr12 64bit -- 25 Jan 2024\n";
        assert_eq!(simulator_version(spectre).as_deref(), Some("21.1.0.509.isr12"));
        assert_eq!(simulator_version("******  ngspice-42 : Circuit level simulation program\n").as_deref(), Some("42"));
        assert_eq!(simulator_version("nothing\n"), None);
    }

    #[test]
    fn differences() {
        let d = diff("a\nb\nc\nd\ne\nf\ng\n", "a\nb\nc\nD\ne\nf\ng\n");
        assert_eq!(d, "…\n b\n c\n-d\n+D\n e\n f\n…\n");
        assert_eq!(diff("x\n", "x\n"), "", "no difference");
    }

    #[test]
    fn yaml_values() {
        assert_eq!(yaml_str("CC-BY-4.0"), "CC-BY-4.0");
        assert_eq!(yaml_str("yes"), "\"yes\"");
        assert_eq!(yaml_str("1.8"), "\"1.8\"");
        assert_eq!(yaml_str("a: b"), "\"a: b\"");
        assert_eq!(
            yaml_flow(&json!({"vdd": 1.8, "k": 3, "t": 1e-12, "ok": true})),
            "{vdd: 1.8, k: 3, t: 1.0e-12, ok: true}"
        );
        let m =
            manifest::parse(format!("x: {}\n", yaml_flow(&json!({"a": "x y", "b": [1.5e20, -2]}))).as_bytes()).unwrap();
        assert_eq!(manifest::to_json(m.get("x").unwrap()), json!({"a": "x y", "b": [1.5e20, -2]}));
    }

    /// The numbers of metrics.json reach the record unchanged: read exactly (serde_json with
    /// float_roundtrip), written in the shortest form that reads back as the same number.
    #[test]
    fn numbers_keep_every_bit() {
        let mut texts: Vec<String> =
            ["4.767109420596355e-11", "0.1", "1e-300", "2.2250738585072014e-308", "-7.0e22", "123456789.12345679"]
                .iter()
                .map(|s| s.to_string())
                .collect();
        // and many others, by their bits
        let mut x: u64 = 0x9e37_79b9_7f4a_7c15;
        for _ in 0..20000 {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            let f = f64::from_bits(x);
            if f.is_finite() {
                texts.push(format!("{f:e}"));
                texts.push(format!("{f:?}"));
            }
        }
        for t in &texts {
            let want: f64 = t.parse().unwrap();
            let read: Value = serde_json::from_str(&format!("{{\"m\": {t}}}")).unwrap();
            let got = read["m"].as_f64().unwrap();
            assert_eq!(got.to_bits(), want.to_bits(), "{t} read as {got:?}");
            let y = yaml_flow(&read["m"]);
            let back = manifest::parse(
                format!(
                    "m: {y}
"
                )
                .as_bytes(),
            )
            .unwrap();
            let back = manifest::to_json(back.get("m").unwrap()).as_f64().unwrap();
            assert_eq!(back.to_bits(), want.to_bits(), "{t} written as {y}");
        }
    }

    #[test]
    fn results_split_by_sample() {
        let d = std::env::temp_dir().join(format!("toa-split-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        for n in [
            "toaMonteCarlo-001_tran.tran",
            "toaMonteCarlo-001_dc.dc",
            "toaMonteCarlo-12_tran.tran",
            "toaMonteCarlo.mc.log",
            "logFile",
        ] {
            std::fs::write(d.join(n), n).unwrap();
        }
        let all = split_samples(&d, None);
        let names = |k: i64, m: &BTreeMap<i64, Vec<(PathBuf, String)>>| -> Vec<String> {
            m.get(&k).map(|v| v.iter().map(|x| x.1.clone()).collect()).unwrap_or_default()
        };
        assert_eq!(all.keys().copied().collect::<Vec<_>>(), [1, 12]);
        assert_eq!(names(1, &all), ["dc.dc", "tran.tran"]);
        // one sample alone, numbered 1 by the simulator
        let d2 = d.join("one");
        std::fs::create_dir_all(&d2).unwrap();
        std::fs::write(d2.join("toaMonteCarlo-1_tran.tran"), "x").unwrap();
        assert_eq!(names(5, &split_samples(&d2, Some(5))), ["tran.tran"]);
        // or not numbered at all
        let d3 = d.join("plain");
        std::fs::create_dir_all(&d3).unwrap();
        std::fs::write(d3.join("tran.tran"), "x").unwrap();
        assert_eq!(names(5, &split_samples(&d3, Some(5))), ["tran.tran"]);
        let _ = std::fs::remove_dir_all(&d);
    }

    /// What cannot be told is left out of environment.system, never written empty.
    #[test]
    fn the_system_as_far_as_known() {
        let s = System { os: Some("linux x86_64".into()), cpu: Some(String::new()), threads: None };
        assert_eq!(s.json(), Some(json!({"os": "linux x86_64"})));
        assert_eq!(System::default().json(), None);
    }
}
