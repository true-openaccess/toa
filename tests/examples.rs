//! The small examples in `toa/examples/` (see its README), built with the tool from drafts:
//! four libraries, one of them a fork; notes; a removal with its record; `latest` marks, which
//! `toa publish` gives to the first of a series and moves from one generation to the next; a
//! frozen library; a library list in volumes and one that includes
//! other lists by address; and a workspace list. Every commit passes the library check (6.4), and
//! `toa index` over the workspace list finds no errors.
//!
//! The files of the libraries built must equal those kept in `toa/examples/`. Set
//! `TOA_EXAMPLES_WRITE=1` to write them again, and `TOA_EXAMPLES_OUT=<dir>` to keep the built
//! repositories, with their history, and their index in `<dir>`.

#[path = "support.rs"]
mod support;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde_json::Value;
use support::write;
use toa_core::hash::{self, Algorithm};
use toa_repo::git::SystemGit;
use toa_repo::{index, library, publish, remove, seal};

/// The libraries, in the order of the workspace list.
const LIBS: [&str; 4] = ["official", "lee", "alice", "official-chen"];

fn address(lib: &str) -> String {
    format!("https://example.org/toa-examples/{lib}.git")
}

fn sha256(b: &[u8]) -> String {
    hash::hex(&hash::digest(Algorithm::Sha256, b).unwrap())
}

struct Build {
    root: PathBuf,
    home: PathBuf,
    date: String,
    /// Draft name -> object name, of every draft sealed so far.
    names: BTreeMap<String, String>,
    /// Problems other than errors the library check reported, to be shown.
    notes: Vec<String>,
}

impl Build {
    fn env(&self) -> Vec<(String, String)> {
        let mut e: Vec<(String, String)> =
            support::env(&self.home).into_iter().filter(|(k, _)| !k.ends_with("_DATE")).collect();
        e.push(("GIT_AUTHOR_DATE".into(), self.date.clone()));
        e.push(("GIT_COMMITTER_DATE".into(), self.date.clone()));
        e
    }

    fn sg(&self) -> SystemGit {
        let mut g = SystemGit::new();
        g.env = self.env().into_iter().map(|(k, v)| (k.into(), v.into())).collect();
        g
    }

    fn git(&self, lib: &str, args: &[&str]) -> String {
        let out = std::process::Command::new("git")
            .arg("-C")
            .arg(self.root.join(lib))
            .args(args)
            .envs(self.env())
            .stdin(std::process::Stdio::null())
            .output()
            .expect("git runs");
        assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    }

    /// The object name of a draft sealed earlier.
    fn n(&self, draft: &str) -> String {
        self.names.get(draft).unwrap_or_else(|| panic!("{draft} is not sealed yet")).clone()
    }

    /// Write a file of a library; `$(draft)` stands for the name the draft was sealed as.
    fn put(&self, lib: &str, rel: &str, text: &str) {
        let mut s = text.to_string();
        while let Some(i) = s.find("$(") {
            let j = s[i..].find(')').unwrap() + i;
            let v = self.n(&s[i + 2..j]);
            s.replace_range(i..=j, &v);
        }
        let rel = self.fill_path(rel);
        write(&self.root.join(lib).join(rel), s.as_bytes());
    }

    fn fill_path(&self, rel: &str) -> String {
        let mut s = rel.to_string();
        while let Some(i) = s.find("$(") {
            let j = s[i..].find(')').unwrap() + i;
            let v = self.n(&s[i + 2..j]);
            s.replace_range(i..=j, &v);
        }
        s
    }

    fn new_library(&self, lib: &str) {
        let d = self.root.join(lib);
        std::fs::create_dir_all(&d).unwrap();
        self.git(lib, &["init", "--quiet", "--object-format=sha1"]);
        self.git(lib, &["symbolic-ref", "HEAD", "refs/heads/main"]);
        write(&d.join(".gitattributes"), b"* -text\n");
    }

    /// Seal the drafts there are now, without publishing or committing.
    fn seal_only(&mut self, lib: &str) {
        let ws = self.root.join("workspace.source.jsonl");
        let sealed = seal::seal(
            &self.sg(),
            &self.root.join(lib),
            &seal::SealOptions { dry_run: false, lookup: Vec::new(), workspace: Some(ws) },
        )
        .unwrap_or_else(|e| panic!("{lib}: sealing: {e:?}"));
        for (d, o, _) in sealed.objects {
            self.names.insert(d, o);
        }
    }

    /// Write an object as it was sealed by an earlier version of the tool, under the name its
    /// files give it: an object published before the rules of its type changed (`$(draft)` in
    /// its files filled first). Sealing leaves it as it is.
    fn put_sealed(&mut self, lib: &str, parent: &str, draft: &str, files: &[(&str, String)]) {
        let filled: Vec<(String, String)> = files
            .iter()
            .map(|(p, t)| {
                let mut s = t.clone();
                while let Some(i) = s.find("$(") {
                    let j = s[i..].find(')').unwrap() + i;
                    let v = self.n(&s[i + 2..j]);
                    s.replace_range(i..=j, &v);
                }
                (p.to_string(), s)
            })
            .collect();
        let refs: Vec<(&str, &[u8])> = filled.iter().map(|(p, t)| (p.as_str(), t.as_bytes())).collect();
        let prefix = &draft[..draft.find('_').unwrap()];
        let name = format!("{prefix}_{}", hash::hash_files(Algorithm::Sha1, &refs).unwrap());
        for (p, t) in &filled {
            write(&self.root.join(lib).join(parent).join(&name).join(p), t.as_bytes());
        }
        self.names.insert(draft.to_string(), name);
    }

    /// Seal the drafts, tidy by `toa publish` (7.2) and commit: one commit, checked by 6.4
    /// against the one before.
    fn step(&mut self, lib: &str, date: &str, msg: &str, volumes: bool) -> String {
        self.date = date.to_string();
        let g = self.sg();
        let dir = self.root.join(lib);
        let ws = self.root.join("workspace.source.jsonl");
        let sealed = seal::seal(
            &g,
            &dir,
            &seal::SealOptions { dry_run: false, lookup: Vec::new(), workspace: Some(ws.clone()) },
        )
        .unwrap_or_else(|e| panic!("{lib}: sealing: {e:?}"));
        for (d, o, _) in sealed.objects {
            self.names.insert(d, o);
        }
        let o = publish::Options {
            to: Some(address(lib)),
            branch: "main".into(),
            workspace: Some(ws),
            volumes,
            ..Default::default()
        };
        let p = publish::publish(&g, &dir, &o).unwrap_or_else(|e| panic!("{lib}: publishing: {e:?}"));
        assert!(p.name_only.is_empty(), "{lib}: objects left with their names only: {:?}", p.name_only);
        assert!(p.lost_ancestors.is_empty(), "{lib}: ancestors others cannot find: {:?}", p.lost_ancestors);
        let parent = std::process::Command::new("git")
            .arg("-C")
            .arg(&dir)
            .args(["rev-parse", "--verify", "--quiet", "HEAD"])
            .envs(self.env())
            .output()
            .ok()
            .filter(|o| o.status.success())
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string());
        self.git(lib, &["add", "--all"]);
        self.git(lib, &["commit", "--quiet", "-m", msg]);
        let commit = self.git(lib, &["rev-parse", "HEAD"]);
        self.check(lib, &commit, parent.as_deref());
        commit
    }

    /// A commit of what is in the working tree as it is, such as notes or a removal.
    fn commit(&mut self, lib: &str, date: &str, msg: &str) -> String {
        self.date = date.to_string();
        let parent = self.git(lib, &["rev-parse", "HEAD"]);
        self.git(lib, &["add", "--all"]);
        self.git(lib, &["commit", "--quiet", "-m", msg]);
        let commit = self.git(lib, &["rev-parse", "HEAD"]);
        self.check(lib, &commit, Some(&parent));
        commit
    }

    fn check(&mut self, lib: &str, commit: &str, base: Option<&str>) {
        let g = support::reader(&self.home);
        let r = library::check(g.as_ref(), &self.root.join(lib), commit, base, &Default::default()).unwrap();
        let errors: Vec<_> = r.problems.iter().filter(|p| p.is_error()).collect();
        assert!(errors.is_empty(), "{lib} {commit}: the library check: {errors:#?}");
        for p in r.problems {
            self.notes.push(format!("{lib} {}: {} {} {}", &commit[..12], p.code, p.path, p.message));
        }
    }
}

// ------------------------------------------------------------------ the content

const PINS: &str = "pins:
- {name: inp, kind: analog, direction: in}
- {name: inn, kind: analog, direction: in}
- {name: clk, kind: digital, direction: in}
- {name: outp, kind: digital, direction: out}
- {name: outn, kind: digital, direction: out}
- {name: vdd, kind: supply}
- {name: vss, kind: ground}
";

const RAZAVI: &str = "// StrongARM latched comparator, after Razavi; devices of sky130
subckt strongarm_comp inp inn clk outp outn vdd vss
  m0 (tail clk vss vss) nfet_01v8 w=4u l=0.15u
  m1 (x1 inp tail vss) nfet_01v8 w=2u l=0.15u
  m2 (x2 inn tail vss) nfet_01v8 w=2u l=0.15u
  m3 (outn outp x1 vss) nfet_01v8 w=1u l=0.15u
  m4 (outp outn x2 vss) nfet_01v8 w=1u l=0.15u
  m5 (outn outp vdd vdd) pfet_01v8 w=1u l=0.15u
  m6 (outp outn vdd vdd) pfet_01v8 w=1u l=0.15u
  m7 (outn clk vdd vdd) pfet_01v8 w=1u l=0.15u
  m8 (outp clk vdd vdd) pfet_01v8 w=1u l=0.15u
ends strongarm_comp
";

const LOWKICK: &str = "// StrongARM latched comparator with less kickback: the input pair is isolated
// from the latch by two switches while the latch regenerates; devices of sky130
subckt strongarm_comp inp inn clk outp outn vdd vss
  m0 (tail clk vss vss) nfet_01v8 w=4u l=0.15u
  m1 (y1 inp tail vss) nfet_01v8 w=2u l=0.15u
  m2 (y2 inn tail vss) nfet_01v8 w=2u l=0.15u
  m9 (x1 clk y1 vss) nfet_01v8 w=2u l=0.15u
  m10 (x2 clk y2 vss) nfet_01v8 w=2u l=0.15u
  m3 (outn outp x1 vss) nfet_01v8 w=1u l=0.15u
  m4 (outp outn x2 vss) nfet_01v8 w=1u l=0.15u
  m5 (outn outp vdd vdd) pfet_01v8 w=1u l=0.15u
  m6 (outp outn vdd vdd) pfet_01v8 w=1u l=0.15u
  m7 (outn clk vdd vdd) pfet_01v8 w=1u l=0.15u
  m8 (outp clk vdd vdd) pfet_01v8 w=1u l=0.15u
ends strongarm_comp
";

const TB: &str = "// Input-referred offset: a slow differential ramp, the comparator clocked at 100 MHz
simulator lang=spectre
parameters vdd=1.8
vdd (vdd 0) vsource dc=vdd
vclk (clk 0) vsource type=pulse val0=0 val1=vdd period=10n width=5n rise=50p fall=50p
vinp (inp 0) vsource type=pwl wave=[0 0.9-1m 1u 0.9+1m]
vinn (inn 0) vsource dc=0.9
dut (inp inn clk outp outn vdd 0) strongarm_comp
tran tran stop=1u
";

/// The netlist of `tb_offset.g2`: the supply's value is not declared here; assembling a run
/// writes it with the conditions (7.6).
const TB2: &str = "// Input-referred offset: a slow differential ramp, the comparator clocked at 100 MHz
simulator lang=spectre
vdd (vdd 0) vsource dc=vdd
vclk (clk 0) vsource type=pulse val0=0 val1=vdd period=10n width=5n rise=50p fall=50p
vinp (inp 0) vsource type=pwl wave=[0 0.9-1m 1u 0.9+1m]
vinn (inn 0) vsource dc=0.9
dut (inp inn clk outp outn vdd 0) strongarm_comp
tran tran stop=1u
";

const ANALYZE: &str = "\"\"\"The offset: the input at which the decisions turn over.\"\"\"


def offset(vin, decisions):
    for v, d in zip(vin, decisions):
        if d:
            return v * 1e3
    return None
";

fn latch(width: &str) -> String {
    format!(
        "// Dynamic latch comparator by Lee; devices of sky130
subckt dyn_latch_cmp inp inn clk outp outn vdd vss
  m0 (tail clk vss vss) nfet_01v8 w=4u l=0.15u
  m1 (x1 inp tail vss) nfet_01v8 w={width} l=0.15u
  m2 (x2 inn tail vss) nfet_01v8 w={width} l=0.15u
  m3 (outn outp x1 vss) nfet_01v8 w=1u l=0.15u
  m4 (outp outn x2 vss) nfet_01v8 w=1u l=0.15u
  m5 (outn outp vdd vdd) pfet_01v8 w=1u l=0.15u
  m6 (outp outn vdd vdd) pfet_01v8 w=1u l=0.15u
  m7 (outn clk vdd vdd) pfet_01v8 w=0.5u l=0.15u
  m8 (outp clk vdd vdd) pfet_01v8 w=0.5u l=0.15u
ends dyn_latch_cmp
"
    )
}

const WRAP: &str = "// A StrongARM comparator made of Lee's dynamic latch comparator
subckt strongarm_comp inp inn clk outp outn vdd vss
  xlatch (inp inn clk outp outn vdd vss) dyn_latch_cmp
ends strongarm_comp
";

const WRAP2: &str = "// A StrongARM comparator made of Lee's dynamic latch comparator, with the
// inputs swapped so that outp rises when inp is the higher
subckt strongarm_comp inp inn clk outp outn vdd vss
  xlatch (inn inp clk outn outp vdd vss) dyn_latch_cmp
ends strongarm_comp
";

fn build(root: &Path, home: &Path) -> Build {
    let mut b = Build {
        root: root.to_path_buf(),
        home: home.to_path_buf(),
        date: String::new(),
        names: BTreeMap::new(),
        notes: Vec::new(),
    };

    // the workspace list: the four libraries, each by its address and by a local checkout
    let mut ws = String::from("{\"toa\": \"source/1\"}\n");
    for l in LIBS {
        ws.push_str(&format!(
            "{{\"include\": \"library.source.jsonl\", \"group\": \"{l}\", \"branch\": \"main\", \"sources\": [{{\"git\": \"{}\"}}, {{\"local\": \"{l}\"}}]}}\n",
            address(l)
        ));
    }
    write(&root.join("workspace.source.jsonl"), ws.as_bytes());

    // official, 1: an IP definition, an implementation and a testbench, sealed together
    b.new_library("official");
    b.put(
        "official",
        "ifc/strongarm-comp/ifc_draft-comp/toa.yaml",
        &format!(
            "type: ifc\nlicense: CC0-1.0\nname: strongarm-comp\ntitle: StrongARM latched comparator\n\
         description: Clocked comparator with a regenerative latch; the outputs are valid while clk is high.\n{PINS}"
        ),
    );
    b.put("official", "ifc/strongarm-comp/ifc_draft-comp/README.md",
        "# strongarm-comp\n\nA clocked comparator. While `clk` is low both outputs are reset high; when `clk` rises, the\nlatch regenerates and one output falls.\n");
    b.put("official", "ckt/strongarm-comp/sky130-razavi.g1/ckt_draft-razavi/toa.yaml",
        "type: ckt\nlicense: CC0-1.0\nifc: ifc_draft-comp\nname: sky130-razavi\nform: transistor\nspecifications: {process: sky130}\n\
         description: The classic StrongARM latch.\nfiles:\n  netlist:\n  - strongarm_comp.scs\n");
    b.put("official", "ckt/strongarm-comp/sky130-razavi.g1/ckt_draft-razavi/strongarm_comp.scs", RAZAVI);
    // the testbench g1 was sealed before B36 (no socket, no format of results, its netlist
    // declaring vdd): it is kept as it was published, and the rules of today seal only g2 (step 4)
    b.seal_only("official");
    b.put_sealed(
        "official",
        "tb/strongarm-comp/tb_offset.g1",
        "tb_draft-offset",
        &[
            ("toa.yaml", "type: tb\nlicense: CC0-1.0\nifc: $(ifc_draft-comp)\nname: tb_offset\ntitle: Input-referred offset by a slow ramp\n\
         files:\n  netlist:\n  - tb.scs\nsimulators:\n- {name: spectre, version: '21.1'}\nrun: spectre tb.scs +aps -format psfxl -raw psf\n\
         analyze: {entry: analyze.py, python: '>=3.10'}\nmetrics:\n- {id: offset, unit: mV, better: lower, tolerance: {abs: 0.2}}\n\
         parameters:\n  vdd: {unit: V, description: Supply voltage}\n".to_string()),
            ("tb.scs", TB.to_string()),
            ("analyze.py", ANALYZE.to_string()),
        ],
    );
    let first = b.step(
        "official",
        "2026-11-01T09:00:00+08:00",
        "Publish strongarm-comp, sky130-razavi.g1 and tb_offset.g1",
        false,
    );

    // official, 2: a note beside the published interface
    b.put("official", "ifc/strongarm-comp/$(ifc_draft-comp).notes/20261103-clock.md",
        "The outputs are reset high while `clk` is low, so a following stage should sample them only\nwhile `clk` is high.\n");
    b.commit("official", "2026-11-03T10:00:00+08:00", "Add a note on the clock of strongarm-comp");

    // official, 3: its first release frozen as an object, listing its members and where they are
    let member = |draft: &str, path: &str, label: &str| {
        format!(
            "{{\"object\": \"$({draft})\", \"label\": \"{label}\", \"group\": \"strongarm-comp\", \"sources\": [{{\"git\": \"{}\", \"commit\": \"{first}\", \"path\": \"{path}/$({draft})\"}}]}}\n",
            address("official")
        )
    };
    let members = String::from("{\"toa\": \"source/1\"}\n")
        + &member("ckt_draft-razavi", "ckt/strongarm-comp/sky130-razavi.g1", "strongarm-comp/sky130-razavi.g1")
        + &member("ifc_draft-comp", "ifc/strongarm-comp", "strongarm-comp")
        + &member("tb_draft-offset", "tb/strongarm-comp/tb_offset.g1", "strongarm-comp/tb_offset.g1");
    b.put(
        "official",
        "lib/official.g1/lib_draft-release/toa.yaml",
        "type: lib\nlicense: CC0-1.0\nname: official\ntitle: The official sample library, November 2026\n",
    );
    b.put("official", "lib/official.g1/lib_draft-release/members.source.jsonl", &members);
    b.put("official", "lib/official.g1/lib_draft-release/README.md",
        "# The official sample library, November 2026\n\nThe interface strongarm-comp, the circuit sky130-razavi.g1 and the testbench tb_offset.g1.\n");
    b.step("official", "2026-11-04T09:00:00+08:00", "Freeze the first release as official.g1", false);

    // lee, 1: another IP with its implementation; the library list in volumes
    b.new_library("lee");
    b.put(
        "lee",
        "ifc/dyn-latch-cmp/ifc_draft-latch/toa.yaml",
        &format!(
            "type: ifc\nlicense: CC0-1.0\nname: dyn-latch-cmp\ntitle: Dynamic latch comparator\n\
         description: A comparator of one dynamic latch stage.\n{PINS}"
        ),
    );
    b.put("lee", "ckt/dyn-latch-cmp/lee-sky130.g1/ckt_draft-lee1/toa.yaml",
        "type: ckt\nlicense: CC0-1.0\nauthor: Lee\nifc: ifc_draft-latch\nname: lee-sky130\nform: transistor\nspecifications: {process: sky130}\n\
         description: A dynamic latch comparator.\nfiles:\n  netlist:\n  - dyn_latch_cmp.scs\n");
    b.put("lee", "ckt/dyn-latch-cmp/lee-sky130.g1/ckt_draft-lee1/dyn_latch_cmp.scs", &latch("2u"));
    b.step("lee", "2026-11-05T14:00:00Z", "Publish dyn-latch-cmp and lee-sky130.g1", true);

    // lee, 2: the next version, based on the first
    b.put("lee", "ckt/dyn-latch-cmp/lee-sky130.g2/ckt_draft-lee2/toa.yaml",
        "type: ckt\nlicense: CC0-1.0\nauthor: Lee\nifc: $(ifc_draft-latch)\nbase: [$(ckt_draft-lee1)]\nname: lee-sky130\n\
         form: transistor\nspecifications: {process: sky130}\ndescription: A dynamic latch comparator with a wider input pair for less offset.\n\
         files:\n  netlist:\n  - dyn_latch_cmp.scs\n");
    b.put("lee", "ckt/dyn-latch-cmp/lee-sky130.g2/ckt_draft-lee2/dyn_latch_cmp.scs", &latch("4u"));
    b.step("lee", "2026-11-08T14:00:00Z", "Publish lee-sky130.g2", false);

    // official-chen, a fork of official: a new circuit based on official's under a name of its
    // own, so a series of its own from generation 1; a document, and a note of its own beside the
    // interface
    b.new_library("official-chen");
    let official = b.root.join("official").to_string_lossy().replace('\\', "/");
    b.git("official-chen", &["fetch", "--quiet", "--update-head-ok", &official, "main:main"]);
    b.git("official-chen", &["reset", "--quiet", "--hard", "main"]);
    b.put("official-chen", "ckt/strongarm-comp/sky130-lowkick.g1/ckt_draft-lowkick/toa.yaml",
        "type: ckt\nlicense: CC0-1.0\nauthor: Chen Wei\nifc: $(ifc_draft-comp)\nbase: [$(ckt_draft-razavi)]\nname: sky130-lowkick\n\
         form: transistor\nspecifications: {process: sky130}\ndescription: The StrongARM latch with switches that cut kickback to the inputs.\n\
         files:\n  netlist:\n  - strongarm_comp.scs\n");
    b.put("official-chen", "ckt/strongarm-comp/sky130-lowkick.g1/ckt_draft-lowkick/strongarm_comp.scs", LOWKICK);
    b.put("official-chen", "doc/strongarm-comp/kickback/doc_draft-kickback/toa.yaml",
        "type: doc\nlicense: CC0-1.0\nauthor: Chen Wei\ntitle: Less kickback in a StrongARM comparator\ntags: [design-note]\n\
         refs: [$(ifc_draft-comp), $(ckt_draft-razavi)]\ndate: '2026-11-09'\n");
    b.put("official-chen", "doc/strongarm-comp/kickback/doc_draft-kickback/README.md",
        "# Less kickback in a StrongARM comparator\n\nWhen the latch regenerates, its swing couples back to the inputs through the input pair.\nTwo switches between the pair and the latch, open while it regenerates, cut that path.\n");
    b.put(
        "official-chen",
        "ifc/strongarm-comp/$(ifc_draft-comp).notes/20261109-fork.md",
        "This fork adds sky130-lowkick.g1 and a note on kickback.\n",
    );
    b.step("official-chen", "2026-11-09T08:30:00+08:00", "Publish sky130-lowkick.g1 and the note on kickback", false);

    // alice, 1: a circuit of official's interface made of lee's, and an evidence record of it
    // with official's testbench; the library list includes the other two lists by address
    b.new_library("alice");
    b.put("alice", "library.source.jsonl", &format!(
        "{{\"toa\": \"source/1\"}}\n{{\"include\": \"library.source.jsonl\", \"group\": \"official\", \"sources\": [{{\"git\": \"{}\"}}]}}\n\
         {{\"include\": \"library.source.jsonl\", \"group\": \"lee\", \"sources\": [{{\"git\": \"{}\"}}]}}\n",
        address("official"), address("lee")));
    b.put("alice", "ckt/strongarm-comp/wrap-lee.g1/ckt_draft-wrap1/toa.yaml",
        "type: ckt\nlicense: CC0-1.0\nauthor: Alice Zhang\nifc: $(ifc_draft-comp)\nname: wrap-lee\nform: composition\n\
         description: StrongARM comparator built from Lee's dynamic latch.\n\
         uses:\n  dyn_latch_cmp:\n    ifc: $(ifc_draft-latch)\n    ckt: $(ckt_draft-lee2)\nfiles:\n  netlist:\n  - strongarm_comp.scs\n");
    b.put("alice", "ckt/strongarm-comp/wrap-lee.g1/ckt_draft-wrap1/strongarm_comp.scs", WRAP);
    b.put("alice", "evd/strongarm-comp/20261110T1200Z-wrap-off/evd_draft-off/toa.yaml",
        "type: evd\nkind: sim\nlicense: CC0-1.0\nauthor: Alice Zhang\ndut: ckt_draft-wrap1\ntbs: [$(tb_draft-offset)]\n\
         conditions:\n  parameters: {vdd: 1.8}\n  options: {temp: 27}\n  models:\n  - {file: sky130.lib.spice, section: tt}\n\
         environment:\n  simulator: {name: spectre, version: '23.1.0'}\n  analysis: {python: '3.11.9'}\n\
         metrics:\n  tb_offset.offset: 1.7\n");
    let lock = serde_json::json!({
        "toa": "lock/1",
        "dut": "ckt_draft-wrap1",
        "tbs": [{"object": b.n("tb_draft-offset"), "files": {"analyze.py": sha256(ANALYZE.as_bytes()), "tb.scs": sha256(TB.as_bytes())}}],
        "tree": [
            {"path": "", "object": "ckt_draft-wrap1", "ifc": b.n("ifc_draft-comp"), "files": {"strongarm_comp.scs": sha256(WRAP.as_bytes())}},
            {"path": "dyn_latch_cmp", "object": b.n("ckt_draft-lee2"), "ifc": b.n("ifc_draft-latch"), "files": {"dyn_latch_cmp.scs": sha256(latch("4u").as_bytes())}}
        ]
    });
    b.put(
        "alice",
        "evd/strongarm-comp/20261110T1200Z-wrap-off/evd_draft-off/lock.json",
        &(serde_json::to_string_pretty(&lock).unwrap() + "\n"),
    );
    b.step("alice", "2026-11-10T12:00:00Z", "Publish wrap-lee.g1 and its offset record", false);

    // alice, 2: the next version
    b.put("alice", "ckt/strongarm-comp/wrap-lee.g2/ckt_draft-wrap2/toa.yaml",
        "type: ckt\nlicense: CC0-1.0\nauthor: Alice Zhang\nifc: $(ifc_draft-comp)\nbase: [$(ckt_draft-wrap1)]\nname: wrap-lee\nform: composition\n\
         description: StrongARM comparator built from Lee's dynamic latch, with the inputs swapped.\n\
         uses:\n  dyn_latch_cmp:\n    ifc: $(ifc_draft-latch)\n    ckt: $(ckt_draft-lee2)\nfiles:\n  netlist:\n  - strongarm_comp.scs\n");
    b.put("alice", "ckt/strongarm-comp/wrap-lee.g2/ckt_draft-wrap2/strongarm_comp.scs", WRAP2);
    b.step("alice", "2026-11-12T12:00:00Z", "Publish wrap-lee.g2", false);

    // lee, 3: the first version removed, with a removal record left where it was (4.2)
    let v1 = b.n("ckt_draft-lee1");
    let opts = remove::Options {
        record: true,
        reason: Some("Superseded by lee-sky130.g2.".into()),
        branch: "main".into(),
        to: Some(address("lee")),
        ..Default::default()
    };
    let r = remove::remove(&b.sg(), &b.root.join("lee"), &v1, &opts).unwrap_or_else(|e| panic!("lee: removing: {e:?}"));
    assert_eq!(r.record.as_deref(), Some(format!("ckt/dyn-latch-cmp/lee-sky130.g1/{v1}.removed.json").as_str()));
    b.step("lee", "2026-11-14T14:00:00Z", "Remove lee-sky130.g1, leaving a removal record", false);

    // official, 4: the testbench under the rules of B36, a new generation of g1: the block measured
    // by its socket, the format of the results, the conditions in presets and not in the netlist
    b.put("official", "tb/strongarm-comp/tb_offset.g2/tb_draft-offset2/toa.yaml",
        "type: tb\nlicense: CC0-1.0\nifc: $(ifc_draft-comp)\nbase: [$(tb_draft-offset)]\nname: tb_offset\n\
         title: Input-referred offset by a slow ramp\nsocket: strongarm_comp\n\
         files:\n  netlist:\n  - tb.scs\nsimulators:\n- {name: spectre, version: '21.1'}\nrun: spectre input.scs +aps -format psfascii -raw psf\n\
         analyze: {entry: analyze.py, python: '>=3.10', results: psfascii}\nmetrics:\n- {id: offset, unit: mV, better: lower, tolerance: {abs: 0.2}}\n\
         parameters:\n  vdd: {unit: V, description: Supply voltage}\n\
         presets:\n  nom:\n    parameters: {vdd: 1.8}\n    options: {temp: 27}\n    models:\n    - {file: sky130.lib.spice, section: tt, process: sky130}\n\
         \x20 supply:\n  - parameters: {vdd: 1.62}\n    options: {temp: 27}\n    models:\n    - {file: sky130.lib.spice, section: tt, process: sky130}\n\
         \x20 - parameters: {vdd: 1.98}\n    options: {temp: 27}\n    models:\n    - {file: sky130.lib.spice, section: tt, process: sky130}\n");
    b.put("official", "tb/strongarm-comp/tb_offset.g2/tb_draft-offset2/tb.scs", TB2);
    b.put("official", "tb/strongarm-comp/tb_offset.g2/tb_draft-offset2/analyze.py", ANALYZE);
    b.step("official", "2026-11-24T09:00:00+08:00", "Publish tb_offset.g2, the testbench under B36", false);
    b
}

/// The files of a directory, `.git` left out: path relative to it -> bytes.
fn files(dir: &Path) -> BTreeMap<String, Vec<u8>> {
    fn walk(d: &Path, rel: &str, out: &mut BTreeMap<String, Vec<u8>>) {
        let Ok(rd) = std::fs::read_dir(d) else { return };
        for e in rd {
            let e = e.unwrap();
            let n = e.file_name().to_string_lossy().to_string();
            let r = if rel.is_empty() { n.clone() } else { format!("{rel}/{n}") };
            if e.file_type().unwrap().is_dir() {
                if n != ".git" {
                    walk(&e.path(), &r, out);
                }
            } else {
                out.insert(r, std::fs::read(e.path()).unwrap());
            }
        }
    }
    let mut out = BTreeMap::new();
    walk(dir, "", &mut out);
    out
}

#[test]
fn the_examples() {
    let home = support::temp("examples-home");
    let root = match std::env::var_os("TOA_EXAMPLES_OUT") {
        Some(d) => {
            let d = PathBuf::from(d);
            assert!(!d.exists() || std::fs::read_dir(&d).unwrap().next().is_none(), "TOA_EXAMPLES_OUT must be empty");
            std::fs::create_dir_all(&d).unwrap();
            d
        }
        None => support::temp("examples"),
    };
    let b = build(&root, &home);
    for n in &b.notes {
        eprintln!("library check: {n}");
    }

    // the index over the workspace list: every object valid, no errors
    let out = if std::env::var_os("TOA_EXAMPLES_OUT").is_some() { root.join("index") } else { home.join("index") };
    let g = support::reader(&home);
    let o = index::Options {
        workspace: root.join("workspace.source.jsonl"),
        out: out.clone(),
        cache: home.join("cache"),
        offline: true,
        tool: "toa examples".into(),
        ..Default::default()
    };
    let r = index::build(g.as_ref(), &o).unwrap();
    assert!(r.unreadable.is_empty() && !r.not_fetched, "{:?}", r.unreadable);
    assert_eq!((r.libraries, r.objects, r.valid), (4, 12, 12));
    let problems = std::fs::read_to_string(out.join("problems.jsonl")).unwrap_or_default();
    let errors: Vec<&str> = problems.lines().filter(|l| l.contains("\"level\":\"error\"")).collect();
    assert!(errors.is_empty(), "{errors:#?}");
    let object = |n: &str| -> Value {
        let p = out.join("objects").join(&n[n.find('_').unwrap() + 1..][..2]).join(format!("{n}.json"));
        serde_json::from_slice(&std::fs::read(p).unwrap()).unwrap()
    };
    // lineage through a removal record; across a fork; labels from the library lists
    let lee2 = object(&b.n("ckt_draft-lee2"));
    assert_eq!(lee2["history"]["state"], "verified", "{lee2}");
    assert_eq!(lee2["history"]["generation"], 2);
    let lowkick = object(&b.n("ckt_draft-lowkick"));
    assert_eq!(lowkick["history"]["state"], "verified", "{lowkick}");
    assert_eq!(lowkick["history"]["generation"], 1, "a name of its own starts a series");
    assert_eq!(lowkick["locations"][0]["label"], "strongarm-comp/sky130-lowkick.g1");
    let wrap1 = object(&b.n("ckt_draft-wrap1"));
    assert_eq!(wrap1["locations"][0]["label"], "strongarm-comp/wrap-lee.g1");
    // the marks toa publish gave and moved (5.2), and the newer version alice marks (7.5); the
    // fork's circuit is of another series
    assert_eq!(wrap1["locations"][0]["latest"], false);
    assert_eq!(object(&b.n("ckt_draft-wrap2"))["locations"][0]["latest"], true);
    assert_eq!(lee2["locations"][0]["latest"], true);
    assert_eq!(lowkick["locations"][0]["latest"], true);
    assert!(object(&b.n("evd_draft-off"))["locations"][0]["latest"] == false, "an evidence record has no series");
    assert_eq!(wrap1["newer"][0]["name"], b.n("ckt_draft-wrap2").as_str());
    assert_eq!(wrap1["newer"][0]["marked_by"], serde_json::json!(["alice"]));
    assert!(object(&b.n("ckt_draft-razavi"))["newer"].as_array().unwrap().is_empty());
    // the frozen library: its members are references, and its label is <name>.g<generation>
    let release = object(&b.n("lib_draft-release"));
    assert_eq!(release["locations"][0]["label"], "official.g1");
    assert_eq!(release["references"].as_array().unwrap().len(), 3);
    assert_eq!(release["references"][0]["at"], "members.source.jsonl:2/object");
    let razavi = object(&b.n("ckt_draft-razavi"));
    assert_eq!(razavi["locations"].as_array().unwrap().len(), 2, "in official and in its fork");
    let evd = object(&b.n("evd_draft-off"));
    assert_eq!(evd["locations"][0]["label"], "strongarm-comp/20261110T1200Z-wrap-off");

    // the files kept in toa/examples/
    let kept = Path::new(env!("CARGO_MANIFEST_DIR")).join("../examples");
    let writing = std::env::var_os("TOA_EXAMPLES_WRITE").is_some();
    let mut differ = Vec::new();
    for l in LIBS {
        let built = files(&root.join(l));
        let have = files(&kept.join(l));
        if built != have {
            differ.push(l.to_string());
            if writing {
                let _ = std::fs::remove_dir_all(kept.join(l));
                for (p, bytes) in &built {
                    write(&kept.join(l).join(p), bytes);
                }
            }
        }
    }
    let ws = std::fs::read(root.join("workspace.source.jsonl")).unwrap();
    if std::fs::read(kept.join("workspace.source.jsonl")).ok().as_deref() != Some(ws.as_slice()) {
        differ.push("workspace.source.jsonl".into());
        if writing {
            write(&kept.join("workspace.source.jsonl"), &ws);
        }
    }
    assert!(
        writing || differ.is_empty(),
        "toa/examples/ differs from what the tool builds now in {differ:?}; check the change, then run \
         the test with TOA_EXAMPLES_WRITE=1 to write them again"
    );
}
