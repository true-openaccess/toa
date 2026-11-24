//! The rules for netlists, Verilog-A and RTL in objects (`ckt`, checks at sealing, item 3; `tb`,
//! item 4): the external files stay outside the object (3.5).
//!
//! What is read: statements that include other files, model cards, and library sections, written
//! as SPICE, Spectre, Verilog-A or Verilog write them. The netlist is not parsed as a whole (that
//! is for a tool that reads RNL, item 4). Not yet checked: local paths, host names, user names and
//! strings shaped like keys elsewhere in the text.

/// What kind of file, for which statements count.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// SPICE or Spectre.
    Netlist,
    /// Verilog-A, and Verilog and other RTL: only `` `include ``.
    Hdl,
}

/// The problems of one file: line number (from 1) and what is wrong. `path` is the file's path in
/// the object, `listed` every path the manifest lists in `files`.
pub fn check(path: &str, text: &str, kind: Kind, listed: &[String]) -> Vec<(usize, String)> {
    let mut out = Vec::new();
    let dir = path.rsplit_once('/').map(|(d, _)| d).unwrap_or("");
    for (i, raw) in text.lines().enumerate() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with(['*', ';', '+']) || line.starts_with("//") {
            continue;
        }
        let mut words = line.split_whitespace();
        let first = words.next().unwrap_or("").to_ascii_lowercase();
        let rest: Vec<&str> = words.collect();
        let n = i + 1;
        let include = match kind {
            Kind::Netlist => matches!(first.as_str(), "include" | ".include" | ".inc" | "ahdl_include" | ".hdl"),
            Kind::Hdl => first == "`include",
        };
        if include {
            match rest.first().map(|t| unquote(t)) {
                Some(target) if kind == Kind::Hdl && SIMULATOR_HEADERS.contains(&target.as_str()) => {}
                Some(target) => {
                    if let Some(why) = target_problem(dir, &target, listed) {
                        out.push((n, format!("'{first} {target}': {why}")));
                    }
                }
                None => out.push((n, format!("'{first}' without a file"))),
            }
            continue;
        }
        if kind == Kind::Netlist {
            match first.as_str() {
                // `.lib <file> <section>` includes a library; `.lib <section>` opens one
                ".lib" => match rest.as_slice() {
                    [file, _section, ..] => {
                        let target = unquote(file);
                        if let Some(why) = target_problem(dir, &target, listed) {
                            out.push((n, format!("'.lib {target}': {why}")));
                        }
                    }
                    _ => {
                        out.push((n, "a library section: the models of a process stay outside the object (3.5)".into()))
                    }
                },
                ".model" | "model" => out.push((
                    n,
                    "a model card, even one without parameters: the models of a process stay outside the \
                     object (3.5), declared in specifications.process or external; the type of a device \
                     that drawing needs is written in an RNL @DEVICE record"
                        .into(),
                )),
                "section" | "library" => {
                    out.push((n, format!("a '{first}' of a model library: it stays outside the object (3.5)")))
                }
                _ => {}
            }
        }
    }
    out
}

/// A subcircuit a SPICE or Spectre netlist defines.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Subckt {
    pub name: String,
    /// The number of its ports, in the order written.
    pub ports: usize,
    /// The line of its header, from 1.
    pub line: usize,
}

/// The subcircuits a SPICE or Spectre netlist defines at its top level, and the one that defines
/// the block: the only one, or else the only one no other instantiates. `None` when no subcircuit
/// is defined or the top cannot be told. Continuation lines (`+` of SPICE, `\` of Spectre) are
/// joined; parameters (`name=value`, after `params:`) are not ports. Words are compared without
/// regard to case, as SPICE does; for Spectre this can only make a top harder to tell, never
/// wrong.
pub fn top_subckt(text: &str) -> Option<Subckt> {
    // logical lines: (first line number, text without comments)
    let mut lines: Vec<(usize, String)> = Vec::new();
    let mut open_backslash = false;
    for (i, raw) in text.lines().enumerate() {
        let mut l = raw.split("//").next().unwrap_or("").trim().to_string();
        if l.starts_with('*') {
            continue;
        }
        if let Some(rest) = l.strip_prefix('+') {
            if let Some(last) = lines.last_mut() {
                last.1.push(' ');
                last.1.push_str(rest);
                continue;
            }
            l = rest.to_string();
        }
        let cont = l.ends_with('\\');
        let body = l.trim_end_matches('\\').to_string();
        if open_backslash {
            if let Some(last) = lines.last_mut() {
                last.1.push(' ');
                last.1.push_str(&body);
            }
        } else {
            lines.push((i + 1, body));
        }
        open_backslash = cont;
    }
    let mut subs: Vec<(Subckt, std::collections::BTreeSet<String>)> = Vec::new();
    let mut depth = 0usize;
    for (n, l) in &lines {
        let spaced = l.replace(['(', ')'], " ");
        let words: Vec<String> = spaced.split_whitespace().map(str::to_ascii_lowercase).collect();
        let mut w: &[String] = &words;
        if w.first().map(String::as_str) == Some("inline") {
            w = &w[1..];
        }
        match w.first().map(String::as_str) {
            Some(".subckt" | "subckt") => {
                depth += 1;
                if depth == 1 {
                    let name = w.get(1)?;
                    let ports = w[2..]
                        .iter()
                        .take_while(|x| !x.contains('=') && !matches!(x.as_str(), "params:" | "param:"))
                        .count();
                    subs.push((Subckt { name: name.clone(), ports, line: *n }, Default::default()));
                }
            }
            Some(".ends" | "ends") => depth = depth.saturating_sub(1),
            Some(_) if depth >= 1 => {
                if let Some((_, used)) = subs.last_mut() {
                    used.extend(words.iter().cloned());
                }
            }
            _ => {}
        }
    }
    if subs.len() == 1 {
        return subs.pop().map(|(s, _)| s);
    }
    let tops: Vec<&Subckt> = subs
        .iter()
        .enumerate()
        .filter(|(i, (s, _))| !subs.iter().enumerate().any(|(j, (_, used))| j != *i && used.contains(&s.name)))
        .map(|(_, (s, _))| s)
        .collect();
    match tops.as_slice() {
        [one] => Some((*one).clone()),
        _ => None,
    }
}

/// Headers of Verilog-A that every simulator brings, included by name.
const SIMULATOR_HEADERS: &[&str] = &["disciplines.vams", "constants.vams", "discipline.h", "constants.h"];

fn unquote(t: &str) -> String {
    t.trim_matches(|c| c == '"' || c == '\'').to_string()
}

/// Why an included path is not allowed: it must be relative, stay within the object, use no
/// environment variables, and be a file listed in `files`.
fn target_problem(dir: &str, target: &str, listed: &[String]) -> Option<String> {
    if target.is_empty() {
        return Some("no file named".into());
    }
    if target.contains(['$', '~', '\\', '%']) {
        return Some("environment variables, home directories and '\\' are not used".into());
    }
    if target.starts_with('/') || target.as_bytes().get(1) == Some(&b':') {
        return Some("an absolute path: a file of the object is named by a relative path".into());
    }
    if target.split('/').any(|p| p == "..") {
        return Some("'..' is not used: the file must be in the object".into());
    }
    let joined = if dir.is_empty() { target.to_string() } else { format!("{dir}/{target}") };
    let path: Vec<&str> = joined.split('/').filter(|p| !p.is_empty() && *p != ".").collect();
    let path = path.join("/");
    if !listed.contains(&path) {
        return Some(format!("{path} is not a file listed in files"));
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_top_subcircuit_and_its_ports() {
        let spectre =
            "// a comparator\nsubckt latch (a b \\\n  q)\nends latch\nsubckt comp (inp inm clk outp outm vdd vss)\n\
                       parameters w=1u\nI0 (x y q) latch\nends comp\n";
        assert_eq!(top_subckt(spectre), Some(Subckt { name: "comp".into(), ports: 7, line: 5 }));
        let spice = "* dac\n.SUBCKT dac d0 d1 d2\n+ d3 out vdd params: w=1\nR1 out vdd 1k\n.ENDS\n";
        assert_eq!(top_subckt(spice), Some(Subckt { name: "dac".into(), ports: 6, line: 2 }));
        // two that nobody instantiates: which one is the block cannot be told
        assert_eq!(top_subckt("subckt a x\nends a\nsubckt b y\nends b\n"), None);
        assert_eq!(top_subckt("R1 a b 1k\n"), None);
    }

    #[test]
    fn includes_models_and_libraries() {
        let listed = vec!["top.scs".to_string(), "sub/b.scs".to_string(), "va/m.va".to_string()];
        let text = "// top\ninclude \"sub/b.scs\"\nahdl_include \"va/m.va\"\ninclude \"/pdk/models.scs\" section=tt\n\
                    .lib '../lib.spice' tt\n.model nch nmos level=54\nsection tt\n.include $PDK/x.sp\ninclude \"c.scs\"\n";
        let p = check("top.scs", text, Kind::Netlist, &listed);
        let lines: Vec<usize> = p.iter().map(|x| x.0).collect();
        assert_eq!(lines, [4, 5, 6, 7, 8, 9], "{p:?}");
        // relative to the including file
        assert!(check("sub/a.scs", "include \"b.scs\"\n", Kind::Netlist, &listed).is_empty());
        assert!(check("m.va", "`include \"disciplines.vams\"\n", Kind::Hdl, &listed).is_empty());
        assert_eq!(check("m.va", "`include \"/home/me/x.va\"\n", Kind::Hdl, &listed).len(), 1);
    }
}
