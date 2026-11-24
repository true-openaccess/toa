//! Type `tb`, the testbench (`spec/types/en/tb.md`).

use std::collections::BTreeSet;

use crate::ckt;
use crate::v::{self, V};
use crate::{Lookup, Object};
use toa_core::manifest::Node;
use toa_core::Problem;

pub(crate) fn check(o: &Object, lookup: &dyn Lookup, out: &mut Vec<Problem>) {
    let mut v = V { out, section: "types/tb", file: "toa.yaml" };
    let m = o.manifest;
    let own = ["ifc", "name", "files", "external", "simulators", "run", "analyze", "metrics", "parameters", "presets"];
    if !v.fields(m, "", &crate::allowed(&own), &["ifc", "name", "files", "run", "analyze", "metrics"]) {
        return;
    }
    let ifc = m.get("ifc").and_then(|n| v.name(n, "ifc", crate::IFC));
    if let Some(n) = m.get("name") {
        v.shaped(n, "name", v::tb_name, "a testbench name: tb_[a-z0-9_]{1,36}");
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
        if v.fields(a, "analyze", &["entry", "python", "requirements"], &["entry", "python"]) {
            if let Some(n) = a.get("python") {
                v.string(n, "analyze/python");
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
    if let Some(n) = m.get("presets") {
        match n.as_map() {
            Some(map) => {
                for (k, p) in map {
                    let at = format!("presets/{}", k.text);
                    if v.fields(p, &at, &["parameters", "options", "models"], &[]) {
                        conditions(&mut v, p, &at, false);
                    }
                }
            }
            None => v.err("E_FIELD", "presets", "presets is a mapping of named sets of conditions"),
        }
    }

    // 3. when found, ifc is an interface
    if let Some(ifc) = &ifc {
        if let Some(t) = lookup.manifest(ifc).and_then(|x| x.get("type").and_then(Node::as_str).map(str::to_string)) {
            if t != crate::IFC {
                v.err("E_REF_TYPE", "ifc", format!("{ifc} is not an interface (its type is {t})"));
            }
        }
    }
    // 4. the netlists keep the rules of circuits
    ckt::netlists(v.out, "types/tb", o, m.get("files"), &listed);
}

/// The three slots of conditions (`tb` presets, `evd` conditions). `record` for an evidence
/// record: one point of conditions, so a value of `parameters` is a single value, and every model
/// file has `file`; a preset may give a list of values, a sweep.
pub(crate) fn conditions(v: &mut V, n: &Node, at: &str, record: bool) {
    for k in ["parameters", "options"] {
        let Some(x) = n.get(k) else { continue };
        let kat = v::join(at, k);
        let Some(map) = x.as_map() else {
            v.err("E_FIELD", &kat, format!("{k} is a mapping"));
            continue;
        };
        for (name, val) in map {
            let pat = format!("{kat}/{}", name.text);
            match val.as_seq() {
                Some(items) if k == "parameters" && !record && !items.is_empty() => {
                    for (i, it) in items.iter().enumerate() {
                        v.scalar(it, &format!("{pat}/{i}"));
                    }
                }
                Some(_) if k == "parameters" && record => v.err(
                    "E_FIELD",
                    &pat,
                    "an evidence record is one point of conditions: a single value, not a list (a sweep gives one record per point)",
                ),
                _ => {
                    v.scalar(val, &pat);
                }
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

    #[test]
    fn the_example_passes_and_mistakes_are_found() {
        let text = "type: tb\nlicense: CERN-OHL-P-2.0\nifc: ifc_945cdda7d9b4c0f6a2e1d3b5c7a9e0f1d2c3b4a5\nname: tb_offset\n\
                    title: Input-referred offset by a slow ramp\nfiles:\n  netlist:\n  - tb.scs\n\
                    simulators:\n- {name: spectre, version: '21.1'}\nrun: spectre tb.scs +aps\n\
                    analyze: {entry: analyze.py, python: '>=3.10', requirements: requirements.txt}\n\
                    metrics:\n- {id: offset, unit: mV, better: lower, tolerance: {abs: 0.2}}\n\
                    parameters:\n  vdd: {unit: V, description: Supply voltage}\n\
                    presets:\n  nom:\n    parameters: {vdd: 1.8, load: [1, 2]}\n    options: {temp: 27}\n    models: [{file: models.scs, section: tt}]\n";
        let m = manifest::parse(text.as_bytes()).unwrap();
        let mut files: Vec<(String, Vec<u8>)> = ["toa.yaml", "tb.scs", "analyze.py", "requirements.txt"]
            .iter()
            .map(|p| (p.to_string(), Vec::new()))
            .collect();
        let o = crate::Object { manifest: &m, files: &files, lock: None };
        let r = crate::check("tb", &o, &crate::Nothing);
        assert!(r.is_empty(), "{r:?}");

        let bad =
            text.replace("{abs: 0.2}", "{}").replace("better: lower", "better: less").replace("tb_offset", "offset");
        let m = manifest::parse(bad.as_bytes()).unwrap();
        files.pop();
        let o = crate::Object { manifest: &m, files: &files, lock: None };
        let codes: Vec<_> = crate::check("tb", &o, &crate::Nothing).iter().map(|p| p.code).collect();
        assert_eq!(codes, ["E_FIELD", "E_FILES_MISSING", "E_FIELD", "E_FIELD"], "{codes:?}");
    }
}
