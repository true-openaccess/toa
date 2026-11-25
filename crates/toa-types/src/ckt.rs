//! Type `ckt`, the circuit (`spec/types/en/ckt.md`), and what testbenches share with it:
//! files by role, external files, the rules for netlists.

use std::collections::BTreeSet;

use crate::netlist::{self, Kind};
use crate::v::{self, V};
use crate::{Lookup, Object};
use toa_core::manifest::{Node, Value};
use toa_core::Problem;

/// The roles of `files`.
pub(crate) const ROLES: &[&str] = &["netlist", "va", "rtl", "packages", "extras"];

pub(crate) fn check(o: &Object, lookup: &dyn Lookup, out: &mut Vec<Problem>) {
    let mut v = V { out, section: "types/ckt", file: "toa.yaml" };
    let m = o.manifest;
    let own = ["ifc", "name", "form", "files", "external", "uses", "parameters", "specifications", "origin"];
    if !v.fields(m, "", &crate::allowed(&own), &["ifc", "name", "form", "files"]) {
        return;
    }
    let ifc = m.get("ifc").and_then(|n| v.name(n, "ifc", crate::IFC));
    if let Some(n) = m.get("name") {
        v.shaped(n, "name", |s| v::word(s, v::lower, v::lower_digit_us_dash, 1, 39), "[a-z][a-z0-9_-]{1,39}");
    }
    let form = m.get("form").and_then(|n| {
        v.shaped(n, "form", |s| v::word(s, v::lower, v::lower_digit_us_dash, 1, 31), "[a-z][a-z0-9_-]{1,31}")
    });
    let listed = m.get("files").map(|n| files(&mut v, n, o)).unwrap_or_default();
    if let Some(n) = m.get("external") {
        external(&mut v, n);
    }
    if let Some(n) = m.get("parameters") {
        if let Some(map) = n.as_map() {
            for (k, x) in map {
                v.scalar(x, &format!("parameters/{}", k.text));
            }
        } else {
            v.err("E_FIELD", "parameters", "parameters is a mapping from names to numbers, strings or booleans");
        }
    }
    if let Some(n) = m.get("specifications") {
        specifications(&mut v, n);
    }
    // specifications.process must be written for the forms made of devices or cells of a process
    // (3.5; a mixed circuit cannot be told from its fields, so it is not checked)
    let process = m.get("specifications").and_then(|s| s.get("process"));
    if let Some(f @ ("transistor" | "passive" | "gates")) = form.as_deref() {
        if process.is_none() {
            v.err(
                "E_FIELD_MISSING",
                "specifications/process",
                format!("a circuit of the form {f} names the process of its devices or cells (3.5)"),
            );
        }
    }
    if let Some(n) = m.get("origin") {
        origin(&mut v, n);
    }
    let uses = m.get("uses").map(|n| uses(&mut v, n)).unwrap_or_default();

    // 2. when found: ifc is an interface; each use's ckt implements the use's ifc
    if let Some(ifc) = &ifc {
        if let Some(found) = lookup.manifest(ifc) {
            match found.get("type").and_then(Node::as_str) {
                Some(t) if t != crate::IFC => {
                    v.err("E_REF_TYPE", "ifc", format!("{ifc} is not an interface (its type is {t})"))
                }
                _ => ports(&mut v, o, m, ifc, &found),
            }
        }
    }
    for (key, uifc, uckt) in &uses {
        if let Some(found) = lookup.manifest(uckt) {
            let its = found.get("ifc").and_then(Node::as_str);
            if its.is_some_and(|x| x != uifc) {
                v.err(
                    "E_USES_IFC",
                    &format!("uses/{key}/ckt"),
                    format!("{uckt} implements {}, not the interface {uifc} given here", its.unwrap_or("")),
                );
            }
        }
    }
    netlists(v.out, "types/ckt", o, m.get("files"), &listed);
}

/// The ports of the subcircuit that the first netlist defines, against the pins of the interface:
/// a pin of `width` N stands for N ports (types/ifc). Checked only when the count can be told: the
/// interface's pins are well formed and the netlist's top subcircuit is plain to see.
fn ports(v: &mut V, o: &Object, m: &Node, ifc: &str, found: &Node) {
    let Some(want) = crate::ifc::pin_count(found) else { return };
    let first = m.get("files").and_then(|f| f.get("netlist")).and_then(Node::as_seq).and_then(|l| l.first());
    let Some(path) = first.and_then(Node::as_str) else { return };
    let Some(bytes) = o.bytes(path) else { return };
    let Some(top) = netlist::top_subckt(&String::from_utf8_lossy(bytes)) else { return };
    if top.ports as i64 != want {
        v.err(
            "E_PORTS",
            "files/netlist/0",
            format!(
                "{path}:{}: the subcircuit {} has {} ports, but the interface {ifc} has {want} counting a bus \
                 by its width; pins and ports are matched by position (types/ifc)",
                top.line, top.name, top.ports
            ),
        );
    }
}

/// `files` by role (`ckt`; `tb` the same): lists of paths that exist in the object, none twice;
/// `netlist` lists at least one. Returns every path listed.
pub(crate) fn files(v: &mut V, n: &Node, o: &Object) -> Vec<String> {
    if !v.fields(n, "files", ROLES, &["netlist"]) {
        return Vec::new();
    }
    let mut all = Vec::new();
    for role in ROLES {
        let Some(list) = n.get(role) else { continue };
        let min = usize::from(*role == "netlist");
        let Some(items) = v.list(list, &format!("files/{role}"), min) else { continue };
        for (i, it) in items.iter().enumerate() {
            let at = format!("files/{role}/{i}");
            let Some(p) = v.shaped(it, &at, v::file_path, "a path of a file in the object") else { continue };
            if all.contains(&p) {
                v.err("E_FILES_TWICE", &at, format!("{p} is listed twice"));
            } else if !o.has(&p) {
                v.err("E_FILES_MISSING", &at, format!("{p} is not in the object"));
            }
            all.push(p);
        }
    }
    all
}

/// `external` (3.5): the common rules are in `toa-core`; here the fields allowed and the shape of
/// `process`.
pub(crate) fn external(v: &mut V, n: &Node) {
    let Some(items) = n.as_seq() else { return };
    for (i, it) in items.iter().enumerate() {
        let at = format!("external/{i}");
        if it.as_map().is_some() {
            v.fields(it, &at, &["name", "role", "process", "file", "version", "match", "sha256"], &[]);
            if let Some(p) = it.get("process") {
                v.shaped(p, &format!("{at}/process"), v::process_name, "a short name: [a-z][a-z0-9_-]{0,39}");
            }
        }
    }
}

fn specifications(v: &mut V, n: &Node) {
    let Some(map) = n.as_map() else {
        v.err("E_FIELD", "specifications", "specifications is a mapping");
        return;
    };
    for (k, x) in map {
        let at = format!("specifications/{}", k.text);
        match k.text.as_str() {
            "process" => {
                v.shaped(x, &at, v::process_name, "a short name such as sky130: [a-z][a-z0-9_-]{0,39}");
            }
            "vdd" | "temp" => numeric(v, x, &at, false),
            "targets" => targets(v, x, &at),
            "pins" => match x.as_map() {
                Some(pins) => {
                    for (p, e) in pins {
                        numeric(v, e, &format!("{at}/{}", p.text), true);
                    }
                }
                None => v.err("E_FIELD", &at, "pins is a mapping from pin names"),
            },
            _ => numeric(v, x, &at, true),
        }
    }
}

/// `specifications.targets`: full metric names to one target or a list; a target is `{min, max,
/// preset}` with at least one of `min` and `max`.
fn targets(v: &mut V, n: &Node, at: &str) {
    let Some(map) = n.as_map() else {
        v.err("E_FIELD", at, "targets maps full metric names to a target or a list of targets");
        return;
    };
    for (k, x) in map {
        let kat = format!("{at}/{}", k.text);
        if !v::full_metric(&k.text) {
            v.err("E_FIELD", &kat, "a full metric name <testbench name>.<id>");
        }
        let items: Vec<(String, &Node)> = match x.as_seq() {
            Some([]) => {
                v.err("E_FIELD", &kat, "a list of targets has at least one");
                Vec::new()
            }
            Some(list) => list.iter().enumerate().map(|(i, t)| (format!("{kat}/{i}"), t)).collect(),
            None => vec![(kat.clone(), x)],
        };
        for (tat, t) in items {
            if !v.fields(t, &tat, &["min", "max", "preset"], &[]) {
                continue;
            }
            if t.get("min").is_none() && t.get("max").is_none() {
                v.err("E_FIELD", &tat, "a target has min, max or both");
            }
            let lo = t.get("min").and_then(|y| v.number(y, &format!("{tat}/min")));
            let hi = t.get("max").and_then(|y| v.number(y, &format!("{tat}/max")));
            if let (Some(lo), Some(hi)) = (lo, hi) {
                if lo > hi {
                    v.err("E_FIELD", &tat, "min is not above max");
                }
            }
            if let Some(p) = t.get("preset") {
                if v.string(p, &format!("{tat}/preset")).is_some_and(str::is_empty) {
                    v.err("E_FIELD", &format!("{tat}/preset"), "preset names a preset of the testbench");
                }
            }
        }
    }
}

/// A number, or a mapping of `min`, `nom`, `max`, `unit`, `description` (and, for free keys and
/// pins, also `value`, and a string or boolean in place of the number).
fn numeric(v: &mut V, n: &Node, at: &str, entry: bool) {
    match &n.value {
        Value::Int(_) | Value::Float(_) => {}
        Value::Str(_) | Value::Bool(_) if entry => {}
        Value::Map(_) => {
            let allowed: &[&str] = if entry {
                &["min", "nom", "max", "value", "unit", "description"]
            } else {
                &["min", "nom", "max", "unit", "description"]
            };
            v.fields(n, at, allowed, &[]);
            for k in ["min", "nom", "max"] {
                if let Some(x) = n.get(k) {
                    v.number(x, &format!("{at}/{k}"));
                }
            }
            if let Some(x) = n.get("value") {
                v.scalar(x, &format!("{at}/value"));
            }
            for (k, max) in [("unit", 24), ("description", 200)] {
                if let Some(x) = n.get(k) {
                    if x.as_str().is_none_or(|s| s.chars().count() > max) {
                        v.err("E_FIELD", &format!("{at}/{k}"), format!("{k} is a string of at most {max} characters"));
                    }
                }
            }
        }
        _ => v.err("E_FIELD", at, "a number, or a mapping of min, nom, max, unit and description"),
    }
}

fn origin(v: &mut V, n: &Node) {
    let req = ["repo", "commit", "path", "license", "authors"];
    let all = ["repo", "commit", "tag", "path", "license", "authors", "files"];
    if !v.fields(n, "origin", &all, &req) {
        return;
    }
    if let Some(x) = n.get("repo") {
        v.shaped(x, "origin/repo", repo, "a repository such as owner/name");
    }
    if let Some(x) = n.get("commit") {
        v.shaped(x, "origin/commit", v::hex40, "40 lowercase hexadecimal digits");
    }
    for k in ["tag", "path"] {
        if let Some(x) = n.get(k) {
            v.string(x, &format!("origin/{k}"));
        }
    }
    // the licence of the original repository: its SPDX expression and its licence file there
    if let Some(l) = n.get("license") {
        if v.fields(l, "origin/license", &["spdx", "file"], &["spdx", "file"]) {
            for k in ["spdx", "file"] {
                if let Some(x) = l.get(k) {
                    if v.string(x, &format!("origin/license/{k}")).is_some_and(str::is_empty) {
                        v.err("E_FIELD", &format!("origin/license/{k}"), format!("{k} is not empty"));
                    }
                }
            }
        }
    }
    if let Some(items) = n.get("authors").and_then(|x| v.list(x, "origin/authors", 0)) {
        for (i, a) in items.iter().enumerate() {
            v.string(a, &format!("origin/authors/{i}"));
        }
    }
    if let Some(f) = n.get("files") {
        match f.as_map() {
            Some(map) => {
                for (k, e) in map {
                    let at = format!("origin/files/{}", k.text);
                    if v.fields(e, &at, &["sha256", "size"], &["sha256", "size"]) {
                        if let Some(h) = e.get("sha256") {
                            v.shaped(h, &format!("{at}/sha256"), v::hex64, "64 lowercase hexadecimal digits");
                        }
                        if !e.get("size").is_some_and(|s| matches!(s.value, Value::Int(i) if i >= 0)) {
                            v.err("E_FIELD", &format!("{at}/size"), "size is an integer from 0");
                        }
                    }
                }
            }
            None => v.err("E_FIELD", "origin/files", "files is a mapping from paths to {sha256, size}"),
        }
    }
}

fn repo(s: &str) -> bool {
    let ok = |p: &str| !p.is_empty() && p.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '-'));
    s.split_once('/').is_some_and(|(a, b)| ok(a) && ok(b))
}

/// `uses`: `(subcircuit, ifc, ckt)` of the items that are well formed.
fn uses(v: &mut V, n: &Node) -> Vec<(String, String, String)> {
    let Some(map) = n.as_map() else {
        v.err("E_FIELD", "uses", "uses is a mapping from subcircuit names");
        return Vec::new();
    };
    let mut out = Vec::new();
    for (k, it) in map {
        let at = format!("uses/{}", k.text);
        if !v::subcircuit(&k.text) {
            v.err("E_FIELD", &at, "the key is a subcircuit name: [A-Za-z_][A-Za-z0-9_]{0,63}");
        }
        if !v.fields(it, &at, &["ifc", "ckt", "requirements"], &["ifc", "ckt"]) {
            continue;
        }
        let ifc = it.get("ifc").and_then(|x| v.name(x, &format!("{at}/ifc"), crate::IFC));
        let ckt = it.get("ckt").and_then(|x| v.name(x, &format!("{at}/ckt"), crate::CKT));
        if let Some(r) = it.get("requirements") {
            match r.as_map() {
                Some(reqs) => {
                    for (mk, b) in reqs {
                        let rat = format!("{at}/requirements/{}", mk.text);
                        if !v::full_metric(&mk.text) {
                            v.err("E_FIELD", &rat, "a full metric name <testbench name>.<id>");
                        }
                        if v.fields(b, &rat, &["min", "max"], &[]) {
                            for x in ["min", "max"] {
                                if let Some(y) = b.get(x) {
                                    v.number(y, &format!("{rat}/{x}"));
                                }
                            }
                        }
                    }
                }
                None => v.err("E_FIELD", &format!("{at}/requirements"), "requirements is a mapping"),
            }
        }
        if let (Some(ifc), Some(ckt)) = (ifc, ckt) {
            out.push((k.text.clone(), ifc, ckt));
        }
    }
    out
}

/// The rules for netlists (item 3), for every file of the roles `netlist`, `va` and `rtl`.
pub(crate) fn netlists(
    out: &mut Vec<Problem>,
    section: &'static str,
    o: &Object,
    files: Option<&Node>,
    listed: &[String],
) {
    let Some(files) = files else { return };
    let mut seen = BTreeSet::new();
    for (role, kind) in [("netlist", Kind::Netlist), ("va", Kind::Hdl), ("rtl", Kind::Hdl)] {
        for p in files.get(role).and_then(Node::as_seq).unwrap_or(&[]).iter().filter_map(Node::as_str) {
            if !seen.insert(p) {
                continue;
            }
            let Some(b) = o.bytes(p) else { continue };
            for (line, m) in netlist::check(p, &String::from_utf8_lossy(b), kind, listed) {
                out.push(Problem::error("E_NETLIST", section, format!("{p}:{line}"), m));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use toa_core::manifest;

    const IFC: &str = "ifc_945cdda7d9b4c0f6a2e1d3b5c7a9e0f1d2c3b4a5";
    const IFC2: &str = "ifc_6c2f0e9a8b7d6c5e4f3a2b1c0d9e8f7a6b5c4d3e";
    const SUB: &str = "ckt_f1ede73af6ba50ae0621e34b2b6f6b946dbf1ff1";

    struct One(Node);
    impl Lookup for One {
        fn manifest(&self, name: &str) -> Option<Node> {
            (name == SUB).then(|| self.0.clone())
        }
        fn file_sha256(&self, _: &str, _: &str) -> Option<String> {
            None
        }
    }

    fn run(text: &str, files: &[(&str, &str)], lookup: &dyn Lookup) -> Vec<(&'static str, String)> {
        let m = manifest::parse(text.as_bytes()).unwrap();
        let mut fs = vec![("toa.yaml".to_string(), text.as_bytes().to_vec())];
        fs.extend(files.iter().map(|(p, b)| (p.to_string(), b.as_bytes().to_vec())));
        crate::check("ckt", &Object { manifest: &m, files: &fs, lock: None }, lookup)
            .into_iter()
            .map(|p| (p.code, p.path))
            .collect()
    }

    #[test]
    fn the_example_passes() {
        let text = format!(
            "type: ckt\nlicense: CERN-OHL-P-2.0\nifc: {IFC}\nname: wrap-lee\nform: composition\nauthor: Alice Zhang\n\
             description: StrongARM comparator built from Lee's dynamic latch.\nuses:\n  dyn_latch_cmp:\n    ifc: {IFC2}\n    ckt: {SUB}\n\
             files:\n  netlist:\n  - strongarm_comp.scs\n\
             origin: {{repo: lee/latch, commit: {}, path: x, license: {{spdx: MIT, file: LICENSE}}, authors: [Lee]}}\n",
            "a".repeat(40)
        );
        let r = run(&text, &[("strongarm_comp.scs", "subckt x a b\nends x\n")], &crate::Nothing);
        assert!(r.is_empty(), "{r:?}");
        // the circuit used implements another interface
        let other = manifest::parse(format!("type: ckt\nifc: {IFC}\n").as_bytes()).unwrap();
        let r = run(&text, &[("strongarm_comp.scs", "")], &One(other));
        assert_eq!(r, [("E_USES_IFC", "toa.yaml:uses/dyn_latch_cmp/ckt".to_string())]);
        // the licence of the origin is {spdx, file}
        let old = text.replace("license: {spdx: MIT, file: LICENSE}", "license: MIT, license_file: LICENSE");
        let codes: Vec<&str> = run(&old, &[("strongarm_comp.scs", "")], &crate::Nothing).iter().map(|x| x.0).collect();
        assert_eq!(codes, ["E_FIELD_UNKNOWN", "E_FIELD"]);
    }

    struct Interface(Node);
    impl Lookup for Interface {
        fn manifest(&self, name: &str) -> Option<Node> {
            (name == IFC).then(|| self.0.clone())
        }
        fn file_sha256(&self, _: &str, _: &str) -> Option<String> {
            None
        }
    }

    #[test]
    fn ports_against_pins_and_the_forms_of_a_process() {
        // three pins, one a bus of 4: six ports of the subcircuit, by position
        let ifc = manifest::parse(
            b"type: ifc\nname: dac\ntitle: A DAC\npins:\n- {name: d, width: 4}\n- {name: out}\n- {name: vdd}\n",
        )
        .unwrap();
        let text = format!(
            "type: ckt\nlicense: MIT\nifc: {IFC}\nname: rs\nform: passive\nfiles:\n  netlist: [dac.sp]\n\
             specifications: {{process: sky130}}\n"
        );
        let six = ".subckt dac d<0> d<1> d<2> d<3> out vdd\nR1 out vdd 1k\n.ends\n";
        assert_eq!(run(&text, &[("dac.sp", six)], &Interface(ifc.clone())), []);
        let one_per_pin = ".subckt dac d out vdd\nR1 out vdd 1k\n.ends\n";
        assert_eq!(
            run(&text, &[("dac.sp", one_per_pin)], &Interface(ifc.clone())),
            [("E_PORTS", "toa.yaml:files/netlist/0".to_string())]
        );
        // not checked when the interface cannot be found
        assert_eq!(run(&text, &[("dac.sp", one_per_pin)], &crate::Nothing), []);
        // passive and gates name their process, as transistor does
        for form in ["passive", "gates"] {
            let bare = text
                .replace("form: passive", &format!("form: {form}"))
                .replace("specifications: {process: sky130}\n", "");
            assert_eq!(
                run(&bare, &[("dac.sp", six)], &Interface(ifc.clone())),
                [("E_FIELD_MISSING", "toa.yaml:specifications/process".to_string())]
            );
        }
    }

    #[test]
    fn targets() {
        let text = format!(
            "type: ckt\nlicense: MIT\nifc: {IFC}\nname: wrap\nform: ideal\nfiles:\n  netlist: [a.scs]\n\
             specifications:\n  targets:\n    tb_offset.offset: {{max: 2}}\n    tb_static.gain:\n    - {{min: 40, preset: nom}}\n    - {{min: 30, max: 90}}\n"
        );
        assert_eq!(run(&text, &[("a.scs", "")], &crate::Nothing), []);
        let bad = text
            .replace("{max: 2}", "{typ: 2}")
            .replace("{min: 30, max: 90}", "{min: 90, max: 30}")
            .replace("tb_static.gain", "gain");
        let codes: Vec<&str> = run(&bad, &[("a.scs", "")], &crate::Nothing).iter().map(|x| x.0).collect();
        assert_eq!(codes, ["E_FIELD_UNKNOWN", "E_FIELD", "E_FIELD", "E_FIELD"]);
    }

    #[test]
    fn mistakes_are_found() {
        let text = format!(
            "type: ckt\nlicense: MIT\nifc: {IFC}\nname: x\nform: transistor\nseries: s\nfiles:\n  netlist: [a.scs, a.scs, b.scs]\n  \
             docs: [r.md]\nspecifications: {{vdd: {{nom: 1.8, typ: 2}}, temp: hot}}\n"
        );
        let r = run(&text, &[("a.scs", ".model n nmos\n")], &crate::Nothing);
        let codes: Vec<&str> = r.iter().map(|x| x.0).collect();
        assert_eq!(
            codes,
            [
                "E_FIELD_UNKNOWN", // series
                "E_FIELD",         // name: one character
                "E_FIELD_UNKNOWN", // files/docs
                "E_FILES_TWICE",
                "E_FILES_MISSING", // b.scs
                "E_FIELD_UNKNOWN", // vdd/typ
                "E_FIELD",         // temp
                "E_FIELD_MISSING", // specifications/process for transistors
                "E_NETLIST",       // the model card
            ],
            "{r:?}"
        );
    }
}
