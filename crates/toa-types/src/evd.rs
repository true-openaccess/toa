//! Type `evd`, the evidence record (`spec/types/en/evd.md`), with its `lock.json`. The kinds `sim`
//! and `mc` are defined; a record of another kind is kept as it is.

use std::collections::{BTreeMap, BTreeSet};

use serde_json::Value as J;

use crate::tb::conditions;
use crate::v::{self, V};
use crate::{Lookup, Object};
use toa_core::manifest::{Node, Value};
use toa_core::name::{self, Form};
use toa_core::Problem;

pub(crate) fn check(o: &Object, lookup: &dyn Lookup, out: &mut Vec<Problem>) {
    let mut v = V { out, section: "types/evd", file: "toa.yaml" };
    let m = o.manifest;
    // the kind first: a kind not known here gets only the common rules and this field
    let Some(k) = m.get("kind") else {
        v.err("E_FIELD_MISSING", "kind", "kind is required: the kind of evidence, sim or mc in 0.1");
        return;
    };
    let Some(kind) =
        v.shaped(k, "kind", |s| v::word(s, v::lower, v::lower_digit_us_dash, 0, 31), "[a-z][a-z0-9_-]{0,31}")
    else {
        return;
    };
    if kind != crate::SIM && kind != crate::MC {
        return;
    }
    let mc = kind == crate::MC;
    let own = ["kind", "dut", "tbs", "conditions", "probes", "environment", "metrics", "evidence"];
    let required: &[&str] = if mc {
        &["kind", "dut", "tbs", "conditions", "environment", "metrics", "evidence"]
    } else {
        &["kind", "dut", "tbs", "conditions", "environment", "metrics"]
    };
    if !v.fields(m, "", &crate::allowed(&own), required) {
        return;
    }
    let dut = m.get("dut").and_then(|n| v.name(n, "dut", crate::CKT));
    let mut tbs = Vec::new();
    if let Some(items) = m.get("tbs").and_then(|n| v.list(n, "tbs", 1)) {
        for (i, it) in items.iter().enumerate() {
            if let Some(t) = v.name(it, &format!("tbs/{i}"), crate::TB) {
                tbs.push(t);
            }
        }
    }
    // 7. statistics, for mc only
    let mut points: Option<i64> = None;
    let mut first: i64 = 1;
    if let Some(c) = m.get("conditions") {
        if v.fields(c, "conditions", &["parameters", "options", "models", "statistics"], &[]) {
            conditions(&mut v, c, "conditions", true);
            match (c.get("statistics"), mc) {
                (Some(_), false) => v.err(
                    "E_FIELD",
                    "conditions/statistics",
                    "a record with statistics is a Monte Carlo simulation, of the kind mc",
                ),
                (None, true) => v.err(
                    "E_FIELD_MISSING",
                    "conditions/statistics",
                    "a record of the kind mc says in statistics how its samples were drawn and run",
                ),
                (Some(st), true) => {
                    points = st.get("points").and_then(|x| match x.value {
                        Value::Int(i) if i >= 1 => Some(i),
                        _ => None,
                    });
                    if let Some(Value::Int(f)) = st.get("first").map(|x| &x.value) {
                        first = *f;
                    }
                }
                (None, false) => {}
            }
        }
    }
    let mut probes: Vec<String> = Vec::new();
    if let Some(n) = m.get("probes") {
        match n.as_map() {
            Some(map) => {
                for (k, x) in map {
                    let at = format!("probes/{}", k.text);
                    if !crate::ifc::pin_name(&k.text) {
                        v.err("E_FIELD", &at, "a probe name: [a-zA-Z][a-zA-Z0-9_]{0,31}");
                    }
                    if let Some(net) = v.string(x, &at) {
                        if net.is_empty() || net.chars().count() > 200 || net.chars().any(char::is_whitespace) {
                            v.err(
                                "E_FIELD",
                                &at,
                                "a net: its name, or the instances down to it and its name joined by '.'",
                            );
                        }
                    }
                    probes.push(k.text.clone());
                }
            }
            None => v.err("E_FIELD", "probes", "probes maps probe names to nets"),
        }
    }
    if let Some(e) = m.get("environment") {
        environment(&mut v, e);
    }
    let mut metric_names = Vec::new();
    // for mc: the n and failed of each metric
    let mut counts: Vec<(String, Option<(i64, i64)>)> = Vec::new();
    if let Some(n) = m.get("metrics") {
        match n.as_map() {
            Some(map) => {
                for (k, x) in map {
                    let at = format!("metrics/{}", k.text);
                    if !v::full_metric(&k.text) {
                        v.err("E_FIELD", &at, "a full metric name <testbench name>.<id>");
                    }
                    if mc {
                        counts.push((k.text.clone(), mc_metric(&mut v, x, &at, points)));
                    } else {
                        v.number(x, &at);
                    }
                    metric_names.push(k.text.clone());
                }
            }
            None if mc => v.err(
                "E_FIELD",
                "metrics",
                "metrics is a mapping from full metric names to {n, failed, mean, sigma, min, max}",
            ),
            None => v.err("E_FIELD", "metrics", "metrics is a mapping from full metric names to numbers"),
        }
    }
    // 2. evidence is under evidence/ in the object
    if let Some(items) = m.get("evidence").and_then(|n| v.list(n, "evidence", 0)) {
        for (i, it) in items.iter().enumerate() {
            let at = format!("evidence/{i}");
            if !v.fields(it, &at, &["path", "kind", "label"], &["path", "kind"]) {
                continue;
            }
            if let Some(p) = it.get("path").and_then(|n| v.shaped(n, &format!("{at}/path"), v::file_path, "a path")) {
                if !p.starts_with("evidence/") || !o.has(&p) {
                    v.err(
                        "E_EVIDENCE",
                        &format!("{at}/path"),
                        format!("{p} is not a file under evidence/ in the object"),
                    );
                }
            }
            if let Some(k) = it.get("kind") {
                v.one_of(k, &format!("{at}/kind"), &["spectrum", "curve", "waveform", "table", "json"]);
            }
            if let Some(l) = it.get("label") {
                v.string(l, &format!("{at}/label"));
            }
        }
    }

    // when found: dut is a circuit; every testbench measures its interface; metrics are theirs
    let dut_m = dut.as_ref().and_then(|d| lookup.manifest(d));
    if let (Some(d), Some(dm)) = (&dut, &dut_m) {
        let t = dm.get("type").and_then(Node::as_str).unwrap_or("");
        if t != crate::CKT {
            v.err("E_REF_TYPE", "dut", format!("{d} is not a circuit (its type is {t})"));
        }
    }
    let dut_ifc = dut_m.as_ref().and_then(|dm| dm.get("ifc").and_then(Node::as_str).map(str::to_string));
    let mut known_metrics: Option<BTreeSet<String>> = Some(BTreeSet::new());
    // in the order of tbs and of the metrics of each testbench, when every testbench is found
    let mut ordered_metrics: Option<Vec<String>> = Some(Vec::new());
    // the probes the testbenches list, when every testbench is found
    let mut listed_probes: Option<BTreeSet<String>> = Some(BTreeSet::new());
    for (i, t) in tbs.iter().enumerate() {
        match lookup.manifest(t) {
            Some(tm) => {
                let tifc = tm.get("ifc").and_then(Node::as_str);
                if let (Some(a), Some(b)) = (tifc, &dut_ifc) {
                    if a != b {
                        v.err(
                            "E_TB_IFC",
                            &format!("tbs/{i}"),
                            format!("{t} measures {a}, not the interface of dut {b}"),
                        );
                    }
                }
                if let Some(set) = listed_probes.as_mut() {
                    for p in tm.get("probes").and_then(Node::as_seq).unwrap_or(&[]) {
                        if let Some(p) = p.as_str() {
                            set.insert(p.to_string());
                        }
                    }
                }
                let tname = tm.get("name").and_then(Node::as_str).unwrap_or("");
                for mm in tm.get("metrics").and_then(Node::as_seq).unwrap_or(&[]) {
                    if let (Some(k), Some(set)) = (mm.get("id").and_then(Node::as_str), known_metrics.as_mut()) {
                        set.insert(format!("{tname}.{k}"));
                        if let Some(o) = ordered_metrics.as_mut() {
                            o.push(format!("{tname}.{k}"));
                        }
                    }
                }
            }
            None => {
                known_metrics = None;
                listed_probes = None;
                ordered_metrics = None;
            }
        }
    }
    // 6. probes names exactly the probes the testbenches list
    if let Some(want) = listed_probes {
        let have: BTreeSet<String> = probes.iter().cloned().collect();
        for p in want.difference(&have) {
            v.err("E_EVD_PROBES", "probes", format!("the testbenches list the probe {p}, and probes gives it no net"));
        }
        for p in have.difference(&want) {
            v.err("E_EVD_PROBES", &format!("probes/{p}"), format!("{p} is a probe no testbench of tbs lists"));
        }
    }
    // 3. every key of metrics is a metric of one of the testbenches
    if let Some(known) = known_metrics {
        for k in &metric_names {
            if !known.contains(k) {
                v.err("E_METRIC_UNKNOWN", &format!("metrics/{k}"), format!("{k} is no metric of the testbenches"));
            }
        }
        // for mc, every metric of the testbenches, even one no sample gave
        if mc {
            for k in known.iter().filter(|k| !metric_names.contains(k)) {
                v.err(
                    "E_MC_METRIC",
                    "metrics",
                    format!("{k} is a metric of the testbenches: metrics holds it, with n 0 when no sample gave it"),
                );
            }
        }
    }
    // 9. samples.csv
    if mc {
        samples_file(&mut v, o, m, points, first, &counts, ordered_metrics.as_deref());
    }

    // the file lock.json
    let mut l = V { out: v.out, section: "types/evd", file: "lock.json" };
    match o.lock {
        None => l.err("E_LOCK_MISSING", "", "an evidence record of the kind sim or mc has a lock.json (types/evd)"),
        Some(lock) => check_lock(&mut l, lock, dut.as_deref(), &tbs, lookup),
    }
}

/// A metric of a record of the kind `mc` (check 8): `{n, failed, mean, sigma, min, max}`, with
/// `n + failed` equal to `points`, and the others written as `n` requires. Its `n` and `failed`.
fn mc_metric(v: &mut V, x: &Node, at: &str, points: Option<i64>) -> Option<(i64, i64)> {
    if !v.fields(x, at, &["n", "failed", "mean", "sigma", "min", "max"], &["n", "failed"]) {
        return None;
    }
    let n = x.get("n").and_then(|y| v.int(y, &v::join(at, "n"), 0, i64::MAX));
    let failed = x.get("failed").and_then(|y| v.int(y, &v::join(at, "failed"), 0, i64::MAX));
    for k in ["mean", "sigma", "min", "max"] {
        if let Some(y) = x.get(k) {
            if let Some(f) = v.number(y, &v::join(at, k)) {
                if k == "sigma" && f < 0.0 {
                    v.err("E_FIELD", &v::join(at, k), "sigma is 0 or more");
                }
            }
        }
    }
    let (n, failed) = (n?, failed?);
    if let Some(p) = points {
        if n.checked_add(failed) != Some(p) {
            v.err("E_MC_METRIC", at, format!("n + failed is {}, not points {p}", n.saturating_add(failed)));
        }
    }
    for (k, want) in [("mean", n >= 1), ("min", n >= 1), ("max", n >= 1), ("sigma", n >= 2)] {
        let has = x.get(k).is_some();
        if want && !has {
            let when = if k == "sigma" { "2 or more" } else { "1 or more" };
            v.err("E_MC_METRIC", &v::join(at, k), format!("{k} is written when n is {when}"));
        }
        if !want && has {
            v.err("E_MC_METRIC", &v::join(at, k), format!("{k} is not written when n is {n}"));
        }
    }
    Some((n, failed))
}

/// Check 9: `evidence/samples.csv` is listed as a table, keeps its format, has one line for each
/// sample from `first`, its columns are the metrics of `metrics` (in the order of the
/// testbenches when they are known), and the `n` and `failed` of each metric agree with it.
fn samples_file(
    v: &mut V,
    o: &Object,
    m: &Node,
    points: Option<i64>,
    first: i64,
    counts: &[(String, Option<(i64, i64)>)],
    ordered: Option<&[String]>,
) {
    let listed = m.get("evidence").and_then(Node::as_seq).unwrap_or(&[]).iter().any(|e| {
        e.get("path").and_then(Node::as_str) == Some(crate::mc::SAMPLES)
            && e.get("kind").and_then(Node::as_str) == Some("table")
    });
    if !listed {
        v.err("E_MC_SAMPLES", "evidence", "a record of the kind mc lists evidence/samples.csv with the kind table");
    }
    let Some(bytes) = o.bytes(crate::mc::SAMPLES) else {
        v.err("E_MC_SAMPLES", crate::mc::SAMPLES, "a record of the kind mc has evidence/samples.csv");
        return;
    };
    let s = match crate::mc::Samples::parse(bytes) {
        Ok(s) => s,
        Err(e) => {
            V { out: v.out, section: "types/evd", file: crate::mc::SAMPLES }.err("E_MC_SAMPLES", "", e);
            return;
        }
    };
    let mut names: Vec<&String> = counts.iter().map(|(k, _)| k).collect();
    let columns_ok = match ordered {
        Some(o) => s.metrics.as_slice() == o,
        None => {
            let mut cols: Vec<&String> = s.metrics.iter().collect();
            names.sort();
            cols.sort();
            names == cols
        }
    };
    let mut f = V { out: v.out, section: "types/evd", file: crate::mc::SAMPLES };
    if !columns_ok {
        f.err(
            "E_MC_SAMPLES",
            "",
            "its first line is sample and the full name of each metric, in the order of tbs and of the metrics of each testbench",
        );
        return;
    }
    if let Some(p) = points {
        let want: Vec<i64> = (first..first.saturating_add(p)).collect();
        let got: Vec<i64> = s.rows.iter().map(|r| r.0).collect();
        if got != want {
            let last = first.saturating_add(p) - 1;
            f.err("E_MC_SAMPLES", "", format!("it has one line for each sample, from {first} to {last}, in order"));
            return;
        }
    }
    for (k, c) in counts {
        let (Some((n, failed)), Some(col)) = (c, s.column(k)) else { continue };
        let have = col.iter().flatten().count() as i64;
        let miss = col.len() as i64 - have;
        if (*n, *failed) != (have, miss) {
            v.err(
                "E_MC_SAMPLES",
                &format!("metrics/{k}"),
                format!("samples.csv gives {k} in {have} samples and not in {miss}; n is {n} and failed {failed}"),
            );
        }
    }
}

fn environment(v: &mut V, e: &Node) {
    if !v.fields(e, "environment", &["simulator", "pdk", "analysis", "system", "note"], &["simulator"]) {
        return;
    }
    if let Some(s) = e.get("simulator") {
        // the whole command line, options included, is `command`; conditions.options are the
        // settings of the simulation itself
        if v.fields(s, "environment/simulator", &["name", "version", "command"], &["name", "version"]) {
            for k in ["name", "version", "command"] {
                if let Some(x) = s.get(k) {
                    v.string(x, &format!("environment/simulator/{k}"));
                }
            }
        }
    }
    if let Some(p) = e.get("pdk") {
        if v.fields(p, "environment/pdk", &["process", "release"], &[]) {
            if let Some(x) = p.get("process") {
                v.shaped(x, "environment/pdk/process", v::process_name, "a short name: [a-z][a-z0-9_-]{0,39}");
            }
            if let Some(x) = p.get("release") {
                v.string(x, "environment/pdk/release");
            }
        }
    }
    if let Some(a) = e.get("analysis") {
        if v.fields(a, "environment/analysis", &["python", "packages"], &[]) {
            if let Some(x) = a.get("python") {
                v.string(x, "environment/analysis/python");
            }
            if let Some(x) = a.get("packages") {
                match x.as_map() {
                    Some(map) => {
                        for (k, y) in map {
                            v.string(y, &format!("environment/analysis/packages/{}", k.text));
                        }
                    }
                    None => v.err("E_FIELD", "environment/analysis/packages", "packages maps names to versions"),
                }
            }
        }
    }
    if let Some(s) = e.get("system") {
        if v.fields(s, "environment/system", &["os", "cpu", "threads"], &[]) {
            for k in ["os", "cpu"] {
                if let Some(x) = s.get(k) {
                    v.string(x, &format!("environment/system/{k}"));
                }
            }
            if let Some(x) = s.get("threads") {
                if !matches!(x.value, toa_core::manifest::Value::Int(n) if n >= 1) {
                    v.err("E_FIELD", "environment/system/threads", "threads is an integer from 1");
                }
            }
        }
    }
    if let Some(x) = e.get("note") {
        v.string(x, "environment/note");
    }
}

fn is_name(s: &str, prefix: &str) -> bool {
    matches!(name::form(s), Form::Object | Form::Draft) && name::prefix_of(s) == Some(prefix)
}

/// A `path` of the tree: subcircuit names joined by `.`, or `""`.
fn tree_path(s: &str) -> bool {
    s.is_empty() || s.split('.').all(v::subcircuit)
}

/// A map of files: paths of the object to SHA-256.
fn files_map(l: &mut V, x: Option<&J>, at: &str) -> Option<BTreeMap<String, String>> {
    let Some(map) = x.and_then(J::as_object) else {
        l.err("E_LOCK", at, "files maps the paths of files to their SHA-256");
        return None;
    };
    let mut out = BTreeMap::new();
    for (p, h) in map {
        let ok = v::file_path(p) && h.as_str().is_some_and(v::hex64);
        if !ok {
            l.err("E_LOCK", &format!("{at}/{p}"), "a path of a file in the object, to 64 lowercase hexadecimal digits");
            continue;
        }
        out.insert(p.clone(), h.as_str().unwrap_or_default().to_string());
    }
    Some(out)
}

fn keys(l: &mut V, x: &J, at: &str, allowed: &[&str], required: &[&str]) -> bool {
    let Some(m) = x.as_object() else {
        l.err("E_LOCK", at, format!("{} is a JSON object", if at.is_empty() { "lock.json" } else { at }));
        return false;
    };
    for k in m.keys() {
        if !allowed.contains(&k.as_str()) {
            l.err("E_LOCK", &v::join(at, k), format!("'{k}' is not a field here"));
        }
    }
    for r in required {
        if !m.contains_key(*r) {
            l.err("E_LOCK", &v::join(at, r), format!("{r} is required"));
        }
    }
    true
}

/// The file `lock.json`: its format and its agreement with the manifest (checks at sealing,
/// item 1); its tree is the expanded tree of `dut`, with the hashes of the files, when the objects
/// can be found (item 4); each interface has one circuit in the tree (item 5).
fn check_lock(l: &mut V, lock: &J, dut: Option<&str>, tbs: &[String], lookup: &dyn Lookup) {
    if !keys(l, lock, "", &["toa", "dut", "tbs", "tree", "label"], &["toa", "dut", "tbs", "tree"]) {
        return;
    }
    if lock.get("toa").and_then(J::as_str) != Some("lock/1") {
        l.err("E_LOCK", "toa", "toa is \"lock/1\"");
    }
    if let Some(x) = lock.get("dut") {
        match x.as_str().filter(|d| is_name(d, crate::CKT)) {
            Some(d) if dut.is_some_and(|m| m != d) => {
                l.err("E_LOCK_AGREE", "dut", format!("dut is {d}, the manifest says {}", dut.unwrap_or("")))
            }
            Some(_) => {}
            None => l.err("E_LOCK", "dut", "dut is a circuit's name"),
        }
    }
    // the testbenches, in the order of the manifest, with the hashes of their files
    if let Some(items) = lock.get("tbs") {
        match items.as_array() {
            Some(a) if !a.is_empty() => {
                let names: Vec<&str> = a.iter().filter_map(|t| t.get("object").and_then(J::as_str)).collect();
                if names != tbs.iter().map(String::as_str).collect::<Vec<_>>() {
                    l.err("E_LOCK_AGREE", "tbs", "the testbenches are those of the manifest, in its order");
                }
                for (i, t) in a.iter().enumerate() {
                    let at = format!("tbs/{i}");
                    if !keys(l, t, &at, &["object", "files", "label"], &["object", "files"]) {
                        continue;
                    }
                    let obj = t.get("object").and_then(J::as_str).filter(|s| is_name(s, crate::TB));
                    if obj.is_none() {
                        l.err("E_LOCK", &format!("{at}/object"), "object is a testbench's name");
                    }
                    let files = files_map(l, t.get("files"), &format!("{at}/files"));
                    if let (Some(o), Some(f)) = (obj, files) {
                        if let Some(m) = lookup.manifest(o) {
                            hashes(l, &at, o, &tb_files(&m), &f, lookup);
                        }
                    }
                }
            }
            _ => l.err("E_LOCK", "tbs", "tbs is a list of at least one {object, files}"),
        }
    }
    // the tree, sorted by path
    let Some(tree) = lock.get("tree").and_then(J::as_array).filter(|a| !a.is_empty()) else {
        l.err("E_LOCK", "tree", "tree is a list of at least one {path, object, ifc, files}");
        return;
    };
    let mut rows: Vec<(String, String, String, BTreeMap<String, String>)> = Vec::new();
    for (i, r) in tree.iter().enumerate() {
        let at = format!("tree/{i}");
        if !keys(l, r, &at, &["path", "object", "ifc", "files", "label"], &["path", "object", "ifc", "files"]) {
            continue;
        }
        let path = r.get("path").and_then(J::as_str).filter(|p| tree_path(p));
        if path.is_none() {
            l.err("E_LOCK", &format!("{at}/path"), "path is subcircuit names joined by '.', or \"\"");
        }
        let obj = r.get("object").and_then(J::as_str).filter(|s| is_name(s, crate::CKT));
        if obj.is_none() {
            l.err("E_LOCK", &format!("{at}/object"), "object is a circuit's name");
        }
        let ip = r.get("ifc").and_then(J::as_str).filter(|s| is_name(s, crate::IFC));
        if ip.is_none() {
            l.err("E_LOCK", &format!("{at}/ifc"), "ifc is an interface's name");
        }
        let files = files_map(l, r.get("files"), &format!("{at}/files"));
        if let (Some(p), Some(o), Some(ip), Some(f)) = (path, obj, ip, files) {
            rows.push((p.to_string(), o.to_string(), ip.to_string(), f));
        }
    }
    if rows.windows(2).any(|w| w[0].0 >= w[1].0) {
        l.err("E_LOCK", "tree", "tree is sorted by path, each path once");
    }
    if let Some(first) = rows.first() {
        if first.0.is_empty() && dut.is_some_and(|d| d != first.1) {
            l.err("E_LOCK_AGREE", "tree/0", "the path \"\" is the circuit tested, dut");
        }
    }
    // 5. one circuit per interface
    let mut by_ip: BTreeMap<&str, BTreeSet<&str>> = BTreeMap::new();
    for (_, o, ip, _) in &rows {
        by_ip.entry(ip.as_str()).or_default().insert(o.as_str());
    }
    for (ip, imps) in by_ip {
        if imps.len() > 1 {
            l.err(
                "E_TREE_TWO_CIRCUITS",
                "tree",
                format!("{ip} has {} circuits in the tree; a simulation defines its subcircuit once", imps.len()),
            );
        }
    }
    // 4. the expanded tree of dut, when every object can be found, and the hashes
    let Some(d) = dut else { return };
    let Some(expected) = expand(d, lookup) else { return };
    let got: Vec<(String, String, String)> =
        rows.iter().map(|(p, o, ip, _)| (p.clone(), o.clone(), ip.clone())).collect();
    let want: Vec<(String, String, String)> =
        expected.iter().map(|(p, o, ip, _)| (p.clone(), o.clone(), ip.clone())).collect();
    if got != want {
        let show = |v: &[(String, String, String)]| {
            v.iter().map(|(p, o, _)| format!("{p:?} {o}")).collect::<Vec<_>>().join(", ")
        };
        l.err("E_LOCK_TREE", "tree", format!("tree is not the expanded tree of dut: expected {}", show(&want)));
        return;
    }
    for (i, ((_, o, _, f), (_, _, _, listed))) in rows.iter().zip(&expected).enumerate() {
        hashes(l, &format!("tree/{i}"), o, listed, f, lookup);
    }
}

/// The files of a testbench that `lock.json` hashes: `files`, the `entry` of `analyze` and its
/// `requirements`.
fn tb_files(m: &Node) -> Vec<String> {
    let mut out = listed_files(m);
    for k in ["entry", "requirements"] {
        if let Some(p) = m.get("analyze").and_then(|a| a.get(k)).and_then(Node::as_str) {
            out.push(p.to_string());
        }
    }
    out
}

/// Every path an object lists in `files`.
fn listed_files(m: &Node) -> Vec<String> {
    let mut out = Vec::new();
    for role in crate::ckt::ROLES {
        for p in m.get("files").and_then(|f| f.get(role)).and_then(Node::as_seq).unwrap_or(&[]) {
            if let Some(s) = p.as_str() {
                if !out.iter().any(|x| x == s) {
                    out.push(s.to_string());
                }
            }
        }
    }
    out
}

/// The files hashed are exactly those listed, and each hash agrees where the file can be read.
fn hashes(l: &mut V, at: &str, object: &str, listed: &[String], got: &BTreeMap<String, String>, lookup: &dyn Lookup) {
    let want: BTreeSet<&str> = listed.iter().map(String::as_str).collect();
    let have: BTreeSet<&str> = got.keys().map(String::as_str).collect();
    if want != have {
        l.err(
            "E_LOCK_FILES",
            &format!("{at}/files"),
            format!("the files of {object} are {}", want.into_iter().collect::<Vec<_>>().join(", ")),
        );
        return;
    }
    for (p, h) in got {
        if let Some(real) = lookup.file_sha256(object, p) {
            if &real != h {
                l.err("E_LOCK_HASH", &format!("{at}/files/{p}"), format!("the SHA-256 of {p} in {object} is {real}"));
            }
        }
    }
}

/// One circuit of an expanded tree: `(path, object, ifc, the files it lists)`.
pub type TreeRow = (String, String, String, Vec<String>);

/// The expanded tree of a circuit: `(path, object, ifc, files listed)`, sorted by path; `None`
/// when some object cannot be found.
pub fn expand(dut: &str, lookup: &dyn Lookup) -> Option<Vec<TreeRow>> {
    fn walk(path: String, ckt: &str, lookup: &dyn Lookup, depth: usize, out: &mut Vec<TreeRow>) -> Option<()> {
        if depth > 64 {
            return None;
        }
        let m = lookup.manifest(ckt)?;
        let ifc = m.get("ifc").and_then(Node::as_str)?.to_string();
        out.push((path.clone(), ckt.to_string(), ifc, listed_files(&m)));
        for (k, u) in m.get("uses").and_then(Node::as_map).unwrap_or(&[]) {
            let child = u.get("ckt").and_then(Node::as_str)?;
            let p = if path.is_empty() { k.text.clone() } else { format!("{path}.{}", k.text) };
            walk(p, child, lookup, depth + 1, out)?;
        }
        Some(())
    }
    let mut out = Vec::new();
    walk(String::new(), dut, lookup, 0, &mut out)?;
    out.sort_by(|a, b| a.0.cmp(&b.0));
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use toa_core::manifest;

    const IP: &str = "ifc_2aad90e74cb0ce5d0d823263d251e5440d63192e";
    const IP2: &str = "ifc_e6d0f5c82cd2942c539b7a753badead69c0fb500";
    const TOP: &str = "ckt_a2f5f0804270006822100042faf3f41af87c1a13";
    const SUB: &str = "ckt_bcff38a7e2207ba19e432697df8d87e2d13d3fb4";
    const TB: &str = "tb_127c47fa868ee215c047e7ca55900c571cd07a47";
    const H: &str = "00eb6ff40503198719d4007cdb98250c77dbf1aac08c514a0a43739c0b1482ff";

    struct Map(BTreeMap<&'static str, String>);
    impl Lookup for Map {
        fn manifest(&self, name: &str) -> Option<Node> {
            manifest::parse(self.0.get(name)?.as_bytes()).ok()
        }
        fn file_sha256(&self, _: &str, _: &str) -> Option<String> {
            Some(H.to_string())
        }
    }

    fn objects() -> Map {
        let mut m = BTreeMap::new();
        m.insert(
            TOP,
            format!(
                "type: ckt\nifc: {IP}\nuses:\n  latch:\n    ifc: {IP2}\n    ckt: {SUB}\nfiles:\n  netlist: [top.scs]\n"
            ),
        );
        m.insert(SUB, format!("type: ckt\nifc: {IP2}\nfiles:\n  netlist: [sub.scs]\n"));
        m.insert(TB, format!("type: tb\nifc: {IP}\nname: tb_offset\nfiles:\n  netlist: [tb.scs]\nanalyze: {{entry: a.py}}\nmetrics:\n- {{id: offset}}\n"));
        Map(m)
    }

    fn run(lock: serde_json::Value, metrics: &str) -> Vec<&'static str> {
        let text = format!(
            "type: evd\nlicense: MIT\nkind: sim\ndut: {TOP}\ntbs: [{TB}]\nconditions:\n  models:\n  - {{file: m.lib, section: tt}}\n\
             environment:\n  simulator: {{name: spectre, version: '23.1'}}\nmetrics:\n  {metrics}: 1.7\n"
        );
        let m = manifest::parse(text.as_bytes()).unwrap();
        let files = vec![("toa.yaml".to_string(), Vec::new()), ("lock.json".to_string(), Vec::new())];
        crate::check("evd", &Object { manifest: &m, files: &files, lock: Some(&lock) }, &objects())
            .iter()
            .map(|p| p.code)
            .collect()
    }

    fn lock(tree: serde_json::Value) -> serde_json::Value {
        serde_json::json!({"toa": "lock/1", "dut": TOP, "tbs": [{"object": TB, "files": {"tb.scs": H, "a.py": H}}], "tree": tree})
    }

    #[test]
    fn the_tree_is_the_expanded_tree_of_dut() {
        let good = serde_json::json!([
            {"path": "", "object": TOP, "ifc": IP, "files": {"top.scs": H}},
            {"path": "latch", "object": SUB, "ifc": IP2, "files": {"sub.scs": H}}
        ]);
        assert!(
            run(lock(good.clone()), "tb_offset.offset").is_empty(),
            "{:?}",
            run(lock(good.clone()), "tb_offset.offset")
        );
        // a metric no testbench has
        assert_eq!(run(lock(good), "tb_offset.gain"), ["E_METRIC_UNKNOWN"]);
        // a submodule missing from the tree
        let short = serde_json::json!([{"path": "", "object": TOP, "ifc": IP, "files": {"top.scs": H}}]);
        assert_eq!(run(lock(short), "tb_offset.offset"), ["E_LOCK_TREE"]);
        // a hash that does not agree, and a file not listed
        let wrong = serde_json::json!([
            {"path": "", "object": TOP, "ifc": IP, "files": {"top.scs": "0".repeat(64)}},
            {"path": "latch", "object": SUB, "ifc": IP2, "files": {"sub.scs": H, "extra.scs": H}}
        ]);
        assert_eq!(run(lock(wrong), "tb_offset.offset"), ["E_LOCK_HASH", "E_LOCK_FILES"]);
    }

    #[test]
    fn probes_are_those_the_testbenches_list() {
        let mut objects = objects();
        objects.0.insert(TB, format!("type: tb\nifc: {IP}\nname: tb_offset\nprobes: [latch_p, latch_n]\nfiles:\n  netlist: [tb.scs]\nanalyze: {{entry: a.py}}\nmetrics:\n- {{id: offset}}\n"));
        let good = serde_json::json!([
            {"path": "", "object": TOP, "ifc": IP, "files": {"top.scs": H}},
            {"path": "latch", "object": SUB, "ifc": IP2, "files": {"sub.scs": H}}
        ]);
        let check = |probes: &str| -> Vec<&'static str> {
            let text = format!(
                "type: evd\nlicense: MIT\nkind: sim\ndut: {TOP}\ntbs: [{TB}]\n{probes}conditions: {{}}\n\
                 environment:\n  simulator: {{name: spectre, version: '23.1'}}\nmetrics: {{}}\n"
            );
            let m = manifest::parse(text.as_bytes()).unwrap();
            let lock = lock(good.clone());
            crate::check("evd", &Object { manifest: &m, files: &[], lock: Some(&lock) }, &objects)
                .iter()
                .map(|p| p.code)
                .collect()
        };
        assert_eq!(check("probes: {latch_p: LP, latch_n: latch.LM}\n"), Vec::<&str>::new());
        assert_eq!(check("probes: {latch_p: LP, tail: 'a b'}\n"), ["E_FIELD", "E_EVD_PROBES", "E_EVD_PROBES"]);
        assert_eq!(check(""), ["E_EVD_PROBES", "E_EVD_PROBES"]);
    }

    #[test]
    fn kinds() {
        let check = |text: &str| -> Vec<&'static str> {
            let m = manifest::parse(text.as_bytes()).unwrap();
            crate::check("evd", &Object { manifest: &m, files: &[], lock: None }, &crate::Nothing)
                .iter()
                .map(|p| p.code)
                .collect()
        };
        assert_eq!(check("type: evd\nlicense: MIT\n"), ["E_FIELD_MISSING"]);
        // a kind not known here: kept as it is, its fields not checked
        assert!(check("type: evd\nlicense: MIT\nkind: drc\nrunset: x\n").is_empty());
        assert_eq!(check("type: evd\nlicense: MIT\nkind: Sim\n"), ["E_FIELD"]);
        // one point of conditions; the command line is environment.simulator.command
        let text = format!(
            "type: evd\nlicense: MIT\nkind: sim\ndut: {TOP}\ntbs: [{TB}]\nconditions:\n  parameters: {{vdd: [1.7, 1.8]}}\n\
             environment:\n  simulator: {{name: spectre, version: '23.1', options: {{temp: 27}}}}\nmetrics: {{}}\n"
        );
        assert_eq!(check(&text), ["E_FIELD", "E_FIELD_UNKNOWN", "E_LOCK_MISSING"]);
    }

    #[test]
    fn monte_carlo() {
        let mut objects = objects();
        objects.0.insert(TB, format!("type: tb\nifc: {IP}\nname: tb_offset\nfiles:\n  netlist: [tb.scs]\nanalyze: {{entry: a.py}}\nmetrics:\n- {{id: offset}}\n- {{id: ok}}\n"));
        let tree = serde_json::json!([
            {"path": "", "object": TOP, "ifc": IP, "files": {"top.scs": H}},
            {"path": "latch", "object": SUB, "ifc": IP2, "files": {"sub.scs": H}}
        ]);
        let lock = lock(tree);
        let check = |statistics: &str, metrics: &str, csv: Option<&str>| -> Vec<&'static str> {
            let text = format!(
                "type: evd\nlicense: MIT\nkind: mc\ndut: {TOP}\ntbs: [{TB}]\nconditions:\n  parameters: {{vdd: 1.8}}\n{statistics}\
                 environment:\n  simulator: {{name: spectre, version: '23.1'}}\nmetrics:\n{metrics}\
                 evidence:\n- {{path: evidence/samples.csv, kind: table}}\n"
            );
            let m = manifest::parse(text.as_bytes()).unwrap();
            let mut files = vec![("toa.yaml".to_string(), Vec::new())];
            if let Some(c) = csv {
                files.push(("evidence/samples.csv".to_string(), c.as_bytes().to_vec()));
            }
            crate::check("evd", &Object { manifest: &m, files: &files, lock: Some(&lock) }, &objects)
                .iter()
                .map(|p| p.code)
                .collect()
        };
        let st = "  statistics: {method: mismatch, points: 3, sampling: lds, seed: 12345, first: 4, run: batch}\n";
        let good =
            "  tb_offset.offset: {'n': 2, failed: 1, mean: 0.5, sigma: 0.7071067811865476, min: 0.0, max: 1.0}\n  \
                    tb_offset.ok: {'n': 1, failed: 2, mean: 1, min: 1, max: 1}\n";
        let csv = "sample,tb_offset.offset,tb_offset.ok\n4,0.0,\n5,,\n6,1.0,1\n";
        assert_eq!(check(st, good, Some(csv)), Vec::<&str>::new());
        // 7. statistics: missing, or with values out of their range
        assert_eq!(check("", good, Some(csv)), ["E_FIELD_MISSING"]);
        assert_eq!(
            check("  statistics: {method: all, points: 0, sampling: lhs, seed: 0, first: 2}\n", good, Some(csv)),
            ["E_FIELD_MISSING", "E_FIELD", "E_FIELD", "E_FIELD", "E_FIELD"]
        );
        // 8. n + failed is points; mean, min, max and sigma as n requires
        let bad = "  tb_offset.offset: {'n': 2, failed: 2, mean: 0.5, min: 0.0, max: 1.0}\n  \
                   tb_offset.ok: {'n': 0, failed: 3, mean: 1}\n";
        assert_eq!(
            check(st, bad, Some(csv)),
            ["E_MC_METRIC", "E_MC_METRIC", "E_MC_METRIC", "E_MC_SAMPLES", "E_MC_SAMPLES"]
        );
        // every metric of the testbenches
        let one = "  tb_offset.offset: {'n': 2, failed: 1, mean: 0.5, sigma: 0.7, min: 0.0, max: 1.0}\n";
        assert_eq!(
            check(st, one, Some("sample,tb_offset.offset\n4,0.0\n5,\n6,1.0\n")),
            ["E_MC_METRIC", "E_MC_SAMPLES"]
        );
        // 9. samples.csv: missing, a sample missing, n and failed not agreeing
        assert_eq!(check(st, good, None), ["E_EVIDENCE", "E_MC_SAMPLES"]);
        assert_eq!(check(st, good, Some("sample,tb_offset.offset,tb_offset.ok\n4,0.0,\n6,1.0,1\n")), ["E_MC_SAMPLES"]);
        assert_eq!(
            check(st, good, Some("sample,tb_offset.offset,tb_offset.ok\n4,0.0,\n5,2.0,\n6,1.0,1\n")),
            ["E_MC_SAMPLES"]
        );
        // a sim record has no statistics
        let text = format!(
            "type: evd\nlicense: MIT\nkind: sim\ndut: {TOP}\ntbs: [{TB}]\nconditions:\n{st}\
             environment:\n  simulator: {{name: spectre, version: '23.1'}}\nmetrics: {{}}\n"
        );
        let m = manifest::parse(text.as_bytes()).unwrap();
        let codes: Vec<&str> = crate::check("evd", &Object { manifest: &m, files: &[], lock: Some(&lock) }, &objects)
            .iter()
            .map(|p| p.code)
            .collect();
        assert_eq!(codes, ["E_FIELD"]);
    }
}
