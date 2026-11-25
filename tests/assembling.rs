//! Assembling a run (7.6): a small library of a buffer made of two inverters, sealed, then
//! assembled under a preset of its testbench: the entry file, the renamed subcircuits, the
//! probes, `lock.json`, and the cases where the tool stops.

mod support;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use support::temp;
use toa_repo::assemble::{self, AssembleError, Assembled, Finder, Input};
use toa_repo::seal::{self, SealOptions};

fn write(p: &Path, t: &str) {
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, t.as_bytes()).unwrap();
}

const BUF_IFC: &str = "type: ifc
license: MIT
name: buf
title: A buffer
pins:
- {name: a}
- {name: z}
- {name: vdd}
- {name: vss}
probes:
- {name: mid, description: The node between the two stages}
";

const INV_IFC: &str = "type: ifc
license: MIT
name: inv
title: An inverter
pins:
- {name: a}
- {name: z}
- {name: vdd}
- {name: vss}
";

const INV_CKT: &str = "type: ckt
license: MIT
ifc: ifc_draft-inv
name: ideal
form: ideal
files:
  netlist: [inv.scs]
";

// the subcircuit is called inv1 here, and the buffer instantiates it as inv; both define helper
const INV_SCS: &str = "subckt helper (p n)
  r0 (p n) resistor r=1k
ends helper
subckt inv1 (a y vdd vss)
  x0 (a y) helper
  e0 (y vss a vss) vcvs gain=-1
ends inv1
";

const BUF_CKT: &str = "type: ckt
license: MIT
ifc: ifc_draft-buf
name: two
form: composition
uses:
  inv:
    ifc: ifc_draft-inv
    ckt: ckt_draft-inv
files:
  netlist: [buf.scs]
specifications: {process: fake}
";

const BUF_SCS: &str = "subckt helper (p n)
  c0 (p n) capacitor c=1f
ends helper
subckt buf (a y vdd vss)
  x1 (a m vdd vss) inv
  x2 (m y vdd vss) inv
  x3 (m vss) helper
ends buf
";

const TB: &str = "type: tb
license: MIT
ifc: ifc_draft-buf
name: tb_gain
title: Gain of a buffer
socket: dut_block
probes: [mid]
files:
  netlist: [tb.scs]
run: spectre tb.scs +aps
analyze: {entry: analyze.py, python: '>=3.6', results: psfascii}
metrics:
- {id: gain, unit: '1', better: higher, tolerance: {rel: 0.01}}
parameters:
  vdd: {unit: V, description: Supply}
presets:
  nom:
    parameters: {vdd: 1.8}
    options: {temp: 27}
    models:
    - {file: fake.scs, section: tt, match: [fake models]}
  mc:
    parameters: {vdd: 1.8}
    options: {temp: 27}
    models:
    - {file: fake.scs, section: tt}
    statistics: {method: mismatch, points: 200, sampling: lhs, seed: 12345}
  supply:
  - parameters: {vdd: 1.6}
    options: {temp: 27, reltol: 0.0001}
    models:
    - {file: fake.scs, section: tt}
  - parameters: {vdd: 2.0}
    options: {temp: 27, reltol: 0.0001}
    models:
    - {file: fake.scs, section: ss}
";

const TB_SCS: &str = "simulator lang=spectre
v0 (vdd 0) vsource dc=vdd
v1 (in 0) vsource dc=vdd/2
I0 (in out vdd 0) dut_block
dc dc
save out I0.mid
";

const ANALYZE: &str = "import sys\nprint(sys.argv)\n";

const MAP: &str = "toa: local-files/1
process:
  fake:
    release: '1.0'
    models:
    - file: pdk/fake.scs
      sections: {tt: typical, ss: slow}
";

struct Lib {
    root: PathBuf,
    tb: PathBuf,
    buf: PathBuf,
    by_name: BTreeMap<String, PathBuf>,
    /// Outside the library: the files of this machine and their map.
    machine: PathBuf,
}

impl Finder for Lib {
    fn find(&mut self, name: &str) -> Result<PathBuf, String> {
        self.by_name.get(name).cloned().ok_or_else(|| format!("{name} is not on this machine"))
    }
}

/// The library, sealed, with a model file and a local file map beside it.
fn library(label: &str, tb_scs: &str) -> Lib {
    let home = temp(&format!("{label}-home"));
    let root = temp(label);
    write(&root.join("ifc/buf/ifc_draft-buf/toa.yaml"), BUF_IFC);
    write(&root.join("ifc/inv/ifc_draft-inv/toa.yaml"), INV_IFC);
    write(&root.join("ckt/inv/ideal.g1/ckt_draft-inv/toa.yaml"), INV_CKT);
    write(&root.join("ckt/inv/ideal.g1/ckt_draft-inv/inv.scs"), INV_SCS);
    write(&root.join("ckt/buf/two.g1/ckt_draft-buf/toa.yaml"), BUF_CKT);
    write(&root.join("ckt/buf/two.g1/ckt_draft-buf/buf.scs"), BUF_SCS);
    write(&root.join("tb/buf/tb_gain.g1/tb_draft-gain/toa.yaml"), TB);
    write(&root.join("tb/buf/tb_gain.g1/tb_draft-gain/tb.scs"), tb_scs);
    write(&root.join("tb/buf/tb_gain.g1/tb_draft-gain/analyze.py"), ANALYZE);
    let g = support::system_git(&home);
    seal::seal(&g, &root, &SealOptions { dry_run: false, lookup: Vec::new(), workspace: None }).unwrap();
    // outside the library: the files of this machine
    let machine = temp(&format!("{label}-machine"));
    write(&machine.join("pdk/fake.scs"), "// fake models\nlibrary fake\nendlibrary fake\n");
    write(&machine.join(assemble::LOCAL_FILES_NAME), MAP);
    let mut by_name = BTreeMap::new();
    for d in seal::scan(&root).unwrap().objects {
        by_name.insert(d.file_name().unwrap().to_string_lossy().to_string(), d);
    }
    let pick = |prefix: &str, parent: &str| {
        by_name.values().find(|d| d.parent().unwrap().ends_with(parent) && leaf(d).starts_with(prefix)).unwrap().clone()
    };
    let tb = pick("tb_", "tb_gain.g1");
    let buf = pick("ckt_", "two.g1");
    Lib { root, tb, buf, by_name, machine }
}

fn leaf(p: &Path) -> String {
    p.file_name().unwrap().to_string_lossy().to_string()
}

fn input(lib: &Lib, preset: &str, index: usize, probe: &str, out: &Path) -> Input {
    let tb = assemble::load(&lib.tb).unwrap();
    let sets = assemble::preset_sets(&tb.manifest, preset).unwrap();
    let map = assemble::read_local_files(&lib.machine.join(assemble::LOCAL_FILES_NAME)).unwrap();
    Input {
        tb: lib.tb.clone(),
        dut: lib.buf.clone(),
        conditions: sets[index].clone(),
        probes: [("mid".to_string(), probe.to_string())].into_iter().collect(),
        map,
        out: out.to_path_buf(),
        dry_run: false,
        monte_carlo: None,
    }
}

fn codes(r: Result<Assembled, AssembleError>) -> Vec<&'static str> {
    match r {
        Ok(a) => panic!("assembled {}, though it should have stopped", a.dir.display()),
        Err(AssembleError::Problems(p)) => p.iter().map(|p| p.code).collect(),
        Err(AssembleError::Io(e)) => panic!("{e}"),
    }
}

fn read(p: &Path) -> String {
    std::fs::read_to_string(p).unwrap_or_else(|e| panic!("{}: {e}", p.display()))
}

#[test]
fn a_run_is_assembled() {
    let mut lib = library("asm-ok", TB_SCS);
    // inside the library, as a user may put it
    let out = lib.root.join("runs/supply-1");
    let mut i = input(&lib, "supply", 1, "x1.y", &out);

    // a dry run checks everything and writes nothing
    i.dry_run = true;
    assemble::assemble(&i, &mut lib).unwrap();
    assert!(!out.exists());

    i.dry_run = false;
    let a = assemble::assemble(&i, &mut lib).unwrap();
    assert_eq!(a.entry, "input.scs");
    let buf = leaf(&lib.buf);
    let inv = a.tree.iter().find(|r| r.0 == "inv").unwrap().1.clone();
    assert_eq!(a.tree[0].1, buf, "the circuit tested comes first");
    let mut renamed: Vec<_> = a.renamed.iter().map(|(o, from, to)| (o.as_str(), from.as_str(), to.as_str())).collect();
    renamed.sort();
    let mut want =
        vec![(buf.as_str(), "buf", "dut_block"), (inv.as_str(), "inv1", "inv"), (inv.as_str(), "helper", "helper_2")];
    want.sort();
    assert_eq!(renamed, want);

    // the copies: renamed definitions and instances, the probe replaced, the originals intact
    let b = read(&out.join("ckt").join(&buf).join("buf.scs"));
    assert!(b.contains("subckt dut_block (a y vdd vss)") && b.contains("ends dut_block"), "{b}");
    assert!(b.contains("x3 (m vss) helper\n"), "the circuit tested keeps its own helper: {b}");
    let v = read(&out.join("ckt").join(&inv).join("inv.scs"));
    assert!(
        v.contains("subckt helper_2 (p n)") && v.contains("x0 (a y) helper_2") && v.contains("subckt inv (a y"),
        "{v}"
    );
    assert_eq!(read(&lib.by_name[&inv].join("inv.scs")), INV_SCS);
    let t = read(&out.join("tb/tb.scs"));
    assert!(t.contains("save out I0.x1.y\n"), "{t}");
    assert!(out.join("tb/analyze.py").is_file());

    // the entry file, in the order of 7.6
    let model = lib.machine.join("pdk/fake.scs");
    let e = read(&out.join("input.scs"));
    let lines: Vec<&str> = e.lines().collect();
    assert_eq!(lines[0], "simulator lang=spectre");
    let at = |s: &str| lines.iter().position(|l| l.contains(s)).unwrap_or_else(|| panic!("{s} not in\n{e}"));
    assert!(lines[at("section=slow")].starts_with("include \""), "{e}");
    // written with / on every system
    assert!(lines[at("section=slow")].contains(&model.to_string_lossy().replace('\\', "/")), "{e}");
    assert!(at("section=slow") < at(&format!("ckt/{buf}/buf.scs")));
    assert!(at(&format!("ckt/{buf}/buf.scs")) < at(&format!("ckt/{inv}/inv.scs")));
    assert!(at(&format!("ckt/{inv}/inv.scs")) < at("tb/tb.scs"));
    assert_eq!(lines[at("parameters ")], "parameters vdd=2");
    assert_eq!(lines[at("toaConditions")], "toaConditions options temp=27 reltol=0.0001");
    assert!(at("tb/tb.scs") < at("parameters "));

    // lock.json names the tree; the run directory is not walked as part of the library
    let lock: serde_json::Value = serde_json::from_str(&read(&out.join("lock.json"))).unwrap();
    assert!(lock.to_string().contains(&inv), "{lock}");
    assert!(seal::is_run_directory(&out));
    assert_eq!(
        seal::scan(&lib.root).unwrap().objects.len(),
        lib.by_name.len(),
        "the copies under runs/ are not objects"
    );

    // a directory that is not empty is not written over
    assert!(codes(assemble::assemble(&i, &mut lib)).contains(&"E_ASSEMBLE_EXISTS"));
}

#[test]
fn the_tool_stops() {
    let mut lib = library("asm-stop", TB_SCS);
    let base = temp("asm-stop-run");

    // a probe that leads nowhere
    let i = input(&lib, "nom", 0, "x9.y", &base.join("a"));
    assert!(codes(assemble::assemble(&i, &mut lib)).contains(&"E_PROBES"));
    // a probe not given
    let mut i = input(&lib, "nom", 0, "x1.y", &base.join("b"));
    i.probes.clear();
    assert!(codes(assemble::assemble(&i, &mut lib)).contains(&"E_PROBES"));
    // a parameter missing from the conditions
    let mut i = input(&lib, "nom", 0, "x1.y", &base.join("c"));
    i.conditions["parameters"] = serde_json::json!({});
    assert!(codes(assemble::assemble(&i, &mut lib)).contains(&"E_CONDITIONS"));
    // a model file the map does not have
    let mut i = input(&lib, "nom", 0, "x1.y", &base.join("d"));
    i.conditions["models"] = serde_json::json!([{"file": "other.scs", "section": "tt"}]);
    assert!(codes(assemble::assemble(&i, &mut lib)).contains(&"E_LOCAL_FILES"));
    // a circuit of the tree not on this machine
    let inv = lib.by_name.keys().find(|n| lib.by_name[*n].parent().unwrap().ends_with("ideal.g1")).unwrap().clone();
    lib.by_name.remove(&inv);
    let i = input(&lib, "nom", 0, "x1.y", &base.join("e"));
    assert!(codes(assemble::assemble(&i, &mut lib)).contains(&"E_ASSEMBLE_FIND"));
    for d in ["a", "b", "c", "d", "e"] {
        assert!(!base.join(d).exists(), "{d} was written");
    }
}

#[test]
fn a_testbench_that_sets_its_own_conditions_is_refused() {
    // sealing already refuses it (tb, check 7); a tool that assembles checks again
    let tb = TB_SCS.replace("dc dc\n", "parameters vdd=1.8\ndc dc\n");
    let home = temp("asm-own-home");
    let root = temp("asm-own");
    write(&root.join("ifc/buf/ifc_draft-buf/toa.yaml"), BUF_IFC);
    write(&root.join("tb/buf/tb_gain.g1/tb_draft-gain/toa.yaml"), TB);
    write(&root.join("tb/buf/tb_gain.g1/tb_draft-gain/tb.scs"), &tb);
    write(&root.join("tb/buf/tb_gain.g1/tb_draft-gain/analyze.py"), ANALYZE);
    let g = support::system_git(&home);
    let r = seal::seal(&g, &root, &SealOptions { dry_run: false, lookup: Vec::new(), workspace: None });
    let text = format!("{r:?}");
    assert!(text.contains("E_TB_CONDITIONS"), "{text}");
}

#[test]
fn a_monte_carlo_is_assembled() {
    let mut lib = library("asm-mc", TB_SCS);
    let base = temp("asm-mc-run");
    let mut i = input(&lib, "mc", 0, "x1.y", &base.join("a"));
    // statistics, and no samples said: the tool stops
    assert!(codes(assemble::assemble(&i, &mut lib)).contains(&"E_ASSEMBLE_MC"));
    // sample 7 alone; the analyses inside the statement, the mismatch on the block measured
    let st = i.conditions["statistics"].clone();
    i.monte_carlo = Some(assemble::Samples::of(&st, Some(7)).unwrap());
    let a = assemble::assemble(&i, &mut lib).unwrap();
    let t = read(&a.dir.join("tb/tb.scs"));
    assert_eq!(
        t,
        "simulator lang=spectre\nv0 (vdd 0) vsource dc=vdd\nv1 (in 0) vsource dc=vdd/2\nI0 (in out vdd 0) dut_block\n\
         toaMonteCarlo montecarlo variations=mismatch sampling=lhs seed=12345 numruns=1 firstrun=7 donominal=no \
         savefamilyplots=yes dut=[I0] numbins=200 {\ndc dc\n}\nsave out I0.x1.y\n"
    );
    // all of them
    let mut i = input(&lib, "mc", 0, "x1.y", &base.join("b"));
    i.monte_carlo = Some(assemble::Samples::of(&st, None).unwrap());
    let a = assemble::assemble(&i, &mut lib).unwrap();
    assert!(read(&a.dir.join("tb/tb.scs")).contains(" numruns=200 firstrun=1 "));
    assert!(assemble::Samples::of(&st, Some(201)).is_err());
    // a testbench with no analysis cannot be run so
    let mut lib = library("asm-mc-none", &TB_SCS.replace("dc dc\n", ""));
    let mut i = input(&lib, "mc", 0, "x1.y", &base.join("c"));
    i.monte_carlo = Some(assemble::Samples::of(&st, None).unwrap());
    assert!(codes(assemble::assemble(&i, &mut lib)).contains(&"E_ASSEMBLE_MC"));
}
