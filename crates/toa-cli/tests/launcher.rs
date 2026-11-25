//! `toa run --launcher` (decisions 11-25: a published record names the simulator's own command):
//! the launcher of this machine runs the simulator's command, the record carries that command
//! and no word of the launcher. A small library of drafts, a fake Spectre in Python that writes
//! the words it was called with into `argv.json`, and the `toa` program itself.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

/// The command a record must carry: `run` of the testbench pointed at the entry file and results.
const STANDARD: &str = "spectre input.scs +escchars +aps -format psfascii -raw results";

/// Words of the launchers that must never reach a record.
const MARKERS: &[&str] = &["sbx", "fake_sim"];

const BUF_IFC: &str = "type: ifc
license: MIT
name: buf
title: A buffer
pins:
- {name: a}
- {name: z}
- {name: vdd}
- {name: vss}
";

const BUF_CKT: &str = "type: ckt
license: MIT
ifc: ifc_draft-buf
name: ideal
form: ideal
files:
  netlist: [buf.scs]
";

const BUF_SCS: &str = "subckt buf (a y vdd vss)
  e0 (y vss a vss) vcvs gain=1
ends buf
";

const TB: &str = "type: tb
license: MIT
ifc: ifc_draft-buf
name: tb_gain
title: Gain of a buffer
socket: dut_block
files:
  netlist: [tb.scs]
run: spectre tb.scs +escchars +aps
analyze: {entry: analyze.py, python: '>=3.6', results: psfascii}
metrics:
- {id: gain, unit: '1', better: higher, tolerance: {rel: 0.01}}
parameters:
  vdd: {unit: V, description: Supply}
presets:
  nom:
    parameters: {vdd: 1.8}
    options: {temp: 27}
  mc:
    parameters: {vdd: 1.8}
    options: {temp: 27}
    statistics: {method: mismatch, points: 3, sampling: lds, seed: 12345}
";

const TB_SCS: &str = "simulator lang=spectre
v0 (vdd 0) vsource dc=vdd
v1 (in 0) vsource dc=vdd/2
I0 (in out vdd 0) dut_block
dc dc
save out
";

const ANALYZE: &str = r#"import json, os, sys
results, output = sys.argv[1], sys.argv[2]
with open(os.path.join(results, "conditions.json")) as f:
    c = json.load(f)
k = c.get("sample", 0)
os.makedirs(output, exist_ok=True)
with open(os.path.join(output, "metrics.json"), "w") as f:
    json.dump({"gain": 1.0 + 0.001 * k}, f)
"#;

/// A stand-in for Spectre: whatever words it is called with go into argv.json; it writes the
/// results of a simulation, or of each sample of a Monte Carlo one, as Spectre names them.
const FAKE: &str = r#"import json, os, re, sys
print("Cadence (R) Virtuoso (R) Spectre (R) Circuit Simulator")
print("Version 99.1.0 64bit -- a stand-in")
with open("argv.json", "w") as f:
    json.dump(sys.argv[1:], f)
tb = open(os.path.join("tb", "tb.scs")).read()
os.makedirs("results", exist_ok=True)
m = re.search(r"toaMonteCarlo montecarlo .*numruns=(\d+) firstrun=(\d+)", tb)
if not m:
    open(os.path.join("results", "dc.dc"), "w").write("dc\n")
    sys.exit(0)
n, first = int(m.group(1)), int(m.group(2))
for k in range(first, first + n):
    open(os.path.join("results", "toaMonteCarlo-%03d_dc.dc" % k), "w").write("sample %d\n" % k)
"#;

fn write(p: &Path, t: &str) {
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, t.as_bytes()).unwrap();
}

fn temp(label: &str) -> PathBuf {
    static N: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let n = N.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    let d = std::env::temp_dir().join("toa-cli-tests").join(format!("{label}-{}-{n}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// The Python of this machine, as `toa run` looks for it; `None` when there is none.
fn python() -> Option<String> {
    ["python3", "python"].into_iter().map(str::to_string).find(|p| {
        Command::new(p).arg("--version").output().is_ok_and(|o| o.status.success() && text(&o).contains("Python"))
    })
}

struct Lib {
    root: PathBuf,
    tb: PathBuf,
    ckt: PathBuf,
    map: PathBuf,
    cache: PathBuf,
    fake: PathBuf,
    sha: String,
}

fn library(label: &str) -> Lib {
    let root = temp(label);
    let lib = root.join("lib");
    write(&lib.join("ifc/buf/ifc_draft-buf/toa.yaml"), BUF_IFC);
    let ckt = lib.join("ckt/buf/ideal.g1/ckt_draft-buf");
    write(&ckt.join("toa.yaml"), BUF_CKT);
    write(&ckt.join("buf.scs"), BUF_SCS);
    let tb = lib.join("tb/buf/tb_gain.g1/tb_draft-gain");
    write(&tb.join("toa.yaml"), TB);
    write(&tb.join("tb.scs"), TB_SCS);
    write(&tb.join("analyze.py"), ANALYZE);
    // sealed: a draft is not assembled
    let o = Command::new(env!("CARGO_BIN_EXE_toa")).arg("seal").arg(&lib).output().unwrap();
    assert!(o.status.success(), "seal: {}", text(&o));
    let sealed = |parent: &Path, prefix: &str| {
        std::fs::read_dir(parent)
            .unwrap()
            .flatten()
            .map(|e| e.path())
            .find(|p| p.is_dir() && p.file_name().unwrap().to_string_lossy().starts_with(prefix))
            .unwrap_or_else(|| panic!("no sealed {prefix} in {}", parent.display()))
    };
    let tb = sealed(&lib.join("tb/buf/tb_gain.g1"), "tb_");
    let ckt = sealed(&lib.join("ckt/buf/ideal.g1"), "ckt_");
    let map = root.join("machine/local-files.yaml");
    write(&map, "toa: local-files/1\n");
    let fake = root.join("machine/fake_sim.py");
    write(&fake, FAKE);
    let cache = root.join("cache");
    std::fs::create_dir_all(&cache).unwrap();
    let sha = toa_repo::run::sha256(ANALYZE.as_bytes());
    Lib { root, tb, ckt, map, cache, fake, sha }
}

/// `toa run` of the library's testbench on its circuit, with `more` options, in `out`.
fn run(lib: &Lib, py: &str, out: &Path, more: &[&str]) -> Output {
    let mut args: Vec<String> = vec![
        "run".into(),
        lib.tb.display().to_string(),
        lib.ckt.display().to_string(),
        "--map".into(),
        lib.map.display().to_string(),
        "--out".into(),
        out.display().to_string(),
        "--offline".into(),
        "--python".into(),
        py.into(),
        "--agree".into(),
        lib.sha[..16].to_string(),
    ];
    args.extend(more.iter().map(|s| s.to_string()));
    let o = Command::new(env!("CARGO_BIN_EXE_toa"))
        .args(&args)
        .current_dir(&lib.root)
        .env("TOA_CACHE", &lib.cache)
        .output()
        .unwrap();
    o
}

fn text(o: &Output) -> String {
    format!("{}\n{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr))
}

/// The launcher templates: the fake Spectre, called through Python, with words of their own.
fn launcher(py: &str, lib: &Lib, form: &str) -> String {
    let head = format!("\"{py}\" \"{}\"", lib.fake.display());
    match form {
        "args" => format!("{head} --sbx-ro sbxmodels {{args}}"),
        "command" => format!("{head} --sbx-wrap {{command}}"),
        _ => format!("{head} --sbx-prefix"),
    }
}

/// The words the fake Spectre was called with, after Python and its script.
fn argv_in(dir: &Path) -> Vec<String> {
    let b = std::fs::read(dir.join("argv.json")).unwrap_or_else(|e| panic!("{}: {e}", dir.display()));
    serde_json::from_slice(&b).unwrap()
}

fn expected(form: &str) -> Vec<String> {
    let std: Vec<String> = STANDARD.split(' ').map(str::to_string).collect();
    let mut w: Vec<String> = match form {
        "args" => vec!["--sbx-ro".into(), "sbxmodels".into()],
        "command" => vec!["--sbx-wrap".into()],
        _ => vec!["--sbx-prefix".into()],
    };
    if form == "args" {
        w.extend(std[1..].iter().cloned());
    } else {
        w.extend(std);
    }
    w
}

/// Every file under `dir`.
fn files(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut todo = vec![dir.to_path_buf()];
    while let Some(d) = todo.pop() {
        for e in std::fs::read_dir(&d).unwrap().flatten() {
            let p = e.path();
            if p.is_dir() {
                todo.push(p);
            } else {
                out.push(p);
            }
        }
    }
    out
}

/// The record drafts under `into`, each with its manifest; and no launcher word in any file of
/// a record, nor in lock.json or conditions.json of the run.
fn records_clean(into: &Path, out: &Path) -> Vec<toa_core::manifest::Node> {
    let mut manifests = Vec::new();
    for f in files(into) {
        let t = String::from_utf8_lossy(&std::fs::read(&f).unwrap()).to_string();
        for m in MARKERS {
            assert!(!t.contains(m), "{} holds {m}:\n{t}", f.display());
        }
        if f.file_name().unwrap() == "toa.yaml" {
            manifests.push(toa_core::manifest::parse(t.as_bytes()).unwrap());
        }
    }
    let run_files: Vec<PathBuf> = files(out)
        .into_iter()
        .filter(|f| ["lock.json", "conditions.json"].contains(&f.file_name().unwrap().to_str().unwrap()))
        .collect();
    assert!(!run_files.is_empty());
    for f in run_files {
        let t = std::fs::read_to_string(&f).unwrap();
        for m in MARKERS {
            assert!(!t.contains(m), "{} holds {m}:\n{t}", f.display());
        }
    }
    assert!(!manifests.is_empty(), "no record under {}", into.display());
    manifests
}

fn command_of(m: &toa_core::manifest::Node) -> String {
    let sim = m.get("environment").and_then(|e| e.get("simulator")).expect("environment.simulator");
    assert_eq!(sim.get("name").and_then(|n| n.as_str()), Some("spectre"));
    sim.get("command").and_then(|c| c.as_str()).expect("simulator.command").to_string()
}

#[test]
fn a_launcher_runs_the_command_and_is_not_recorded() {
    let Some(py) = python() else {
        eprintln!("no Python on this machine: skipped");
        return;
    };
    let lib = library("launcher");
    for form in ["args", "command", "prefix"] {
        let out = lib.root.join(format!("run-{form}"));
        let into = lib.root.join(format!("records-{form}"));
        let l = launcher(&py, &lib, form);
        let o = run(
            &lib,
            &py,
            &out,
            &["--preset", "nom", "--launcher", &l, "--evd-into", &into.display().to_string(), "--license", "MIT"],
        );
        assert!(o.status.success(), "{form}: {}", text(&o));
        assert!(!text(&o).contains("warning"), "{form}: {}", text(&o));
        assert_eq!(argv_in(&out), expected(form), "{form}");
        for m in records_clean(&into, &out) {
            assert_eq!(command_of(&m), STANDARD, "{form}");
        }
    }
}

#[test]
fn a_monte_carlo_runs_through_the_launcher() {
    let Some(py) = python() else {
        eprintln!("no Python on this machine: skipped");
        return;
    };
    let lib = library("launcher-mc");
    for mode in ["batch", "each"] {
        let out = lib.root.join(format!("run-{mode}"));
        let into = lib.root.join(format!("records-{mode}"));
        let l = launcher(&py, &lib, "args");
        let o = run(
            &lib,
            &py,
            &out,
            &[
                "--preset",
                "mc",
                "--mc-run",
                mode,
                "--launcher",
                &l,
                "--evd-into",
                &into.display().to_string(),
                "--license",
                "MIT",
            ],
        );
        assert!(o.status.success(), "{mode}: {}", text(&o));
        // the standard command inside the launcher, once in batch, once for each sample in each
        let dirs: Vec<PathBuf> = if mode == "batch" {
            vec![out.clone()]
        } else {
            (1..=3).map(|k| out.join("runs").join(k.to_string())).collect()
        };
        for d in &dirs {
            assert_eq!(argv_in(d), expected("args"), "{mode}: {}", d.display());
        }
        for k in 1..=3 {
            assert!(out.join("samples").join(k.to_string()).join("results/dc.dc").is_file(), "{mode}: sample {k}");
        }
        let ms = records_clean(&into, &out);
        assert_eq!(ms.len(), 1, "{mode}");
        assert_eq!(ms[0].get("kind").and_then(|k| k.as_str()), Some("mc"), "{mode}");
        assert_eq!(command_of(&ms[0]), STANDARD, "{mode}");
    }
}

#[test]
fn a_command_that_is_not_a_simulator_is_warned_of() {
    let lib = library("launcher-warn");
    let py = python().unwrap_or_else(|| "python3".into());
    // a wrapper given with --command: warned of, and still run (here a dry run)
    let o = run(&lib, &py, &lib.root.join("w"), &["--preset", "nom", "--dry-run", "--command", "sbx-wrap {input}"]);
    assert!(o.status.success(), "{}", text(&o));
    let t = text(&o);
    assert!(t.contains("warning") && t.contains("sbx-wrap") && t.contains("--launcher"), "{t}");
    // the simulator's own command line: no warning
    let o = run(
        &lib,
        &py,
        &lib.root.join("s"),
        &["--preset", "nom", "--dry-run", "--command", "spectre {input} +aps -format psfascii -raw results"],
    );
    assert!(o.status.success(), "{}", text(&o));
    assert!(!text(&o).contains("warning"), "{}", text(&o));
    // a dry run shows what the launcher runs, and the command apart from it
    let o = run(&lib, &py, &lib.root.join("d"), &["--preset", "nom", "--dry-run", "--launcher", "srun -n1"]);
    assert!(o.status.success(), "{}", text(&o));
    let t = text(&o);
    assert!(t.contains(&format!("srun -n1 {STANDARD}")), "{t}");
}
