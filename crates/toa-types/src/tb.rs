//! Type `tb`, the testbench (`spec/types/en/tb.md`).

use std::collections::BTreeSet;

use crate::ckt;
use crate::reading::{self, Lang};
use crate::v::{self, V};
use crate::{Lookup, Object};
use toa_core::manifest::Node;
use toa_core::Problem;

/// The formats of results that `analyze.results` names.
pub const RESULTS: &[&str] = &["psfascii", "nutascii"];

pub(crate) fn check(o: &Object, lookup: &dyn Lookup, out: &mut Vec<Problem>) {
    let mut v = V { out, section: "types/tb", file: "toa.yaml" };
    let m = o.manifest;
    let own = [
        "ifc",
        "name",
        "socket",
        "probes",
        "files",
        "external",
        "simulators",
        "run",
        "analyze",
        "metrics",
        "parameters",
        "presets",
    ];
    if !v.fields(m, "", &crate::allowed(&own), &["ifc", "name", "socket", "files", "run", "analyze", "metrics"]) {
        return;
    }
    let ifc = m.get("ifc").and_then(|n| v.name(n, "ifc", crate::IFC));
    if let Some(n) = m.get("name") {
        v.shaped(n, "name", v::tb_name, "a testbench name: tb_[a-z0-9_]{1,36}");
    }
    let socket = m
        .get("socket")
        .and_then(|n| v.shaped(n, "socket", v::subcircuit, "a subcircuit name: [A-Za-z_][A-Za-z0-9_]{0,63}"));
    let mut probes: Vec<String> = Vec::new();
    if let Some(items) = m.get("probes").and_then(|n| v.list(n, "probes", 0)) {
        for (i, it) in items.iter().enumerate() {
            let at = format!("probes/{i}");
            if let Some(p) = v.shaped(it, &at, crate::ifc::pin_name, "a probe name: [a-zA-Z][a-zA-Z0-9_]{0,31}") {
                if probes.contains(&p) {
                    v.err("E_FIELD", &at, format!("the probe '{p}' is listed twice"));
                } else {
                    probes.push(p);
                }
            }
        }
    }
    let listed = m.get("files").map(|n| ckt::files(&mut v, n, o)).unwrap_or_default();
    if let Some(n) = m.get("external") {
        ckt::external(&mut v, n);
    }
    if let Some(n) = m.get("run") {
        if v.string(n, "run").is_some_and(str::is_empty) {
            v.err("E_FIELD", "run", "run is the command that runs the simulation");
        }
    }
    if let Some(a) = m.get("analyze") {
        if v.fields(a, "analyze", &["entry", "python", "requirements", "results"], &["entry", "python", "results"]) {
            if let Some(n) = a.get("python") {
                v.string(n, "analyze/python");
            }
            if let Some(n) = a.get("results") {
                v.one_of(n, "analyze/results", RESULTS);
            }
            for k in ["entry", "requirements"] {
                let Some(n) = a.get(k) else { continue };
                let at = format!("analyze/{k}");
                if let Some(p) = v.shaped(n, &at, v::file_path, "a path of a file in the object") {
                    if !o.has(&p) {
                        v.err("E_FILES_MISSING", &at, format!("{p} is not in the object"));
                    }
                }
            }
        }
    }
    if let Some(items) = m.get("metrics").and_then(|n| v.list(n, "metrics", 1)) {
        let mut ids = BTreeSet::new();
        for (i, it) in items.iter().enumerate() {
            let at = format!("metrics/{i}");
            if !v.fields(
                it,
                &at,
                &["id", "unit", "better", "tolerance", "description"],
                &["id", "unit", "better", "tolerance"],
            ) {
                continue;
            }
            if let Some(id) =
                it.get("id").and_then(|n| v.shaped(n, &format!("{at}/id"), v::metric_id, "[a-zA-Z][a-zA-Z0-9_]{0,31}"))
            {
                if !ids.insert(id.clone()) {
                    v.err("E_METRIC_TWICE", &format!("{at}/id"), format!("the metric '{id}' is defined twice"));
                }
            }
            for k in ["unit", "description"] {
                if let Some(n) = it.get(k) {
                    v.string(n, &format!("{at}/{k}"));
                }
            }
            if let Some(n) = it.get("better") {
                v.one_of(n, &format!("{at}/better"), &["lower", "higher", "none"]);
            }
            if let Some(t) = it.get("tolerance") {
                let tat = format!("{at}/tolerance");
                if v.fields(t, &tat, &["abs", "rel"], &[]) {
                    if t.get("abs").is_none() && t.get("rel").is_none() {
                        v.err("E_FIELD", &tat, "tolerance has abs, rel or both");
                    }
                    for k in ["abs", "rel"] {
                        if let Some(x) = t.get(k) {
                            if v.number(x, &format!("{tat}/{k}")).is_some_and(|x| x < 0.0) {
                                v.err("E_FIELD", &format!("{tat}/{k}"), "a tolerance is not negative");
                            }
                        }
                    }
                }
            }
        }
    }
    if let Some(items) = m.get("simulators").and_then(|n| v.list(n, "simulators", 0)) {
        for (i, it) in items.iter().enumerate() {
            let at = format!("simulators/{i}");
            if v.fields(it, &at, &["name", "version"], &["name"]) {
                for k in ["name", "version"] {
                    if let Some(n) = it.get(k) {
                        v.string(n, &format!("{at}/{k}"));
                    }
                }
            }
        }
    }
    if let Some(n) = m.get("parameters") {
        match n.as_map() {
            Some(map) => {
                for (k, p) in map {
                    let at = format!("parameters/{}", k.text);
                    if v.fields(p, &at, &["unit", "description"], &[]) {
                        for f in ["unit", "description"] {
                            if let Some(x) = p.get(f) {
                                v.string(x, &format!("{at}/{f}"));
                            }
                        }
                    }
                }
            }
            None => v.err("E_FIELD", "parameters", "parameters is a mapping of {unit, description}"),
        }
    }
    let declared = parameter_names(m);
    // 8. every set of every preset: single values, every parameter and no other
    let mut preset_options: BTreeSet<String> = BTreeSet::new();
    if let Some(n) = m.get("presets") {
        match n.as_map() {
            Some(map) => {
                for (k, p) in map {
                    let at = format!("presets/{}", k.text);
                    let sets: Vec<(String, &Node)> = match p.as_seq() {
                        Some([]) => {
                            v.err("E_FIELD", &at, "a preset is one set of conditions or a list of at least one");
                            Vec::new()
                        }
                        Some(items) => items.iter().enumerate().map(|(i, x)| (format!("{at}/{i}"), x)).collect(),
                        None => vec![(at.clone(), p)],
                    };
                    for (sat, set) in sets {
                        if !v.fields(set, &sat, &["parameters", "options", "models", "statistics"], &[]) {
                            continue;
                        }
                        conditions(&mut v, set, &sat, false);
                        let given = parameter_values(set);
                        for d in declared.iter().filter(|d| !given.contains(d)) {
                            v.err(
                                "E_PRESET",
                                &v::join(&sat, "parameters"),
                                format!("the parameter {d} is given no value: each set is written in full"),
                            );
                        }
                        for g in given.iter().filter(|g| !declared.contains(g)) {
                            v.err(
                                "E_PRESET",
                                &format!("{}/{g}", v::join(&sat, "parameters")),
                                format!("{g} is not a parameter of the testbench (parameters)"),
                            );
                        }
                        if let Some(opts) = set.get("options").and_then(Node::as_map) {
                            preset_options.extend(opts.iter().map(|(k, _)| k.text.clone()));
                        }
                    }
                }
            }
            None => v.err("E_FIELD", "presets", "presets maps names to a set of conditions or a list of sets"),
        }
    }

    // 3. when found, ifc is an interface, and every probe is one of its probes
    let mut pins: Option<i64> = None;
    if let Some(ifc) = &ifc {
        if let Some(found) = lookup.manifest(ifc) {
            let t = found.get("type").and_then(Node::as_str).unwrap_or("").to_string();
            if t != crate::IFC {
                v.err("E_REF_TYPE", "ifc", format!("{ifc} is not an interface (its type is {t})"));
            } else {
                let theirs = crate::ifc::probe_names(&found);
                for (i, p) in probes.iter().enumerate() {
                    if !theirs.contains(p) {
                        v.err(
                            "E_TB_PROBE",
                            &format!("probes/{i}"),
                            format!("{p} is not a probe of the interface {ifc}"),
                        );
                    }
                }
                pins = crate::ifc::pin_count(&found);
            }
        }
    }
    // 4. the netlists keep the rules of circuits
    ckt::netlists(v.out, "types/tb", o, m.get("files"), &listed);
    // 5. to 7.: the block measured, the probes, and what the conditions set
    if let Some(socket) = &socket {
        let want = Want { socket, probes: &probes, pins, declared: &declared, options: &preset_options };
        netlist_checks(v.out, o, m, &want);
    }
}

/// The names of `parameters`.
pub(crate) fn parameter_names(m: &Node) -> Vec<String> {
    m.get("parameters")
        .and_then(Node::as_map)
        .map(|map| map.iter().map(|(k, _)| k.text.clone()).collect())
        .unwrap_or_default()
}

/// The parameters a set of conditions gives a value to.
fn parameter_values(set: &Node) -> Vec<String> {
    set.get("parameters")
        .and_then(Node::as_map)
        .map(|map| map.iter().map(|(k, _)| k.text.clone()).collect())
        .unwrap_or_default()
}

/// The netlists of `files.netlist` that are in the object: path, language and text.
pub(crate) fn tb_netlists<'o>(o: &'o Object, m: &Node) -> Vec<(String, Lang, std::borrow::Cow<'o, str>)> {
    let mut out = Vec::new();
    for p in m.get("files").and_then(|f| f.get("netlist")).and_then(Node::as_seq).unwrap_or(&[]) {
        let Some(p) = p.as_str() else { continue };
        if let Some(b) = o.bytes(p) {
            out.push((p.to_string(), Lang::of(p), String::from_utf8_lossy(b)));
        }
    }
    out
}

/// What checks 5 to 7 look for.
struct Want<'a> {
    socket: &'a str,
    probes: &'a [String],
    pins: Option<i64>,
    declared: &'a [String],
    options: &'a BTreeSet<String>,
}

/// Checks at sealing 5, 6 and 7, on the netlists of the testbench.
fn netlist_checks(out: &mut Vec<Problem>, o: &Object, m: &Node, w: &Want) {
    let mut err = |code: &'static str, at: String, msg: String| out.push(Problem::error(code, "types/tb", at, msg));
    let socket = w.socket;
    let netlists = tb_netlists(o, m);
    let mut instances = 0;
    for (path, lang, text) in &netlists {
        let n = reading::read(text, *lang);
        // 5. socket is instantiated, never defined; each instance has a port per pin
        for s in n.subckts.iter().filter(|s| lang.same(&s.name, socket)) {
            err(
                "E_TB_SOCKET",
                format!("{path}:{}", s.line),
                format!("the netlist defines {socket}, the block measured: a testbench only instantiates it (socket)"),
            );
        }
        let mine: Vec<&reading::Instance> = n.instances.iter().filter(|i| lang.same(&i.master, socket)).collect();
        instances += mine.len();
        if let Some(want) = w.pins {
            for i in mine.iter().filter(|i| i.nodes.len() as i64 != want) {
                err(
                    "E_PORTS",
                    format!("{path}:{}", i.line),
                    format!(
                        "the instance {} of {socket} has {} ports; the interface has {want} pins, counting a bus by its width",
                        i.name,
                        i.nodes.len()
                    ),
                );
            }
        }
        // 6. what it names inside the block measured is a probe of probes
        let names: Vec<String> = mine.iter().map(|i| i.name.clone()).collect();
        for (line, inst, name) in reading::references(text, *lang, &names) {
            if !w.probes.iter().any(|p| lang.same(p, &name)) {
                err(
                    "E_TB_PROBE",
                    format!("{path}:{line}"),
                    format!(
                        "{inst}.{name} names something inside the block measured that is not in probes; a testbench \
                         names only probes of the interface"
                    ),
                );
            }
        }
        // 7. the conditions are written by the tool, not by the netlists
        for (k, line) in &n.parameters {
            if w.declared.iter().any(|d| lang.same(d, k)) {
                err(
                    "E_TB_CONDITIONS",
                    format!("{path}:{line}"),
                    format!("the netlist declares the parameter {k}: the tool writes it with the conditions of each run (7.6)"),
                );
            }
        }
        for (k, line) in &n.options {
            if w.options.iter().any(|d| lang.same(d, k)) {
                err(
                    "E_TB_CONDITIONS",
                    format!("{path}:{line}"),
                    format!(
                        "the netlist sets the option {k}, which the presets set: the tool writes it with the conditions \
                         of each run (7.6)"
                    ),
                );
            }
        }
    }
    if instances == 0 && !netlists.is_empty() {
        err(
            "E_TB_SOCKET",
            "toa.yaml:files/netlist".to_string(),
            format!("the netlists instantiate no subcircuit named {socket}, the block measured (socket)"),
        );
    }
}

/// Whether two results of a metric agree (`tb`, tolerance): `|a - b| <= max(abs, rel * max(|a|,
/// |b|))`, an `abs` or `rel` not written counting as 0. Neither result is the reference.
pub fn agree(a: f64, b: f64, abs: Option<f64>, rel: Option<f64>) -> bool {
    let bound = abs.unwrap_or(0.0).max(rel.unwrap_or(0.0) * a.abs().max(b.abs()));
    (a - b).abs() <= bound
}

/// The values of `statistics` (`evd`, the kind `mc`).
pub const METHODS: &[&str] = &["mismatch", "process", "both"];
pub const SAMPLINGS: &[&str] = &["random", "lds", "lhs"];
pub const RUNS: &[&str] = &["each", "batch"];
pub const SEED_MAX: i64 = 2147483647;

/// The fourth slot of conditions, for a Monte Carlo simulation. In a preset, `run` may be left
/// out (the user chooses); in a record it is written (`record`).
pub(crate) fn statistics(v: &mut V, n: &Node, at: &str, record: bool) {
    let required: &[&str] = if record {
        &["method", "points", "sampling", "seed", "run"]
    } else {
        &["method", "points", "sampling", "seed"]
    };
    if !v.fields(n, at, &["method", "points", "sampling", "seed", "first", "run"], required) {
        return;
    }
    if let Some(x) = n.get("method") {
        v.one_of(x, &v::join(at, "method"), METHODS);
    }
    if let Some(x) = n.get("points") {
        v.int(x, &v::join(at, "points"), 1, i64::MAX);
    }
    if let Some(x) = n.get("sampling") {
        v.one_of(x, &v::join(at, "sampling"), SAMPLINGS);
    }
    if let Some(x) = n.get("seed") {
        v.int(x, &v::join(at, "seed"), 1, SEED_MAX);
    }
    let first = n.get("first").and_then(|x| v.int(x, &v::join(at, "first"), 1, i64::MAX));
    if let Some(x) = n.get("run") {
        v.one_of(x, &v::join(at, "run"), RUNS);
    }
    if n.get("sampling").and_then(Node::as_str) == Some("lhs") && first.is_some_and(|f| f != 1) {
        v.err("E_FIELD", &v::join(at, "first"), "with lhs the samples are spread over points bins, and first is 1");
    }
}

/// The slots of conditions (`tb` presets, `evd` conditions): every value single, never a list.
/// `record` for an evidence record, where every model file has `file` and `statistics` has `run`.
pub(crate) fn conditions(v: &mut V, n: &Node, at: &str, record: bool) {
    if let Some(s) = n.get("statistics") {
        statistics(v, s, &v::join(at, "statistics"), record);
    }
    for k in ["parameters", "options"] {
        let Some(x) = n.get(k) else { continue };
        let kat = v::join(at, k);
        let Some(map) = x.as_map() else {
            v.err("E_FIELD", &kat, format!("{k} is a mapping"));
            continue;
        };
        for (name, val) in map {
            let pat = format!("{kat}/{}", name.text);
            if val.as_seq().is_some() {
                v.err(
                    "E_FIELD",
                    &pat,
                    "every value of a set of conditions is single, never a list: a sweep is a list of sets, one per point",
                );
            } else {
                v.scalar(val, &pat);
            }
        }
    }
    let Some(items) = n.get("models").and_then(|x| v.list(x, &v::join(at, "models"), 0)) else { return };
    let required: &[&str] = if record { &["file"] } else { &[] };
    for (i, it) in items.iter().enumerate() {
        let iat = format!("{}/{i}", v::join(at, "models"));
        if !v.fields(it, &iat, &["file", "section", "version", "match", "process", "sha256"], required) {
            continue;
        }
        if let Some(x) = it.get("file") {
            v.shaped(x, &format!("{iat}/file"), v::file_name, "a file name without '/' or '\\'");
        }
        if let Some(x) = it.get("section") {
            let ok = x.as_str().is_some() || x.as_seq().is_some_and(|s| s.iter().all(|y| y.as_str().is_some()));
            if !ok {
                v.err("E_FIELD", &format!("{iat}/section"), "section is a name or a list of names");
            }
        }
        if let Some(x) = it.get("version") {
            v.string(x, &format!("{iat}/version"));
        }
        if let Some(x) = it.get("match") {
            if x.as_seq().is_none_or(|s| s.iter().any(|y| y.as_str().is_none())) {
                v.err("E_FIELD", &format!("{iat}/match"), "match is a list of strings");
            }
        }
        if let Some(x) = it.get("process") {
            v.shaped(x, &format!("{iat}/process"), v::process_name, "a short name: [a-z][a-z0-9_-]{0,39}");
        }
        if let Some(x) = it.get("sha256") {
            v.shaped(x, &format!("{iat}/sha256"), v::hex64, "64 lowercase hexadecimal digits");
        }
    }
}

#[cfg(test)]
mod tests {
    use toa_core::manifest;

    const IFC: &str = "ifc_945cdda7d9b4c0f6a2e1d3b5c7a9e0f1d2c3b4a5";
    const NETLIST: &str = "simulator lang=spectre\nI0 (inp inn clk outp outn vdd 0) strongarm_comp\n\
                           V1 (vdd 0) vsource dc=vdd\ntran tran stop=10n\nsave I0.latch_p I0.latch_n\n";

    fn manifest_text() -> String {
        format!(
            "type: tb\nlicense: CERN-OHL-P-2.0\nifc: {IFC}\nname: tb_offset\n\
             title: Input-referred offset by a slow ramp\nsocket: strongarm_comp\nprobes: [latch_p, latch_n]\n\
             files:\n  netlist:\n  - tb.scs\nsimulators:\n- {{name: spectre, version: '21.1'}}\n\
             run: spectre input.scs +aps -format psfascii -raw psf\n\
             analyze: {{entry: analyze.py, python: '>=3.10', requirements: requirements.txt, results: psfascii}}\n\
             metrics:\n- {{id: offset, unit: mV, better: lower, tolerance: {{abs: 0.2}}}}\n\
             parameters:\n  vdd: {{unit: V, description: Supply voltage}}\n\
             presets:\n  nom:\n    parameters: {{vdd: 1.8}}\n    options: {{temp: 27}}\n    models: [{{file: models.scs, section: tt}}]\n  \
             supply:\n  - parameters: {{vdd: 1.62}}\n    options: {{temp: 27}}\n    models: [{{file: models.scs, section: tt}}]\n  \
             - parameters: {{vdd: 1.98}}\n    options: {{temp: 27}}\n    models: [{{file: models.scs, section: tt}}]\n"
        )
    }

    struct Interface;
    impl crate::Lookup for Interface {
        fn manifest(&self, name: &str) -> Option<manifest::Node> {
            let text =
                "type: ifc\nname: strongarm-comp\ntitle: X\npins:\n- {name: inp}\n- {name: inn}\n- {name: clk}\n\
                        - {name: outp}\n- {name: outn}\n- {name: vdd}\n- {name: vss}\n\
                        probes:\n- {name: latch_p, description: Positive}\n- {name: latch_n, description: Negative}\n";
            (name == IFC).then(|| manifest::parse(text.as_bytes()).unwrap())
        }
        fn file_sha256(&self, _: &str, _: &str) -> Option<String> {
            None
        }
    }

    fn codes(text: &str, netlist: &str, lookup: &dyn crate::Lookup) -> Vec<(&'static str, String)> {
        let m = manifest::parse(text.as_bytes()).unwrap();
        let files: Vec<(String, Vec<u8>)> =
            [("toa.yaml", ""), ("tb.scs", netlist), ("analyze.py", ""), ("requirements.txt", "")]
                .iter()
                .map(|(p, b)| (p.to_string(), b.as_bytes().to_vec()))
                .collect();
        let o = crate::Object { manifest: &m, files: &files, lock: None };
        crate::check("tb", &o, lookup).into_iter().map(|p| (p.code, p.path)).collect()
    }

    #[test]
    fn the_example_passes() {
        assert_eq!(codes(&manifest_text(), NETLIST, &Interface), []);
        assert_eq!(codes(&manifest_text(), NETLIST, &crate::Nothing), []);
    }

    #[test]
    fn fields_and_presets() {
        let bad = manifest_text()
            .replace("{abs: 0.2}", "{}")
            .replace("better: lower", "better: less")
            .replace(", results: psfascii", ", results: psf")
            .replace("socket: strongarm_comp\n", "")
            .replace("parameters: {vdd: 1.8}", "parameters: {vdd: [1.7, 1.8]}")
            .replace("parameters: {vdd: 1.98}", "parameters: {vdd: 1.98, load: 2}")
            .replace("parameters: {vdd: 1.62}", "parameters: {}");
        let got: Vec<(&str, String)> = codes(&bad, NETLIST, &crate::Nothing);
        let got: Vec<(&str, &str)> = got.iter().map(|(c, p)| (*c, p.as_str())).collect();
        assert_eq!(
            got,
            [
                ("E_FIELD_MISSING", "toa.yaml:socket"),
                ("E_FIELD", "toa.yaml:analyze/results"),
                ("E_FIELD", "toa.yaml:metrics/0/better"),
                ("E_FIELD", "toa.yaml:metrics/0/tolerance"),
                ("E_FIELD", "toa.yaml:presets/nom/parameters/vdd"),
                ("E_PRESET", "toa.yaml:presets/supply/0/parameters"),
                ("E_PRESET", "toa.yaml:presets/supply/1/parameters/load"),
            ]
        );
    }

    #[test]
    fn statistics_in_a_preset() {
        let mc = |st: &str| -> Vec<(&'static str, String)> {
            let text = manifest_text().replace(
                "  supply:\n",
                &format!(
                    "  mc:\n    parameters: {{vdd: 1.8}}\n    options: {{temp: 27}}\n    models: [{{file: models.scs, section: tt}}]\n    statistics: {st}\n  supply:\n"
                ),
            );
            codes(&text, NETLIST, &crate::Nothing)
        };
        // run may be left out in a preset: the user chooses
        assert_eq!(mc("{method: mismatch, points: 200, sampling: lds, seed: 12345}"), []);
        assert_eq!(mc("{method: both, points: 20, sampling: lhs, seed: 1, first: 1, run: each}"), []);
        let got = mc("{method: process, points: 20, sampling: lhs, seed: 2147483648, first: 3, run: some}");
        let got: Vec<(&str, &str)> = got.iter().map(|(c, p)| (*c, p.as_str())).collect();
        assert_eq!(
            got,
            [
                ("E_FIELD", "toa.yaml:presets/mc/statistics/seed"),
                ("E_FIELD", "toa.yaml:presets/mc/statistics/run"),
                ("E_FIELD", "toa.yaml:presets/mc/statistics/first"),
            ]
        );
        let got = mc("{points: 1.5, sampling: lds, seed: 1, extra: 1}");
        let got: Vec<&str> = got.iter().map(|(c, _)| *c).collect();
        assert_eq!(got, ["E_FIELD_UNKNOWN", "E_FIELD_MISSING", "E_FIELD"]);
    }

    #[test]
    fn the_block_measured_probes_and_conditions() {
        let text = manifest_text();
        // defined here, instantiated with a port too few, a name inside that is no probe, the
        // parameter declared and an option of the presets set
        let netlist = "subckt strongarm_comp (a b)\nends strongarm_comp\nparameters vdd=1.8\n\
                       I0 (inp inn clk outp outn vdd) strongarm_comp\nsave I0.latch_p I0.LP\n\
                       opts options temp=27 reltol=1e-4\n";
        let got: Vec<(&str, String)> = codes(&text, netlist, &Interface);
        let got: Vec<(&str, &str)> = got.iter().map(|(c, p)| (*c, p.as_str())).collect();
        assert_eq!(
            got,
            [
                ("E_TB_SOCKET", "tb.scs:1"),
                ("E_PORTS", "tb.scs:4"),
                ("E_TB_PROBE", "tb.scs:5"),
                ("E_TB_CONDITIONS", "tb.scs:3"),
                ("E_TB_CONDITIONS", "tb.scs:6"),
            ]
        );
        // a probe the interface does not have; no instance of socket at all
        let other = text.replace("probes: [latch_p, latch_n]", "probes: [latch_p, tail]");
        let got: Vec<&str> = codes(&other, "R1 (a b) resistor r=1\n", &Interface).iter().map(|x| x.0).collect();
        assert_eq!(got, ["E_TB_PROBE", "E_TB_SOCKET"]);
    }

    #[test]
    fn tolerance() {
        assert!(super::agree(1.0, 1.0625, Some(0.0625), None));
        assert!(!super::agree(1.0, 1.125, Some(0.0625), None));
        // relative to the larger of the two, either way round
        assert!(super::agree(100.0, 102.0, None, Some(0.02)) && super::agree(102.0, 100.0, None, Some(0.02)));
        assert!(!super::agree(100.0, 103.0, None, Some(0.02)));
        // both written: the larger bound
        assert!(super::agree(0.0, 0.05, Some(0.1), Some(0.01)));
        assert!(super::agree(1.0, 1.0, None, None) && !super::agree(1.0, 1.0 + 1e-12, None, None));
    }
}
