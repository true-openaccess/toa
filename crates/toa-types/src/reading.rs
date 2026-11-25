//! What a SPICE or Spectre netlist says, as far as assembling a run (7.6) and the checks of
//! testbenches (`tb`, checks at sealing, items 5 to 7) need it: the subcircuits it defines with
//! their ports and nets, the instances in it with their masters and nodes, the parameters it
//! declares and the options it sets at its top level. And the two changes assembling makes to the
//! copies of netlists: renaming subcircuits, and writing nets in place of probes.
//!
//! This is not a parser of either language. It reads statements as they are written in practice
//! (the netlists Virtuoso, ngspice and people write) and says nothing where it cannot tell; the
//! callers then stop rather than guess (7.6: "when the tool cannot tell, it stops").
//!
//! - Comments: a line starting with `*`; in Spectre, `//` to the end of the line; in SPICE, `;`
//!   and `$ ` to the end of the line.
//! - Continuation: a line ending in `\` goes on in the next; a line starting with `+` goes on the
//!   one before (Spectre reads both).
//! - Names are compared as written in Spectre and without regard to case in SPICE.

use std::collections::{BTreeMap, BTreeSet};

/// The language of a netlist, by its name (7.6): `.scs` is Spectre, any other SPICE.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lang {
    Spectre,
    Spice,
}

impl Lang {
    pub fn of(path: &str) -> Lang {
        if path.to_ascii_lowercase().ends_with(".scs") {
            Lang::Spectre
        } else {
            Lang::Spice
        }
    }

    /// Whether two names are the same name in this language.
    pub fn same(self, a: &str, b: &str) -> bool {
        match self {
            Lang::Spectre => a == b,
            Lang::Spice => a.eq_ignore_ascii_case(b),
        }
    }

    /// The form of a name used to compare it.
    pub fn key(self, a: &str) -> String {
        match self {
            Lang::Spectre => a.to_string(),
            Lang::Spice => a.to_ascii_lowercase(),
        }
    }
}

/// A subcircuit defined in a netlist.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Subckt {
    /// As written.
    pub name: String,
    pub ports: Vec<String>,
    /// The line of its header, from 1.
    pub line: usize,
    /// Its ports and the nodes of the instances directly in it.
    pub nets: BTreeSet<String>,
    /// The subcircuit it is defined in, for a nested definition.
    pub parent: Option<String>,
}

/// An instance: a statement with a name, nodes and a master (a subcircuit, a primitive, a
/// model, or in Spectre an analysis such as `pss`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Instance {
    pub name: String,
    pub master: String,
    pub nodes: Vec<String>,
    pub line: usize,
    /// The subcircuit it is in; `None` at the top level.
    pub parent: Option<String>,
}

/// What one netlist says.
#[derive(Debug, Clone, Default)]
pub struct Netlist {
    pub subckts: Vec<Subckt>,
    pub instances: Vec<Instance>,
    /// Parameters declared at the top level (`parameters a=1` in Spectre, `.param a=1` in
    /// SPICE): (name, line).
    pub parameters: Vec<(String, usize)>,
    /// Options set at the top level (`<name> options a=1` in Spectre, `.options a=1` and
    /// `.temp` in SPICE): (name, line).
    pub options: Vec<(String, usize)>,
}

/// One token of a logical line, and whether it is inside parentheses.
#[derive(Debug, Clone)]
struct Tok {
    text: String,
    paren: bool,
}

/// The code of one physical line: comments taken out.
fn code(raw: &str, lang: Lang) -> &str {
    let t = raw.trim_start();
    if t.starts_with('*') {
        return "";
    }
    let mut end = raw.len();
    if let Some(i) = raw.find("//") {
        end = end.min(i);
    }
    if lang == Lang::Spice {
        if let Some(i) = raw.find(';') {
            end = end.min(i);
        }
        if let Some(i) = raw.find("$ ") {
            end = end.min(i);
        }
    }
    &raw[..end]
}

/// Logical lines: (first line number, code).
pub fn logical(text: &str, lang: Lang) -> Vec<(usize, String)> {
    let mut out: Vec<(usize, String)> = Vec::new();
    let mut open = false;
    for (i, raw) in text.lines().enumerate() {
        let c = code(raw, lang).trim();
        if c.is_empty() {
            // a blank or comment line does not end a `\` continuation in practice; keep it open
            continue;
        }
        if let Some(rest) = c.strip_prefix('+') {
            if let Some(last) = out.last_mut() {
                last.1.push(' ');
                last.1.push_str(rest.trim_end_matches('\\'));
                open = rest.ends_with('\\');
                continue;
            }
        }
        let cont = c.ends_with('\\');
        let body = c.trim_end_matches('\\');
        if open {
            if let Some(last) = out.last_mut() {
                last.1.push(' ');
                last.1.push_str(body);
            }
        } else {
            out.push((i + 1, body.to_string()));
        }
        open = cont;
    }
    out
}

fn tokens(s: &str) -> Vec<Tok> {
    let mut out = Vec::new();
    let mut depth = 0usize;
    let mut cur = String::new();
    let mut quote: Option<char> = None;
    let flush = |cur: &mut String, out: &mut Vec<Tok>, depth: usize| {
        if !cur.is_empty() {
            out.push(Tok { text: std::mem::take(cur), paren: depth > 0 });
        }
    };
    for c in s.chars() {
        if let Some(q) = quote {
            cur.push(c);
            if c == q {
                quote = None;
            }
            continue;
        }
        match c {
            '"' | '\'' => {
                quote = Some(c);
                cur.push(c);
            }
            '(' => {
                flush(&mut cur, &mut out, depth);
                depth += 1;
            }
            ')' => {
                flush(&mut cur, &mut out, depth);
                depth = depth.saturating_sub(1);
            }
            c if c.is_whitespace() => flush(&mut cur, &mut out, depth),
            _ => cur.push(c),
        }
    }
    flush(&mut cur, &mut out, depth);
    // `a = b` written with spaces: one token `a=b`
    let mut joined: Vec<Tok> = Vec::new();
    let mut i = 0;
    while i < out.len() {
        if out[i].text == "=" && !joined.is_empty() && i + 1 < out.len() {
            let last = joined.last_mut().expect("not empty");
            last.text.push('=');
            last.text.push_str(&out[i + 1].text);
            i += 2;
            continue;
        }
        joined.push(out[i].clone());
        i += 1;
    }
    joined
}

fn assignment(t: &str) -> bool {
    t.contains('=')
}

/// Words that start statements other than instances, in either language.
const KEYWORDS: &[&str] = &[
    "subckt",
    "ends",
    "inline",
    "parameters",
    "include",
    "ahdl_include",
    "simulator",
    "model",
    "section",
    "endsection",
    "library",
    "endlibrary",
    "global",
    "save",
    "ic",
    "nodeset",
    "statistics",
    "real",
    "function",
    "paramset",
    "export",
    "if",
    "else",
];

/// The number of nodes of a SPICE primitive, by its first letter; `None` for a subcircuit call
/// (`X`) or a letter not known here.
fn spice_nodes(first: char) -> Option<usize> {
    match first.to_ascii_lowercase() {
        'r' | 'c' | 'l' | 'd' | 'v' | 'i' | 'f' | 'h' | 'b' | 'w' => Some(2),
        'q' | 'j' | 'z' => Some(3),
        'm' | 'e' | 'g' | 't' | 's' | 'o' | 'u' => Some(4),
        'k' => Some(0),
        _ => None,
    }
}

/// Read a netlist.
pub fn read(text: &str, lang: Lang) -> Netlist {
    let mut out = Netlist::default();
    // the subcircuits being defined, innermost last (index into out.subckts)
    let mut stack: Vec<usize> = Vec::new();
    for (line, l) in logical(text, lang) {
        let mut toks = tokens(&l);
        if toks.first().is_some_and(|t| t.text.eq_ignore_ascii_case("inline")) {
            toks.remove(0);
        }
        let Some(first) = toks.first().map(|t| t.text.clone()) else { continue };
        let low = first.to_ascii_lowercase();
        let parent = stack.last().map(|&i| out.subckts[i].name.clone());
        match low.as_str() {
            "subckt" | ".subckt" | "macromodel" => {
                let Some(name) = toks.get(1).map(|t| t.text.clone()) else { continue };
                let ports: Vec<String> = toks[2..]
                    .iter()
                    .take_while(|t| !assignment(&t.text) && !t.text.eq_ignore_ascii_case("params:"))
                    .map(|t| t.text.clone())
                    .collect();
                let nets = ports.iter().cloned().collect();
                out.subckts.push(Subckt { name, ports, line, nets, parent });
                stack.push(out.subckts.len() - 1);
                continue;
            }
            "ends" | ".ends" => {
                stack.pop();
                continue;
            }
            "parameters" | ".param" | ".params" if stack.is_empty() => {
                for t in &toks[1..] {
                    if let Some((k, _)) = t.text.split_once('=') {
                        out.parameters.push((k.trim().to_string(), line));
                    }
                }
                continue;
            }
            ".options" | ".option" | ".opt" | ".opts" if stack.is_empty() => {
                for t in &toks[1..] {
                    let k = t.text.split_once('=').map_or(t.text.as_str(), |(k, _)| k);
                    out.options.push((k.trim().to_string(), line));
                }
                continue;
            }
            ".temp" if stack.is_empty() => {
                out.options.push(("temp".into(), line));
                continue;
            }
            _ => {}
        }
        if low.starts_with('.') || KEYWORDS.contains(&low.as_str()) {
            continue;
        }
        // Spectre `<name> options a=1 …`
        if lang == Lang::Spectre && toks.get(1).is_some_and(|t| !t.paren && t.text == "options") {
            if stack.is_empty() {
                for t in &toks[2..] {
                    if let Some((k, _)) = t.text.split_once('=') {
                        out.options.push((k.trim().to_string(), line));
                    }
                }
            }
            continue;
        }
        let rest = &toks[1..];
        let (nodes, master): (Vec<String>, Option<String>) = if rest.first().is_some_and(|t| t.paren) {
            // `name (n1 n2) master …`
            let nodes: Vec<String> = rest.iter().take_while(|t| t.paren).map(|t| t.text.clone()).collect();
            let master = rest.iter().find(|t| !t.paren).map(|t| t.text.clone()).filter(|m| !assignment(m));
            (nodes, master)
        } else {
            let plain: Vec<String> = rest
                .iter()
                .take_while(|t| !assignment(&t.text) && !t.text.eq_ignore_ascii_case("params:"))
                .map(|t| t.text.clone())
                .collect();
            let c = first.chars().next().unwrap_or('x');
            match (lang, spice_nodes(c)) {
                (Lang::Spice, Some(n)) => {
                    let nodes: Vec<String> = plain.iter().take(n).cloned().collect();
                    (nodes, plain.get(n).cloned())
                }
                _ => match plain.split_last() {
                    Some((m, nodes)) => (nodes.to_vec(), Some(m.clone())),
                    None => (Vec::new(), None),
                },
            }
        };
        let Some(master) = master else { continue };
        if let Some(&i) = stack.last() {
            out.subckts[i].nets.extend(nodes.iter().cloned());
        }
        out.instances.push(Instance { name: first, master, nodes, line, parent });
    }
    out
}

/// The separators of tokens for renaming and probes: white space, parentheses, `=`, `,`,
/// quotes, brackets, braces and `:`.
fn separator(c: char) -> bool {
    c.is_whitespace() || "()=,\"'[]{}:".contains(c)
}

/// Rewrite the code of every line, token by token, leaving comments, white space and every other
/// byte as they are. `f(token, first)` gives the replacement of a token, `first` being whether it
/// is the first token of a statement (an instance's own name).
fn rewrite(text: &str, lang: Lang, f: &dyn Fn(&str, bool) -> Option<String>) -> String {
    let mut out = String::with_capacity(text.len());
    let mut open = false;
    for raw in text.split_inclusive('\n') {
        let (body, nl) = match raw.strip_suffix('\n') {
            Some(b) => match b.strip_suffix('\r') {
                Some(b2) => (b2, "\r\n"),
                None => (b, "\n"),
            },
            None => (raw, ""),
        };
        let c = code(body, lang);
        let trimmed = c.trim();
        let continuation = open || trimmed.starts_with('+');
        if !trimmed.is_empty() {
            open = trimmed.ends_with('\\');
        }
        let mut first = !continuation;
        let mut i = 0;
        let bytes: Vec<char> = c.chars().collect();
        let mut new = String::new();
        while i < bytes.len() {
            if separator(bytes[i]) || (bytes[i] == '+' && new.trim().is_empty()) {
                new.push(bytes[i]);
                i += 1;
                continue;
            }
            let start = i;
            while i < bytes.len() && !separator(bytes[i]) {
                i += 1;
            }
            let tok: String = bytes[start..i].iter().collect();
            match f(&tok, first) {
                Some(r) => new.push_str(&r),
                None => new.push_str(&tok),
            }
            first = false;
        }
        out.push_str(&new);
        out.push_str(&body[c.len()..]);
        out.push_str(nl);
    }
    out
}

/// Rename subcircuits: every token equal to a key of `map` (by the language's comparison) becomes
/// its value, except the first token of a statement (an instance's own name) and tokens with
/// `.` in them (references down the hierarchy).
pub fn rename(text: &str, lang: Lang, map: &BTreeMap<String, String>) -> String {
    if map.is_empty() {
        return text.to_string();
    }
    let keyed: BTreeMap<String, &String> = map.iter().map(|(k, v)| (lang.key(k), v)).collect();
    rewrite(text, lang, &|tok, first| {
        if first {
            return None;
        }
        keyed.get(&lang.key(tok)).map(|v| (*v).clone())
    })
}

/// The tokens `<instance>.<name>` of a netlist where `<instance>` is one of `instances`: (line,
/// instance, name).
pub fn references(text: &str, lang: Lang, instances: &[String]) -> Vec<(usize, String, String)> {
    let keys: BTreeSet<String> = instances.iter().map(|i| lang.key(i)).collect();
    let mut out = Vec::new();
    for (n, raw) in text.lines().enumerate() {
        let c = code(raw, lang);
        for tok in c.split(separator).filter(|t| !t.is_empty()) {
            if let Some((inst, name)) = tok.split_once('.') {
                if keys.contains(&lang.key(inst)) && !name.is_empty() {
                    out.push((n + 1, inst.to_string(), name.to_string()));
                }
            }
        }
    }
    out
}

/// Write nets in place of probes: each token `<instance>.<probe>`, `<instance>` one of
/// `instances` and `<probe>` a key of `probes`, becomes `<instance>.<net>`.
pub fn place_probes(text: &str, lang: Lang, instances: &[String], probes: &BTreeMap<String, String>) -> String {
    let keys: BTreeSet<String> = instances.iter().map(|i| lang.key(i)).collect();
    rewrite(text, lang, &|tok, _| {
        let (inst, name) = tok.split_once('.')?;
        if !keys.contains(&lang.key(inst)) {
            return None;
        }
        let net = probes.iter().find(|(p, _)| lang.same(p, name)).map(|(_, n)| n)?;
        Some(format!("{inst}.{net}"))
    })
}

/// The analyses of Spectre, and `info` and `montecarlo`, which take their place among them.
pub const SPECTRE_ANALYSES: &[&str] = &[
    "ac",
    "acmatch",
    "dc",
    "dcmatch",
    "envlp",
    "hb",
    "hbac",
    "hbnoise",
    "hbsp",
    "hbxf",
    "info",
    "montecarlo",
    "noise",
    "pac",
    "pdisto",
    "pnoise",
    "psp",
    "pss",
    "pstb",
    "pxf",
    "pz",
    "qpac",
    "qpnoise",
    "qpsp",
    "qpss",
    "qpxf",
    "sens",
    "sp",
    "stb",
    "sweep",
    "tran",
    "xf",
];

/// The statements of a Spectre netlist: (first line, last line, code), line numbers from 0. A
/// statement goes on over lines ending in `\`, lines starting with `+`, and braces left open.
fn spectre_statements(lines: &[&str]) -> Vec<(usize, usize, String)> {
    let mut out: Vec<(usize, usize, String)> = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        let c = code(lines[i], Lang::Spectre).trim().to_string();
        if c.is_empty() {
            i += 1;
            continue;
        }
        let start = i;
        let mut text = c.trim_end_matches('\\').to_string();
        let mut depth = c.matches('{').count() as i64 - c.matches('}').count() as i64;
        let mut open = c.ends_with('\\');
        while i + 1 < lines.len() {
            let next = code(lines[i + 1], Lang::Spectre).trim();
            if !(open || depth > 0 || next.starts_with('+')) {
                break;
            }
            i += 1;
            if next.is_empty() {
                continue;
            }
            depth += next.matches('{').count() as i64 - next.matches('}').count() as i64;
            open = next.ends_with('\\');
            text.push(' ');
            text.push_str(next.trim_start_matches('+').trim_end_matches('\\'));
        }
        out.push((start, i, text));
        i += 1;
    }
    out
}

/// Whether a statement of Spectre is an analysis: `<name> [(<nodes>)] <analysis> …`.
fn is_analysis(text: &str) -> bool {
    let spaced = text.replace('(', " ( ").replace(')', " ) ");
    let w: Vec<&str> = spaced.split_whitespace().collect();
    if w.len() < 2 {
        return false;
    }
    let kind = if w[1] == "(" {
        match w.iter().position(|x| *x == ")") {
            Some(j) => w.get(j + 1).copied().unwrap_or(""),
            None => return false,
        }
    } else {
        w[1]
    };
    SPECTRE_ANALYSES.contains(&kind)
}

/// A Spectre netlist with the statements from its first analysis to its last, and what lies
/// between them, inside the braces of `statement` (7.6, a Monte Carlo simulation); every other
/// byte as it was. `Err` when where the analyses are cannot be told.
pub fn wrap_analyses(text: &str, statement: &str) -> Result<String, String> {
    let lines: Vec<&str> = text.split('\n').collect();
    let st = spectre_statements(&lines);
    for (_, _, c) in &st {
        let w: Vec<&str> = c.split_whitespace().collect();
        if w.first() == Some(&"simulator") && c.contains("lang=") && !c.contains("lang=spectre") {
            return Err("it changes to another language; where its analyses are cannot be told".into());
        }
    }
    let analyses: Vec<&(usize, usize, String)> = st.iter().filter(|s| is_analysis(&s.2)).collect();
    let (Some(first), Some(last)) = (analyses.first(), analyses.last()) else {
        return Err("it has no analysis".into());
    };
    let (a, b) = (first.0, last.1);
    let mut out: Vec<String> = lines[..a].iter().map(|l| l.to_string()).collect();
    out.push(format!("{statement} {{"));
    out.extend(lines[a..=b].iter().map(|l| l.to_string()));
    out.push("}".into());
    out.extend(lines[b + 1..].iter().map(|l| l.to_string()));
    Ok(out.join("\n"))
}

#[cfg(test)]
mod tests {
    use super::*;

    const TB: &str = "simulator lang=spectre\nglobal 0\nV0 (CLK 0) vsource type=pulse val0=0 val1=VDD \\\n  period=200p\n\
                      I0 (CLK a b 0 VDD) L3_comp\nsimulatorOptions options reltol=1e-3 temp=27\n\
                      pnoise ( I0.latch_p I0.latch_n ) pnoise start=1M\n+  iprobe=V2 // I0.not_this\nsave I0.latch_p V0:p\n";

    #[test]
    fn statements_of_spectre() {
        let n = read(TB, Lang::Spectre);
        let inst: Vec<(&str, &str, usize)> =
            n.instances.iter().map(|i| (i.name.as_str(), i.master.as_str(), i.nodes.len())).collect();
        assert_eq!(inst, [("V0", "vsource", 2), ("I0", "L3_comp", 5), ("pnoise", "pnoise", 2)]);
        assert_eq!(n.options, [("reltol".to_string(), 6), ("temp".to_string(), 6)]);
        assert!(n.parameters.is_empty());
        let r = references(TB, Lang::Spectre, &["I0".into()]);
        let names: Vec<&str> = r.iter().map(|x| x.2.as_str()).collect();
        assert_eq!(names, ["latch_p", "latch_n", "latch_p"], "comments do not count");
    }

    #[test]
    fn subcircuits_and_their_nets() {
        let ckt = "subckt top (a b \\\n c)\n  parameters w=1u\n  M1 (x a c c) nch l=30n\n  X2 (x b) inner\nends top\n\
                   subckt inner p q\n  R1 (p mid) resistor r=1k\n  R2 mid q resistor r=1k\nends inner\nparameters vdd=1\n";
        let n = read(ckt, Lang::Spectre);
        assert_eq!(n.subckts.len(), 2);
        assert_eq!(n.subckts[0].ports, ["a", "b", "c"]);
        assert!(n.subckts[0].nets.contains("x"));
        assert!(n.subckts[1].nets.contains("mid"));
        assert_eq!(n.parameters, [("vdd".to_string(), 11)], "only the top level");
        let x2 = n.instances.iter().find(|i| i.name == "X2").unwrap();
        assert_eq!((x2.master.as_str(), x2.parent.as_deref()), ("inner", Some("top")));

        let spice = ".SUBCKT dac d0 d1 out vdd PARAMS: w=1\nM1 out d0 vdd vdd pch w=1u\nXa out mid half\n.ENDS\n\
                     .param vdd=1.8\n.temp 27\n.options reltol=1e-4 ; a comment\n";
        let n = read(spice, Lang::Spice);
        assert_eq!(n.subckts[0].ports, ["d0", "d1", "out", "vdd"]);
        assert!(n.subckts[0].nets.contains("mid"));
        let m: Vec<(&str, &str)> = n.instances.iter().map(|i| (i.name.as_str(), i.master.as_str())).collect();
        assert_eq!(m, [("M1", "pch"), ("Xa", "half")]);
        let opts: Vec<&str> = n.options.iter().map(|o| o.0.as_str()).collect();
        assert_eq!(opts, ["temp", "reltol"]);
    }

    #[test]
    fn renaming_and_probes_keep_every_other_byte() {
        let text = "// L3_comp in a comment\nsubckt L3_comp (a L3_comp_x)\nI1 (a b) L3_comp\nL3_comp (a) res\nends L3_comp\r\n";
        let map = BTreeMap::from([("L3_comp".to_string(), "sock".to_string())]);
        assert_eq!(
            rename(text, Lang::Spectre, &map),
            "// L3_comp in a comment\nsubckt sock (a L3_comp_x)\nI1 (a b) sock\nL3_comp (a) res\nends sock\r\n"
        );
        let probes = BTreeMap::from([("latch_p".to_string(), "xl.LP".to_string())]);
        let placed = place_probes(TB, Lang::Spectre, &["I0".into()], &probes);
        assert!(placed.contains("pnoise ( I0.xl.LP I0.latch_n )"), "{placed}");
        assert!(placed.contains("// I0.not_this"));
        assert!(placed.contains("save I0.xl.LP V0:p"));
        // SPICE compares without regard to case
        let s = "Xi0 a b COMP\n.print v(xi0.LATCH_P)\n";
        let map = BTreeMap::from([("comp".to_string(), "sock".to_string())]);
        assert_eq!(rename(s, Lang::Spice, &map), "Xi0 a b sock\n.print v(xi0.LATCH_P)\n");
        assert_eq!(place_probes(s, Lang::Spice, &["XI0".into()], &probes), "Xi0 a b COMP\n.print v(xi0.xl.LP)\n");
    }

    #[test]
    fn analyses_wrapped() {
        let tb = "simulator lang=spectre\nI0 (a b) dut\nsave I0.x\n// the analyses\ndcOp dc write=\"spectre.dc\"\n\
                  swp sweep param=vdd values=[1 2] {\n  tr tran stop=1n \\\n    errpreset=conservative\n}\n\
                  pss1 (out 0) pss fund=1G\n+ harms=5\nfinal info what=oppoint\nsimulatorOptions options temp=27\n";
        let w = wrap_analyses(tb, "toaMonteCarlo montecarlo seed=1").unwrap();
        assert_eq!(
            w,
            "simulator lang=spectre\nI0 (a b) dut\nsave I0.x\n// the analyses\ntoaMonteCarlo montecarlo seed=1 {\n\
             dcOp dc write=\"spectre.dc\"\nswp sweep param=vdd values=[1 2] {\n  tr tran stop=1n \\\n    errpreset=conservative\n}\n\
             pss1 (out 0) pss fund=1G\n+ harms=5\nfinal info what=oppoint\n}\nsimulatorOptions options temp=27\n"
        );
        assert!(wrap_analyses("simulator lang=spectre\nI0 (a b) dut\n", "m montecarlo").is_err());
        assert!(wrap_analyses(
            "simulator lang=spice\n.tran 1n 1u\nsimulator lang=spectre\ntr tran stop=1n\n",
            "m montecarlo"
        )
        .is_err());
    }
}
