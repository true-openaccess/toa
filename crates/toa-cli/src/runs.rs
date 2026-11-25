//! `toa assemble` and `toa run` (7.6; `tb`, `evd`).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use serde_json::{json, Value};
use toa_core::manifest::Node;
use toa_repo::assemble::{self, AssembleError, Finder, Input, LocalFiles};
use toa_repo::fetch::{self, FetchError, Fetcher};

use super::{could_not_run, open_git, take_flag, take_hosts, take_value, usage_error, Out};

/// Finds objects as `toa fetch` does.
struct Fetch<'a, 'b>(&'b mut Fetcher<'a>);

impl Finder for Fetch<'_, '_> {
    fn find(&mut self, name: &str) -> Result<PathBuf, String> {
        self.0.fetch(name, None).map(|f| f.path).map_err(|e| e.to_string())
    }
}

/// The arguments `toa assemble` and `toa run` share.
pub(crate) struct Common {
    tb: String,
    dut: String,
    preset: Option<(String, Option<usize>)>,
    conditions: Option<PathBuf>,
    probes: BTreeMap<String, String>,
    map: Option<PathBuf>,
    out: Option<PathBuf>,
    dry_run: bool,
    workspace: Option<PathBuf>,
    offline: bool,
    hosts: Vec<String>,
    yes: bool,
}

impl Common {
    fn take(args: &mut Vec<String>, what: &str) -> Result<Common, String> {
        let preset = match take_value(args, "--preset")? {
            Some(p) => Some(match p.split_once(':') {
                Some((n, i)) => (
                    n.to_string(),
                    Some(
                        i.parse::<usize>()
                            .map_err(|_| format!("--preset {p}: NAME or NAME:INDEX, the index from 0"))?,
                    ),
                ),
                None => (p, None),
            }),
            None => None,
        };
        let conditions = take_value(args, "--conditions")?.map(PathBuf::from);
        let mut probes = BTreeMap::new();
        while let Some(p) = take_value(args, "--probe")? {
            let Some((k, v)) = p.split_once('=') else { return Err(format!("--probe {p}: PROBE=NET")) };
            probes.insert(k.to_string(), v.to_string());
        }
        let map = take_value(args, "--map")?.map(PathBuf::from);
        let out = take_value(args, "--out")?.map(PathBuf::from);
        let workspace = take_value(args, "--workspace")?.map(PathBuf::from);
        let hosts = take_hosts(args)?;
        let dry_run = take_flag(args, "--dry-run");
        let offline = take_flag(args, "--offline");
        let yes = take_flag(args, "--yes");
        if preset.is_some() == conditions.is_some() {
            return Err(format!(
                "{what} takes the conditions from --preset NAME[:INDEX] or from --conditions FILE, one of them"
            ));
        }
        let positional: Vec<&String> = args.iter().filter(|a| !a.starts_with('-')).collect();
        if positional.len() != 2 || args.len() != 2 {
            return Err(format!(
                "{what} takes a testbench and a circuit, each a name or a directory; unknown: {args:?}"
            ));
        }
        Ok(Common {
            tb: args[0].clone(),
            dut: args[1].clone(),
            preset,
            conditions,
            probes,
            map,
            out,
            dry_run,
            workspace,
            offline,
            hosts,
            yes,
        })
    }
}

/// What the shared part resolved: the objects, the sets of conditions to run and the map.
pub(crate) struct Resolved {
    pub tb: PathBuf,
    pub dut: PathBuf,
    pub tb_name: String,
    /// (label, set): the label names the set, such as `nom` or `supply-1`.
    pub sets: Vec<(String, Value)>,
    pub map: LocalFiles,
}

/// Find an object: a directory as it is, a name as `toa fetch` finds it. The library a
/// directory given is in (its repository, or here when it is in none) is looked in for the
/// circuits of the tree.
fn locate(f: &mut Fetcher, git: &dyn toa_repo::git::Git, what: &str) -> Result<PathBuf, ExitCode> {
    let dir = Path::new(what);
    if dir.is_dir() {
        let root = match git.toplevel(dir) {
            Ok(Some(top)) => top,
            _ => PathBuf::from("."),
        };
        if !f.local.contains(&root) {
            f.local.push(root);
        }
        return Ok(PathBuf::from(what));
    }
    if !toa_core::name::is_object_name(what) {
        return Err(usage_error(&format!("'{what}' is neither an object name nor a directory")));
    }
    match f.fetch(what, None) {
        Ok(x) => Ok(x.path),
        Err(FetchError::Local(m)) => Err(could_not_run(&m)),
        Err(e) => {
            eprintln!("toa: {what}: {e}");
            Err(ExitCode::from(4))
        }
    }
}

fn read_conditions(file: &Path) -> Result<Value, String> {
    let bytes = std::fs::read(file).map_err(|e| format!("{}: {e}", file.display()))?;
    if let Ok(v) = serde_json::from_slice::<Value>(&bytes) {
        return Ok(v);
    }
    toa_core::manifest::parse(&bytes)
        .map(|n| toa_core::manifest::to_json(&n))
        .map_err(|p| format!("{}: neither JSON nor TOA YAML: {}", file.display(), p[0]))
}

fn fetcher<'a>(git: &'a dyn toa_repo::git::Git, c: &Common) -> Result<Fetcher<'a>, ExitCode> {
    use std::io::IsTerminal;
    let Some(cache) = fetch::cache_dir() else { return Err(could_not_run(&"no cache directory: set TOA_CACHE")) };
    let mut f = Fetcher::new(git, cache);
    f.offline = c.offline;
    f.allowed.extend(c.hosts.iter().cloned());
    if let Ok(Some(top)) = git.toplevel(Path::new(".")) {
        f.local.push(top);
    }
    if let Some(w) = c.workspace.clone().or_else(|| fetch::find_workspace(Path::new("."))) {
        match fetch::read_workspace(&w) {
            Ok(w) => f.use_workspace(w),
            Err(e) => eprintln!("toa: the workspace list {} cannot be read: {e}", w.display()),
        }
    }
    if c.yes {
        f.ask = Some(Box::new(|_: &str| true));
    } else if std::io::stdin().is_terminal() && std::io::stderr().is_terminal() {
        f.ask = Some(Box::new(|host: &str| {
            eprint!("toa: connect to {host}? It is not in your own workspace list. [y/N] ");
            let mut line = String::new();
            std::io::stdin().read_line(&mut line).is_ok() && matches!(line.trim(), "y" | "Y" | "yes")
        }));
    }
    Ok(f)
}

/// Resolve the objects, the conditions and the local file map.
fn resolve(c: &Common, f: &mut Fetcher, git: &dyn toa_repo::git::Git) -> Result<Resolved, ExitCode> {
    let tb = locate(f, git, &c.tb)?;
    let dut = locate(f, git, &c.dut)?;
    let loaded = match assemble::load(&tb) {
        Ok(l) => l,
        Err(ps) => {
            for p in ps {
                eprintln!("{}: {p}", tb.display());
            }
            return Err(ExitCode::from(1));
        }
    };
    let tb_name = loaded.manifest.get("name").and_then(toa_core::manifest::Node::as_str).unwrap_or("tb").to_string();
    let sets: Vec<(String, Value)> = match (&c.preset, &c.conditions) {
        (Some((p, index)), _) => {
            let Some(all) = assemble::preset_sets(&loaded.manifest, p) else {
                return Err(usage_error(&format!("{} has no preset {p}", loaded.name)));
            };
            let many = all.len() > 1
                || loaded.manifest.get("presets").and_then(|x| x.get(p)).and_then(|x| x.as_seq()).is_some();
            match index {
                Some(i) => match all.get(*i) {
                    Some(s) => vec![(format!("{p}-{i}"), s.clone())],
                    None => return Err(usage_error(&format!("the preset {p} has {} sets, from 0", all.len()))),
                },
                None if many => all.into_iter().enumerate().map(|(i, s)| (format!("{p}-{i}"), s)).collect(),
                None => vec![(p.clone(), all[0].clone())],
            }
        }
        (None, Some(file)) => match read_conditions(file) {
            Ok(v) => vec![("conditions".into(), v)],
            Err(m) => return Err(usage_error(&m)),
        },
        (None, None) => unreachable!("checked when the arguments were taken"),
    };
    let map_file = c.map.clone().or_else(|| {
        let ws = c.workspace.clone().or_else(|| fetch::find_workspace(Path::new(".")))?;
        let f = ws.parent()?.join(assemble::LOCAL_FILES_NAME);
        f.is_file().then_some(f)
    });
    let map = match map_file {
        Some(m) => match assemble::read_local_files(&m) {
            Ok(m) => m,
            Err(e) => return Err(could_not_run(&e)),
        },
        None => LocalFiles::default(),
    };
    Ok(Resolved { tb, dut, tb_name, sets, map })
}

pub(crate) fn assemble_cmd(out: &mut Out, mut args: Vec<String>) -> ExitCode {
    let sample = match take_value(&mut args, "--sample") {
        Ok(None) => None,
        Ok(Some(k)) => match k.parse::<i64>() {
            Ok(k) if k >= 1 => Some(k),
            _ => return usage_error(&format!("--sample {k}: the number of a sample, 1 or more")),
        },
        Err(m) => return usage_error(&m),
    };
    let c = match Common::take(&mut args, "assemble") {
        Ok(c) => c,
        Err(m) => return usage_error(&m),
    };
    let git_box = match open_git() {
        Ok(g) => g,
        Err(code) => return code,
    };
    let mut f = match fetcher(git_box.as_ref(), &c) {
        Ok(f) => f,
        Err(code) => return code,
    };
    let r = match resolve(&c, &mut f, git_box.as_ref()) {
        Ok(r) => r,
        Err(code) => return code,
    };
    if r.sets.len() > 1 {
        return usage_error(&format!(
            "the preset has {} sets: assemble one with --preset NAME:INDEX (toa run runs them all)",
            r.sets.len()
        ));
    }
    let (label, set) = r.sets[0].clone();
    let dir = c.out.clone().unwrap_or_else(|| PathBuf::from(format!("{}-{label}", r.tb_name)));
    // a Monte Carlo simulation: all its samples in one simulation, or the one of --sample
    let monte_carlo = match (set.get("statistics"), sample) {
        (Some(st), k) => match assemble::Samples::of(st, k) {
            Ok(s) => Some(s),
            Err(m) => return usage_error(&m),
        },
        (None, Some(_)) => return usage_error("--sample is for a Monte Carlo simulation: conditions with statistics"),
        (None, None) => None,
    };
    let input = Input {
        tb: r.tb.clone(),
        dut: r.dut.clone(),
        conditions: set,
        probes: c.probes.clone(),
        map: r.map.clone(),
        out: dir,
        dry_run: c.dry_run,
        monte_carlo,
    };
    match assemble::assemble(&input, &mut Fetch(&mut f)) {
        Ok(a) => {
            report(out, &a, c.dry_run);
            out.finish()
        }
        Err(AssembleError::Problems(ps)) => {
            for p in &ps {
                out.problem("", p);
            }
            out.finish()
        }
        Err(AssembleError::Io(m)) => could_not_run(&m),
    }
}

/// Tell what was assembled.
pub(crate) fn report(out: &Out, a: &assemble::Assembled, dry_run: bool) {
    let dir = a.dir.display().to_string();
    let models: Vec<Value> = a
        .models
        .iter()
        .map(|m| json!({"file": m.item.get("file"), "process": m.process, "path": m.path.display().to_string(), "sections": m.sections}))
        .collect();
    let externals: Vec<Value> = a
        .externals
        .iter()
        .map(|e| json!({"object": e.object, "name": e.name, "path": e.path.display().to_string()}))
        .collect();
    let renamed: Vec<Value> = a.renamed.iter().map(|(o, f, t)| json!({"object": o, "from": f, "to": t})).collect();
    let mut text = format!(
        "{}{dir}: {} on {}, {} circuit(s), entry {}",
        if dry_run { "would assemble " } else { "assembled " },
        a.tb.name,
        a.tree.first().map(|t| t.1.as_str()).unwrap_or(""),
        a.tree.len(),
        a.entry
    );
    for (o, f, t) in &a.renamed {
        text.push_str(&format!("\n  renamed in {o}: subcircuit {f} -> {t}"));
    }
    for m in &a.models {
        text.push_str(&format!(
            "\n  model {} ({}): {}",
            m.item.get("file").and_then(Value::as_str).unwrap_or(""),
            m.process,
            m.path.display()
        ));
    }
    for e in &a.externals {
        text.push_str(&format!("\n  external {} of {}: {}", e.name, e.object, e.path.display()));
    }
    out.line(
        "assembled",
        json!({"dir": dir, "entry": a.entry, "tb": a.tb.name, "dut": a.tree.first().map(|t| t.1.clone()),
            "tree": a.tree.iter().map(|(p, o, i)| json!({"path": p, "object": o, "ifc": i})).collect::<Vec<_>>(),
            "models": models, "externals": externals, "renamed": renamed, "probes": a.probes, "dry_run": dry_run}),
        text,
    );
}

// ------------------------------------------------------------------ toa run

/// The objects of a run, for checking the draft of its evidence record.
struct Dirs(BTreeMap<String, PathBuf>);

impl toa_repo::types::Lookup for Dirs {
    fn manifest(&self, name: &str) -> Option<Node> {
        let b = std::fs::read(self.0.get(name)?.join("toa.yaml")).ok()?;
        toa_core::manifest::parse(&b).ok()
    }
    fn file_sha256(&self, name: &str, path: &str) -> Option<String> {
        std::fs::read(self.0.get(name)?.join(path)).ok().map(|b| toa_repo::run::sha256(&b))
    }
}

/// A word of a command as an evidence record may hold it: no local path (`evd`, environment).
fn public_word(w: &str) -> String {
    let p = Path::new(w);
    if p.is_absolute() || w.starts_with('~') {
        p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default()
    } else {
        w.to_string()
    }
}

/// The name of the simulator for the record: the program of the command when it is a simulator
/// this tool knows; else the one that `run` of the testbench names (a wrapper given with
/// `--command` still runs that simulator); else the program of the command.
fn simulator_name(argv: &[String], run_line: &str) -> String {
    use toa_repo::run::{program, words, SIMULATORS};
    let used = argv.first().map(|w| program(w)).unwrap_or_default();
    if SIMULATORS.contains(&used.as_str()) {
        return used;
    }
    match words(run_line).first().map(|w| program(w)) {
        Some(r) if SIMULATORS.contains(&r.as_str()) => r,
        _ => used,
    }
}

/// A short name for a record label: lower case, other characters as `-`.
fn short_of(label: &str) -> String {
    let s: String =
        label.to_ascii_lowercase().chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '-' }).collect();
    s.trim_matches('-').chars().take(40).collect()
}

/// The UTC time as `YYYYMMDDTHHMMZ`.
fn now_stamp() -> String {
    let secs =
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0);
    let days = secs.div_euclid(86400);
    let rem = secs.rem_euclid(86400);
    // civil from days (Howard Hinnant)
    let z = days + 719468;
    let era = z.div_euclid(146097);
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}{m:02}{d:02}T{:02}{:02}Z", rem / 3600, (rem % 3600) / 60)
}

/// The version of Python, as `python --version` gives it.
fn python_version(python: &str) -> Option<String> {
    let o = std::process::Command::new(python).arg("--version").output().ok()?;
    let t = String::from_utf8_lossy(&o.stdout).to_string() + &String::from_utf8_lossy(&o.stderr);
    t.split_whitespace().nth(1).map(str::to_string)
}

/// The Python this machine has: `python3`, else `python`.
fn default_python() -> String {
    for p in ["python3", "python"] {
        if python_version(p).is_some() {
            return p.into();
        }
    }
    "python3".into()
}

fn node_str<'n>(n: &'n Node, path: &[&str]) -> Option<&'n str> {
    let mut x = n;
    for k in path {
        x = x.get(k)?;
    }
    x.as_str()
}

/// Whether the user agrees to run the analysis script of the testbench, after it was shown
/// (chapter 11): agreed before (by its SHA-256), given with `--agree`, or asked now.
fn agree_to_script(
    agreements: &mut toa_repo::run::Agreements,
    tb: &assemble::Loaded,
    tb_name: &str,
    given: Option<&str>,
) -> Result<bool, String> {
    use std::io::IsTerminal;
    let entry = node_str(&tb.manifest, &["analyze", "entry"]).ok_or("the testbench has no analyze.entry")?;
    let bytes = std::fs::read(tb.dir.join(entry)).map_err(|e| format!("{entry}: {e}"))?;
    let sha = toa_repo::run::sha256(&bytes);
    if agreements.agreed(&sha) {
        return Ok(true);
    }
    if let Some(g) = given {
        if g.len() >= 12 && sha.starts_with(&g.to_ascii_lowercase()) {
            agreements.agree(&sha, tb_name, &bytes).map_err(|e| e.to_string())?;
            return Ok(true);
        }
        return Err(format!("--agree {g} is not the SHA-256 of {entry} of {} ({sha})", tb.name));
    }
    let shown = tb.dir.join(entry).display().to_string();
    let text = String::from_utf8_lossy(&bytes).to_string();
    if !(std::io::stdin().is_terminal() && std::io::stderr().is_terminal()) {
        return Err(format!(
            "the analysis script {shown} (SHA-256 {sha}) is code of the testbench author and has not been \
             agreed to; read it, then run again with --agree {} (or --no-analysis)",
            &sha[..16]
        ));
    }
    match agreements.previous(tb_name) {
        Some((h, old)) => {
            eprintln!(
                "toa: the analysis script of {} differs from the version you agreed to before ({}):",
                tb.name,
                &h[..12]
            );
            eprint!("{}", toa_repo::run::diff(&old, &text));
        }
        None => {
            eprintln!("toa: the analysis script of {} ({shown}), to be run for the first time:", tb.name);
            eprintln!("----------------------------------------------------------------------");
            eprint!("{text}");
            eprintln!("----------------------------------------------------------------------");
        }
    }
    eprintln!(
        "toa: it is code of the testbench author; toa runs it in a process and a directory of its own, \
         which is no isolation (chapter 11)"
    );
    eprint!("toa: run this version (SHA-256 {})? [y/N] ", &sha[..16]);
    let mut line = String::new();
    let yes = std::io::stdin().read_line(&mut line).is_ok() && matches!(line.trim(), "y" | "Y" | "yes");
    if yes {
        agreements.agree(&sha, tb_name, &bytes).map_err(|e| e.to_string())?;
    }
    Ok(yes)
}

/// What `toa run` takes besides what `toa assemble` takes.
struct RunArgs {
    command: Option<String>,
    /// How this machine starts the simulator (a sandbox, a job scheduler): never recorded.
    launcher: Option<String>,
    python: Option<String>,
    agree: Option<String>,
    evd_into: Option<PathBuf>,
    license: Option<String>,
    author: Option<String>,
    short: Option<String>,
    no_analysis: bool,
    /// How to run the samples of a Monte Carlo simulation when its preset leaves it open.
    mc_run: Option<String>,
}

impl RunArgs {
    fn take(args: &mut Vec<String>) -> Result<RunArgs, String> {
        Ok(RunArgs {
            command: take_value(args, "--command")?,
            launcher: take_value(args, "--launcher")?,
            python: take_value(args, "--python")?,
            agree: take_value(args, "--agree")?,
            evd_into: take_value(args, "--evd-into")?.map(PathBuf::from),
            license: take_value(args, "--license")?,
            author: take_value(args, "--author")?,
            short: take_value(args, "--short")?,
            no_analysis: take_flag(args, "--no-analysis"),
            mc_run: take_value(args, "--mc-run")?,
        })
    }
}

/// Run a program in a directory, its output into a log file.
fn run_logged(
    argv: &[String],
    dir: &Path,
    env: &[(String, String)],
    log: &Path,
) -> std::io::Result<std::process::ExitStatus> {
    let out = std::fs::File::create(log)?;
    let err = out.try_clone()?;
    std::process::Command::new(&argv[0])
        .args(&argv[1..])
        .current_dir(dir)
        .envs(env.iter().map(|(k, v)| (k.as_str(), v.as_str())))
        .stdin(std::process::Stdio::null())
        .stdout(out)
        .stderr(err)
        .status()
}

/// How a step of a set of conditions failed: the set fails, or the whole run stops.
enum Fail {
    Set(Vec<toa_core::Problem>),
    Stop(ExitCode),
}

/// What one analysis gave.
struct Analysis {
    /// By the `id` of the metric.
    metrics: BTreeMap<String, f64>,
    missing: Vec<(String, String)>,
    evidence: Vec<(String, String, Option<String>)>,
    notes: Vec<String>,
}

/// What every set of conditions of a run shares.
struct Ctx<'a> {
    tb: &'a assemble::Loaded,
    r: &'a Resolved,
    c: &'a Common,
    ra: &'a RunArgs,
    run_line: String,
    format: String,
    python: String,
    python_ver: Option<String>,
    stamp: String,
    /// The `id` of each metric of the testbench, in its order.
    ids: Vec<String>,
    many: bool,
}

impl Ctx<'_> {
    fn input(&self, set: &Value, dir: &Path, monte_carlo: Option<assemble::Samples>) -> Input {
        Input {
            tb: self.r.tb.clone(),
            dut: self.r.dut.clone(),
            conditions: set.clone(),
            probes: self.c.probes.clone(),
            map: self.r.map.clone(),
            out: dir.to_path_buf(),
            dry_run: self.c.dry_run,
            monte_carlo,
        }
    }

    fn assemble(&self, f: &mut Fetcher, input: &Input) -> Result<assemble::Assembled, Fail> {
        match assemble::assemble(input, &mut Fetch(f)) {
            Ok(a) => Ok(a),
            Err(AssembleError::Problems(ps)) => Err(Fail::Set(ps)),
            Err(AssembleError::Io(m)) => Err(Fail::Stop(could_not_run(&m))),
        }
    }

    /// The simulator's command for an assembled run: the user's, or the one of `run` pointed
    /// at the entry file and the results.
    fn command(&self, entry: &str) -> Result<toa_repo::run::Command, Fail> {
        let (argv, env) = match &self.ra.command {
            Some(cmd) => {
                let w: Vec<String> = toa_repo::run::words(cmd)
                    .into_iter()
                    .map(|x| x.replace("{input}", entry).replace("{results}", "results"))
                    .collect();
                (w, Vec::new())
            }
            None => toa_repo::run::simulator_command(&self.run_line, entry, &self.format)
                .map_err(|m| Fail::Stop(could_not_run(&m)))?,
        };
        if argv.is_empty() {
            return Err(Fail::Stop(usage_error("--command is empty")));
        }
        Ok((argv, env))
    }

    /// What runs on this machine for the simulator's command `argv`: `argv` itself, or `argv`
    /// inside the template of `--launcher`. Only `argv` goes into a record.
    fn launched(&self, argv: &[String]) -> Vec<String> {
        match &self.ra.launcher {
            Some(t) => launch(t, argv),
            None => argv.to_vec(),
        }
    }

    /// The `would_run` line of a dry run: the command, and what runs it here when a launcher is given.
    fn would_run(&self, mut line: Value, dir: &Path, argv: &[String], what: &str) -> (Value, String) {
        let mut text = format!("  would run {what}in {}: {}", dir.display(), argv.join(" "));
        if self.ra.launcher.is_some() {
            let l = self.launched(argv);
            text.push_str(&format!("\n  through the launcher, here: {}", l.join(" ")));
            line["launched"] = json!(l);
        }
        (line, text)
    }

    /// Run the simulator in an assembled directory: its log, or the problem when it failed.
    /// `argv` is the simulator's command as recorded; what runs is that command inside the
    /// launcher, when one is given.
    fn simulate(&self, argv: &[String], env: &[(String, String)], dir: &Path) -> Result<String, Fail> {
        let _ = std::fs::create_dir_all(dir.join("results"));
        let log_path = dir.join("simulator.log");
        let argv = &self.launched(argv);
        let status = run_logged(argv, dir, env, &log_path);
        let log = std::fs::read_to_string(&log_path).unwrap_or_default();
        match status {
            Ok(s) if s.success() => Ok(log),
            Ok(s) => Err(Fail::Set(vec![toa_core::Problem::error(
                "E_RUN_SIMULATOR",
                "7.6",
                log_path.display().to_string(),
                format!("the simulator exited with {s}; see the log"),
            )])),
            Err(e) => Err(Fail::Stop(could_not_run(&format!("{}: {e}", argv[0])))),
        }
    }

    /// Run the analysis script on `results` in a process and directory (`cwd`) of its own, its
    /// output into `output` and its log into `log`; what it gave.
    fn analyze(
        &self,
        script: &Path,
        results: &Path,
        output: &Path,
        cwd: &Path,
        log: &Path,
    ) -> Result<Analysis, toa_core::Problem> {
        let _ = std::fs::create_dir_all(cwd);
        let _ = std::fs::create_dir_all(output);
        let argv = vec![
            self.python.clone(),
            script.display().to_string(),
            results.display().to_string(),
            output.display().to_string(),
        ];
        let st = run_logged(&argv, cwd, &[], log);
        if !st.as_ref().is_ok_and(|s| s.success()) {
            let why = st.map(|s| s.to_string()).unwrap_or_else(|e| e.to_string());
            return Err(toa_core::Problem::error(
                "E_RUN_ANALYSIS",
                "types/tb",
                log.display().to_string(),
                format!("the analysis script failed ({why}); see the log"),
            ));
        }
        // read exactly as written (serde_json with float_roundtrip)
        let metrics_json: Value = std::fs::read(output.join("metrics.json"))
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or(Value::Null);
        let (evidence, mut missing, notes) = toa_repo::run::evidence_of(output);
        let mut metrics = BTreeMap::new();
        for id in &self.ids {
            match metrics_json.get(id).and_then(Value::as_f64).filter(|x| x.is_finite()) {
                Some(x) => {
                    metrics.insert(id.clone(), x);
                }
                None if missing.iter().any(|(k, _)| k == id) => {}
                None => missing.push((id.clone(), "not in metrics.json as a number".into())),
            }
        }
        missing.retain(|(k, _)| !metrics.contains_key(k));
        Ok(Analysis { metrics, missing, evidence, notes })
    }

    /// Where the conditions of a set came from, for the description of its record: the preset
    /// (and the index of the set in it), or the name of the file given with `--conditions`,
    /// without its directory (no local path goes into a record).
    fn conditions_said(&self, label: &str) -> String {
        conditions_said(self.c.conditions.as_deref(), label)
    }

    /// The short name of a record label for a set.
    fn short(&self, label: &str) -> String {
        match &self.ra.short {
            Some(s) if self.many => format!("{s}-{}", label.rsplit('-').next().unwrap_or("")),
            Some(s) => s.clone(),
            None => short_of(label),
        }
    }

    /// Where the draft of the record goes: `evd/<interface name>/<stamp>-<short>/evd_draft-<short>`.
    fn draft(&self, f: &mut Fetcher, into: &Path, short: &str) -> PathBuf {
        let ifc = node_str(&self.tb.manifest, &["ifc"]).unwrap_or("");
        let ifc_name = f
            .fetch(ifc, None)
            .ok()
            .and_then(|x| assemble::load(&x.path).ok())
            .and_then(|l| l.manifest.get("name").and_then(Node::as_str).map(str::to_string))
            .unwrap_or_else(|| ifc.chars().take(12).collect());
        into.join("evd").join(&ifc_name).join(format!("{}-{short}", self.stamp)).join(format!("evd_draft-{short}"))
    }

    /// A record, the parts the kinds share.
    fn record(&self, lic: &str, short: &str, argv: &[String], log: &str, description: String) -> toa_repo::run::Record {
        toa_repo::run::Record {
            license: lic.to_string(),
            author: self.ra.author.clone(),
            short: short.to_string(),
            stamp: self.stamp.clone(),
            simulator: simulator_name(argv, &self.run_line),
            simulator_version: toa_repo::run::simulator_version(log),
            command: argv.iter().map(|w| public_word(w)).collect(),
            python: self.python_ver.clone(),
            system: Some(toa_repo::run::System::here()),
            description: Some(description),
            ..Default::default()
        }
    }
}

pub(crate) fn run_cmd(out: &mut Out, mut args: Vec<String>) -> ExitCode {
    let ra = match RunArgs::take(&mut args) {
        Ok(r) => r,
        Err(m) => return usage_error(&m),
    };
    let c = match Common::take(&mut args, "run") {
        Ok(c) => c,
        Err(m) => return usage_error(&m),
    };
    if ra.evd_into.is_some() && ra.license.is_none() {
        return usage_error("--evd-into needs --license: the licence of the evidence records");
    }
    if ra.evd_into.is_some() && ra.no_analysis {
        return usage_error("an evidence record needs the metrics of the analysis: not with --no-analysis");
    }
    if let Some(l) = &ra.launcher {
        if toa_repo::run::words(l).is_empty() {
            return usage_error("--launcher is empty");
        }
    }
    if let Some(w) = ra.command.as_deref().and_then(command_not_a_simulator) {
        eprintln!("toa: warning: {w}");
    }
    if let Some(m) = &ra.mc_run {
        if !toa_repo::types::RUNS.contains(&m.as_str()) {
            return usage_error(&format!("--mc-run {m}: each or batch"));
        }
    }
    let git_box = match open_git() {
        Ok(g) => g,
        Err(code) => return code,
    };
    let mut f = match fetcher(git_box.as_ref(), &c) {
        Ok(f) => f,
        Err(code) => return code,
    };
    let r = match resolve(&c, &mut f, git_box.as_ref()) {
        Ok(r) => r,
        Err(code) => return code,
    };
    let tb = match assemble::load(&r.tb) {
        Ok(l) => l,
        Err(ps) => {
            for p in &ps {
                out.problem("", p);
            }
            return out.finish();
        }
    };
    let format = node_str(&tb.manifest, &["analyze", "results"]).unwrap_or("").to_string();
    if !toa_repo::types::RESULTS.contains(&format.as_str()) {
        return could_not_run(&format!(
            "{} names no format of results (analyze.results, B36): it cannot be run",
            tb.name
        ));
    }
    // the analysis script is agreed to before anything runs
    let Some(cache) = fetch::cache_dir() else { return could_not_run(&"no cache directory: set TOA_CACHE") };
    let mut agreements = toa_repo::run::Agreements::open(&cache);
    if !ra.no_analysis && !c.dry_run {
        match agree_to_script(&mut agreements, &tb, &r.tb_name, ra.agree.as_deref()) {
            Ok(true) => {}
            Ok(false) => {
                eprintln!("toa: not agreed; nothing was run");
                return ExitCode::from(1);
            }
            Err(m) => {
                eprintln!("toa: {m}");
                return ExitCode::from(1);
            }
        }
    }
    let python = ra.python.clone().unwrap_or_else(default_python);
    let ids: Vec<String> = tb
        .manifest
        .get("metrics")
        .and_then(Node::as_seq)
        .unwrap_or(&[])
        .iter()
        .filter_map(|m| m.get("id").and_then(Node::as_str).map(str::to_string))
        .collect();
    let x = Ctx {
        tb: &tb,
        r: &r,
        c: &c,
        ra: &ra,
        run_line: node_str(&tb.manifest, &["run"]).unwrap_or("").to_string(),
        format,
        python_ver: if ra.no_analysis { None } else { python_version(&python) },
        python,
        stamp: now_stamp(),
        ids,
        many: r.sets.len() > 1,
    };
    let base = c.out.clone().unwrap_or_else(|| {
        let p = c.preset.as_ref().map(|p| p.0.clone()).unwrap_or_else(|| "conditions".into());
        PathBuf::from(format!("{}-{p}", r.tb_name))
    });
    let mut failed = 0;
    for (label, set) in &r.sets {
        let dir = if x.many { base.join(label) } else { base.clone() };
        let done = match set.get("statistics") {
            Some(st) => run_mc(&x, &mut f, out, label, set, st, &dir),
            None => run_sim(&x, &mut f, out, label, set, &dir),
        };
        match done {
            Ok(()) => {}
            Err(Fail::Set(ps)) => {
                for p in &ps {
                    out.problem(label, p);
                }
                failed += 1;
            }
            Err(Fail::Stop(code)) => return code,
        }
    }
    if failed > 0 {
        eprintln!("toa: {failed} of {} set(s) failed", r.sets.len());
        return out.finish_as(1);
    }
    out.finish()
}

/// One set of conditions without `statistics`: a record of the kind `sim`.
fn run_sim(x: &Ctx, f: &mut Fetcher, out: &mut Out, label: &str, set: &Value, dir: &Path) -> Result<(), Fail> {
    let a = x.assemble(f, &x.input(set, dir, None))?;
    report(out, &a, x.c.dry_run);
    let (argv, env) = x.command(&a.entry)?;
    if x.c.dry_run {
        let (line, text) = x.would_run(json!({"dir": dir.display().to_string(), "command": argv}), dir, &argv, "");
        out.line("would_run", line, text);
        return Ok(());
    }
    let log = x.simulate(&argv, &env, dir)?;
    // conditions.json, then the analysis in a process and a directory of its own
    let cj = toa_repo::run::conditions_json(&a.conditions, &a.probes);
    let _ = std::fs::write(
        dir.join("results/conditions.json"),
        serde_json::to_string_pretty(&cj).unwrap_or_default() + "\n",
    );
    if x.ra.no_analysis {
        out.line(
            "ran",
            json!({"dir": dir.display().to_string(), "analysis": false}),
            format!("  simulated in {}", dir.display()),
        );
        return Ok(());
    }
    let entry = node_str(&x.tb.manifest, &["analyze", "entry"]).unwrap_or("");
    let abs = std::path::absolute(dir).unwrap_or(dir.to_path_buf());
    let output = abs.join("out");
    let an = x
        .analyze(
            &abs.join("tb").join(entry),
            &abs.join("results"),
            &output,
            &abs.join("analysis"),
            &abs.join("analysis.log"),
        )
        .map_err(|p| Fail::Set(vec![p]))?;
    for n in &an.notes {
        eprintln!("{label}: note: {n}");
    }
    let mut line = json!({"dir": dir.display().to_string(), "metrics": an.metrics, "missing": an.missing});
    let shown: Vec<String> = an.metrics.iter().map(|(k, v)| format!("{}.{k}={v}", x.r.tb_name)).collect();
    let mut text = format!("  ran in {}: {}", dir.display(), shown.join(", "));
    for (k, why) in &an.missing {
        text.push_str(&format!("\n  not obtained: {}.{k} ({why})", x.r.tb_name));
    }
    // the draft of the evidence record
    if let (Some(into), Some(lic)) = (&x.ra.evd_into, &x.ra.license) {
        let s = x.short(label);
        let draft = x.draft(f, into, &s);
        let mut desc = format!(
            "Made by toa run {}: {} on {}, {}.",
            env!("CARGO_PKG_VERSION"),
            x.r.tb_name,
            a.tree.first().map(|t| t.1.as_str()).unwrap_or(""),
            x.conditions_said(label)
        );
        if !an.missing.is_empty() {
            let m: Vec<String> = an.missing.iter().map(|(k, w)| format!("{k} ({w})")).collect();
            desc.push_str(&format!(" Metrics not obtained: {}.", m.join("; ")));
        }
        let mut rec = x.record(lic, &s, &argv, &log, desc);
        rec.metrics = an.metrics.clone();
        rec.missing = an.missing.clone();
        rec.evidence = an.evidence.clone();
        let problems = write_record(&a, &rec, &output, &draft).map_err(|m| Fail::Stop(could_not_run(&m)))?;
        for p in &problems {
            out.problem(&draft.display().to_string(), p);
        }
        line["evd"] = json!(draft.display().to_string());
        text.push_str(&format!("\n  evidence record draft: {}", draft.display()));
    }
    out.line("ran", line, text);
    Ok(())
}

/// The command that runs on this machine: the simulator's command `argv` inside the template of
/// `--launcher`. A word `{command}` stands for the words of `argv`, a word `{args}` for those
/// after its program; inside a longer word they stand for those words joined by spaces. A
/// template with neither is a prefix: `argv` follows it.
fn launch(template: &str, argv: &[String]) -> Vec<String> {
    let rest = argv.get(1..).unwrap_or(&[]);
    let words = toa_repo::run::words(template);
    let placed = words.iter().any(|w| w.contains("{command}") || w.contains("{args}"));
    let mut out = Vec::new();
    for w in words {
        match w.as_str() {
            "{command}" => out.extend(argv.iter().cloned()),
            "{args}" => out.extend(rest.iter().cloned()),
            _ => out.push(w.replace("{command}", &argv.join(" ")).replace("{args}", &rest.join(" "))),
        }
    }
    if !placed {
        out.extend(argv.iter().cloned());
    }
    out
}

/// The warning for a `--command` whose program is not a simulator this tool knows: the record
/// carries that command as the simulator's, and a wrapper of this machine belongs in `--launcher`.
fn command_not_a_simulator(command: &str) -> Option<String> {
    use toa_repo::run::{program, words, SIMULATORS};
    let p = program(words(command).first()?);
    if SIMULATORS.contains(&p.as_str()) {
        return None;
    }
    Some(format!(
        "--command runs {p}, not a simulator toa knows ({}); the evidence record will carry this command \
         as the simulator's command line. A wrapper of this machine (a sandbox, a job scheduler) belongs \
         in --launcher, which is never recorded",
        SIMULATORS.join(", ")
    ))
}

/// See [`Ctx::conditions_said`].
fn conditions_said(file: Option<&Path>, label: &str) -> String {
    match file {
        Some(f) => {
            let name = f.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
            format!("conditions from the file {name}")
        }
        None => format!("conditions {label}"),
    }
}

/// Move a file; a file already at `to` is never replaced. Where a rename cannot (another file
/// system), it is copied and then removed, so that it is never in neither place.
fn move_file(from: &Path, to: &Path) -> std::io::Result<()> {
    if std::fs::symlink_metadata(to).is_ok() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::AlreadyExists,
            format!("{} is there already", to.display()),
        ));
    }
    if std::fs::rename(from, to).is_ok() {
        return Ok(());
    }
    std::fs::copy(from, to)?;
    std::fs::remove_file(from)
}

fn samples_word(n: usize) -> String {
    if n == 1 {
        "1 sample".into()
    } else {
        format!("{n} samples")
    }
}

/// One set of conditions with `statistics`: a Monte Carlo simulation, a record of the kind `mc`
/// (7.6). With `run: batch` one simulation of all the samples, split by sample; with `each`, one
/// simulation for each. Each sample gets `samples/<k>/results` with `conditions.json`, and the
/// analysis script is called on it; then `evidence/samples.csv` and the statistics.
fn run_mc(
    x: &Ctx,
    f: &mut Fetcher,
    out: &mut Out,
    label: &str,
    set: &Value,
    st: &Value,
    dir: &Path,
) -> Result<(), Fail> {
    let stop = |m: String| Fail::Stop(usage_error(&m));
    let mode = match (st.get("run").and_then(Value::as_str), x.ra.mc_run.as_deref()) {
        (Some(p), Some(u)) if p != u => {
            return Err(stop(format!("{label}: the preset runs the samples {p}, and --mc-run says {u}")))
        }
        (Some(p), _) => p.to_string(),
        (None, Some(u)) => u.to_string(),
        (None, None) => {
            return Err(stop(format!(
                "{label} is a Monte Carlo simulation that leaves how to run it to you: --mc-run each (one \
                 simulation for each sample) or --mc-run batch (all in one)"
            )))
        }
    };
    if x.format != "psfascii" {
        return Err(Fail::Stop(could_not_run(&format!(
            "{label}: the results of a Monte Carlo simulation are split by sample from the files of psfascii; \
             this tool cannot split {}",
            x.format
        ))));
    }
    let all = assemble::Samples::of(st, None)
        .map_err(|m| Fail::Set(vec![toa_core::Problem::error("E_CONDITIONS", "7.6", "statistics", m)]))?;
    let wanted: Vec<i64> = (all.first..all.first + all.count).collect();
    let stats_run = toa_repo::run::statistics_as_run(st, &mode);
    // the simulations: (directory, the one sample it runs when each)
    let mut sims: Vec<(PathBuf, Option<i64>)> = Vec::new();
    let mut first_a: Option<assemble::Assembled> = None;
    if mode == "batch" {
        let a = x.assemble(f, &x.input(set, dir, Some(all.clone())))?;
        report(out, &a, x.c.dry_run);
        sims.push((dir.to_path_buf(), None));
        first_a = Some(a);
    } else {
        if dir.exists() && std::fs::read_dir(dir).map(|mut r| r.next().is_some()).unwrap_or(true) {
            return Err(Fail::Set(vec![toa_core::Problem::error(
                "E_ASSEMBLE_EXISTS",
                "7.6",
                dir.display().to_string(),
                "the directory of the run is not empty",
            )]));
        }
        for k in &wanted {
            let one = assemble::Samples::of(st, Some(*k))
                .map_err(|m| Fail::Set(vec![toa_core::Problem::error("E_CONDITIONS", "7.6", "statistics", m)]))?;
            let d = dir.join("runs").join(k.to_string());
            let a = x.assemble(f, &x.input(set, &d, Some(one)))?;
            if first_a.is_none() {
                report(out, &a, x.c.dry_run);
                first_a = Some(a);
            }
            sims.push((d, Some(*k)));
        }
    }
    let Some(a) = first_a else { return Ok(()) };
    let (argv, env) = x.command(&a.entry)?;
    if x.c.dry_run {
        let (line, text) = x.would_run(
            json!({"dir": dir.display().to_string(), "command": argv, "simulations": sims.len(), "samples": wanted.len(), "run": mode}),
            dir,
            &argv,
            &format!("{} simulation(s) of {} sample(s) ({mode}) ", sims.len(), wanted.len()),
        );
        out.line("would_run", line, text);
        return Ok(());
    }
    // simulate, and gather the results of each sample under samples/<k>/results
    let abs = std::path::absolute(dir).unwrap_or(dir.to_path_buf());
    let mut log = String::new();
    let mut have: BTreeMap<i64, PathBuf> = BTreeMap::new();
    let mut sim_failed: Vec<String> = Vec::new();
    for (d, single) in &sims {
        match x.simulate(&argv, &env, d) {
            Ok(l) => log = l,
            Err(Fail::Set(ps)) => {
                // its samples have no results, unless the simulator wrote some
                sim_failed.extend(ps.iter().map(|p| format!("{}: {}", p.path, p.message)));
                log = std::fs::read_to_string(d.join("simulator.log")).unwrap_or_default();
            }
            Err(e) => return Err(e),
        }
        for (k, files) in toa_repo::run::split_samples(&d.join("results"), *single) {
            if !wanted.contains(&k) {
                continue;
            }
            let to = abs.join("samples").join(k.to_string()).join("results");
            std::fs::create_dir_all(&to).map_err(|e| Fail::Stop(could_not_run(&format!("{}: {e}", to.display()))))?;
            // moved, not copied: the results of a large run are kept once (a file is either here or
            // still where the simulator wrote it, never lost)
            for (from, name) in files {
                move_file(&from, &to.join(&name))
                    .map_err(|e| Fail::Stop(could_not_run(&format!("{}: {e}", from.display()))))?;
            }
            // conditions.json: the conditions with statistics, the probes, and the sample
            let mut cond = a.conditions.clone();
            cond["statistics"] = stats_run.clone();
            let mut cj = toa_repo::run::conditions_json(&cond, &a.probes);
            cj["sample"] = json!(k);
            let _ = std::fs::write(
                to.join("conditions.json"),
                serde_json::to_string_pretty(&cj).unwrap_or_default() + "\n",
            );
            have.insert(k, to);
        }
    }
    if have.is_empty() {
        let mut ps = vec![toa_core::Problem::error(
            "E_RUN_SIMULATOR",
            "7.6",
            dir.display().to_string(),
            "no sample has results: see simulator.log",
        )];
        ps.extend(sim_failed.iter().map(|m| toa_core::Problem::warning("E_RUN_SIMULATOR", "7.6", "", m.clone())));
        return Err(Fail::Set(ps));
    }
    for m in &sim_failed {
        eprintln!("{label}: note: {m}");
    }
    if x.ra.no_analysis {
        out.line(
            "ran",
            json!({"dir": dir.display().to_string(), "analysis": false, "samples": have.len(), "points": wanted.len()}),
            format!("  simulated {} of {} sample(s) in {}", have.len(), wanted.len(), dir.display()),
        );
        return Ok(());
    }
    // the analysis script, once for each sample
    let entry = node_str(&x.tb.manifest, &["analyze", "entry"]).unwrap_or("");
    let script = std::path::absolute(&a.dir).unwrap_or(a.dir.clone()).join("tb").join(entry);
    let mut rows: Vec<(i64, Vec<Option<f64>>)> = Vec::new();
    let mut analysis_failed = 0;
    for k in &wanted {
        let values: Vec<Option<f64>> = match have.get(k) {
            None => vec![None; x.ids.len()],
            Some(results) => {
                let sd = abs.join("samples").join(k.to_string());
                match x.analyze(&script, results, &sd.join("out"), &sd.join("analysis"), &sd.join("analysis.log")) {
                    Ok(an) => x.ids.iter().map(|id| an.metrics.get(id).copied()).collect(),
                    Err(p) => {
                        analysis_failed += 1;
                        if analysis_failed <= 3 {
                            eprintln!("{label}: sample {k}: {}", p.message);
                        }
                        vec![None; x.ids.len()]
                    }
                }
            }
        };
        rows.push((*k, values));
    }
    if analysis_failed > 3 {
        eprintln!("{label}: the analysis failed for {analysis_failed} samples in all");
    }
    // samples.csv, and the statistics computed from it
    let samples = toa_repo::types::mc::Samples {
        metrics: x.ids.iter().map(|id| format!("{}.{id}", x.r.tb_name)).collect(),
        rows,
    };
    let csv = samples.text();
    let csv_path = abs.join(toa_repo::types::mc::SAMPLES);
    if let Some(d) = csv_path.parent() {
        std::fs::create_dir_all(d).map_err(|e| Fail::Stop(could_not_run(&format!("{}: {e}", d.display()))))?;
    }
    std::fs::write(&csv_path, &csv).map_err(|e| Fail::Stop(could_not_run(&format!("{}: {e}", csv_path.display()))))?;
    let read = toa_repo::types::mc::Samples::parse(csv.as_bytes())
        .map_err(|m| Fail::Stop(could_not_run(&format!("samples.csv does not read back: {m}"))))?;
    let stats: Vec<(String, toa_repo::types::mc::Stats)> = read
        .metrics
        .iter()
        .map(|m| (m.clone(), toa_repo::types::mc::stats(&read.column(m).unwrap_or_default())))
        .collect();
    let shown: Vec<String> = stats
        .iter()
        .map(|(m, s)| match (s.mean, s.sigma) {
            (Some(mean), Some(sigma)) => format!("{m} mean={mean} sigma={sigma} (n={})", s.n),
            (Some(mean), None) => format!("{m} mean={mean} (n={})", s.n),
            _ => format!("{m}: no sample gave it"),
        })
        .collect();
    let mut line = json!({"dir": dir.display().to_string(), "run": mode, "points": wanted.len(),
        "simulated": have.len(), "samples": csv_path.display().to_string(),
        "metrics": stats.iter().map(|(m, s)| (m.clone(), toa_repo::run::stats_json(s))).collect::<serde_json::Map<_, _>>()});
    let mut text = format!(
        "  ran {} sample(s) ({mode}) in {}: {} with results\n  {}",
        wanted.len(),
        dir.display(),
        have.len(),
        shown.join("\n  ")
    );
    if let (Some(into), Some(lic)) = (&x.ra.evd_into, &x.ra.license) {
        let s = x.short(label);
        let draft = x.draft(f, into, &s);
        let failed_sims = wanted.len() - have.len();
        let mut desc = format!(
            "Made by toa run {}: {} on {}, {}, a Monte Carlo simulation of {} samples, {}.",
            env!("CARGO_PKG_VERSION"),
            x.r.tb_name,
            a.tree.first().map(|t| t.1.as_str()).unwrap_or(""),
            x.conditions_said(label),
            wanted.len(),
            if mode == "batch" { "all in one simulation" } else { "one simulation for each" }
        );
        if failed_sims > 0 {
            desc.push_str(&format!(" The simulation of {} failed.", samples_word(failed_sims)));
        }
        if analysis_failed > 0 {
            desc.push_str(&format!(" The analysis script failed for {}.", samples_word(analysis_failed)));
        }
        let mut rec = x.record(lic, &s, &argv, &log, desc);
        rec.evidence = vec![(toa_repo::types::mc::SAMPLES.to_string(), "table".to_string(), None)];
        rec.mc = Some(toa_repo::run::McRecord { statistics: stats_run.clone(), stats });
        let problems = write_record(&a, &rec, &abs, &draft).map_err(|m| Fail::Stop(could_not_run(&m)))?;
        for p in &problems {
            out.problem(&draft.display().to_string(), p);
        }
        line["evd"] = json!(draft.display().to_string());
        text.push_str(&format!("\n  evidence record draft: {}", draft.display()));
    }
    out.line("ran", line, text);
    Ok(())
}

/// Write the draft of an evidence record, checked as `evd` asks; the problems found.
fn write_record(
    a: &assemble::Assembled,
    rec: &toa_repo::run::Record,
    output: &Path,
    draft: &Path,
) -> Result<Vec<toa_core::Problem>, String> {
    if draft.exists() {
        return Err(format!("{} is there already", draft.display()));
    }
    let yaml = toa_repo::run::evd_manifest(a, rec);
    let m = toa_core::manifest::parse(yaml.as_bytes())
        .map_err(|p| format!("the manifest written does not read back: {}", p[0]))?;
    let lock_text = serde_json::to_string_pretty(&a.lock).unwrap_or_default() + "\n";
    let mut files: Vec<(String, Vec<u8>)> =
        vec![("toa.yaml".into(), yaml.into_bytes()), ("lock.json".into(), lock_text.into_bytes())];
    for (p, _, _) in &rec.evidence {
        files.push((p.clone(), std::fs::read(output.join(p)).map_err(|e| format!("{p}: {e}"))?));
    }
    let mut dirs = a.objects.clone();
    dirs.insert(a.tb.name.clone(), a.tb.dir.clone());
    let o = toa_repo::types::Object { manifest: &m, files: &files, lock: Some(&a.lock) };
    let problems = toa_repo::types::check(toa_repo::types::EVD, &o, &Dirs(dirs));
    for (p, b) in &files {
        let to = draft.join(p);
        if let Some(d) = to.parent() {
            std::fs::create_dir_all(d).map_err(|e| format!("{}: {e}", d.display()))?;
        }
        std::fs::write(&to, b).map_err(|e| format!("{}: {e}", to.display()))?;
    }
    Ok(problems)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(s: &[&str]) -> Vec<String> {
        s.iter().map(|w| w.to_string()).collect()
    }

    #[test]
    fn the_launcher_around_the_command() {
        let argv = v(&["spectre", "input.scs", "+escchars", "-raw", "results"]);
        assert_eq!(
            launch("my-sandbox --read-only /models {args}", &argv),
            v(&["my-sandbox", "--read-only", "/models", "input.scs", "+escchars", "-raw", "results"])
        );
        assert_eq!(
            launch("srun -n1 {command}", &argv),
            v(&["srun", "-n1", "spectre", "input.scs", "+escchars", "-raw", "results"])
        );
        // no placeholder: a prefix
        assert_eq!(launch("srun -n1", &argv), launch("srun -n1 {command}", &argv));
        // inside a longer word: joined by spaces
        assert_eq!(
            launch("sh -c \"cd . && {command}\"", &argv),
            v(&["sh", "-c", "cd . && spectre input.scs +escchars -raw results"])
        );
    }

    #[test]
    fn a_command_that_is_not_a_simulator() {
        assert!(command_not_a_simulator("spectre {input} +aps").is_none());
        assert!(command_not_a_simulator("/opt/cadence/bin/spectre {input}").is_none());
        let w = command_not_a_simulator("my-sandbox --read-only /models {input}").unwrap();
        assert!(w.contains("my-sandbox") && w.contains("--launcher"), "{w}");
    }

    #[test]
    fn where_the_conditions_came_from() {
        assert_eq!(conditions_said(None, "vcm_sweep-3"), "conditions vcm_sweep-3");
        let f = Path::new("some").join("dir").join("mc.yaml");
        assert_eq!(conditions_said(Some(&f), "conditions"), "conditions from the file mc.yaml");
    }

    #[test]
    fn results_are_moved_never_lost() {
        let d = std::env::temp_dir().join(format!("toa-move-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(d.join("to")).unwrap();
        std::fs::write(d.join("a"), b"one").unwrap();
        move_file(&d.join("a"), &d.join("to/a")).unwrap();
        assert!(!d.join("a").exists());
        assert_eq!(std::fs::read(d.join("to/a")).unwrap(), b"one");
        // a file there already is not replaced, and the one to move stays where it is
        std::fs::write(d.join("b"), b"two").unwrap();
        assert!(move_file(&d.join("b"), &d.join("to/a")).is_err());
        assert_eq!(std::fs::read(d.join("b")).unwrap(), b"two");
        assert_eq!(std::fs::read(d.join("to/a")).unwrap(), b"one");
        let _ = std::fs::remove_dir_all(&d);
    }
}
