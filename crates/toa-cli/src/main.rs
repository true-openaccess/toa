//! `toa`: the reference tool of TOA.

use std::path::PathBuf;
use std::process::ExitCode;

use toa_core::hash::Algorithm;
use toa_core::problem::{has_errors, Problem};

/// The version of the standard this tool implements.
const STANDARD: &str = "0.1";

mod runs;

const USAGE: &str = "usage:
  toa hash [--sha256] DIR...   print the hash of each directory's files (2.2); a tree that is not
                               well-formed (3.1) has no hash
  toa verify [--types] DIR...  verify object directories from their files (7.4): the tree, the
                               manifest, type and prefix, and the name against the hash. With
                               --types, also what the type specification asks (chapter 9), as
                               far as the object alone can tell
  toa source check FILE...     check source lists by the strict reading of a publication branch
                               (5.1); the kind of list follows from its name
  toa seal [DIR] [--dry-run] [--workspace FILE]
                               seal every draft under DIR (default: the repository, or here): names
                               in place of draft names, history.json, the hash computed twice,
                               renamed with its sidecar files, its source list written (7.1), with
                               the git sources of objects published in libraries of the workspace
                               list that have an address. It completes, or changes nothing
  toa derive NAME|DIR [--tag TAG] [--into DIR] [--dry-run] [--workspace FILE] [--offline]
             [--allow-host HOST]... [--yes]
                               copy an object into a new draft whose base is that object (3.4):
                               found as toa fetch finds it (by name: this machine, then what the
                               workspace list gives) or taken from its directory, and verified
                               from its files first (7.4). Its files but history.json are copied,
                               nothing beside it; in toa.yaml, base becomes [the object] and no
                               other byte changes. The draft is <prefix>_draft-<tag> in DIR
                               (default: here); the tag is TAG, else the manifest's name with _
                               as -, else from-<8 hex of the hash>. Refuses a tag taken here or
                               by a draft of the repository. It completes, or writes nothing
  toa assemble TB CKT (--preset NAME[:INDEX] | --conditions FILE) [--probe PROBE=NET]...
               [--sample K] [--map FILE] [--out DIR] [--dry-run] [--workspace FILE] [--offline]
               [--allow-host HOST]... [--yes]
                               assemble a run (7.6): the testbench TB on the circuit CKT (each a
                               name or a directory) with its expanded tree, every object verified
                               (7.4), for one set of conditions: a preset of the testbench (a list
                               of sets needs :INDEX, from 0) or a file of {parameters, options,
                               models}. Each probe the testbench lists needs --probe with its net
                               in the circuit (a net of its subcircuit, or instances down to it
                               and the net, joined by .), checked to be there. Model and external
                               files come from the local file map (--map, else local-files.yaml
                               beside the workspace list), match and sha256 checked. Writes DIR
                               (default <testbench name>-<preset>): tb/, ckt/<object>/, input.scs
                               or input.sp, lock.json; the circuit's subcircuit renamed to the
                               socket, clashing ones renamed, probes replaced by their nets.
                               Conditions with statistics are a Monte Carlo simulation: its
                               analyses inside one toaMonteCarlo statement, for all its samples,
                               or for sample K alone with --sample. Runs nothing. It completes,
                               or writes nothing
  toa run TB CKT (--preset NAME | --conditions FILE) [the options of assemble]
          [--command CMD] [--launcher TEMPLATE] [--python EXE] [--agree SHA] [--no-analysis]
          [--mc-run each|batch] [--evd-into LIB] [--license L] [--author A] [--short S]
                               assemble and run (7.6, chapter 11): for each set of conditions (a
                               preset that is a list gives DIR/<preset>-<index>), assemble, run the
                               simulator (the command of run pointed at the entry file, results
                               in analyze.results; --command replaces it, {input} is the entry
                               file: it is recorded, so it must be the simulator's own command
                               line, and a program that is not a simulator toa knows is warned
                               of). --launcher says how this machine starts that command (a
                               sandbox, a job scheduler) and is never recorded: {command} in it
                               is the whole command, {args} the command without its program;
                               with neither, it is a prefix. For example --launcher
                               'my-sandbox --read-only /models {args}' or 'srun -n1'.
                               Then write results/conditions.json, run the analysis script
                               in a process of its own, in analysis/, with python (--python):
                               only after the user agrees to it. Asked interactively, with the
                               script or its difference from the version agreed before; without
                               a terminal it needs --agree with the start of its SHA-256 (at least
                               12 digits). Reads out/metrics.json and out/evidence/. With
                               --evd-into, writes an evidence record draft into the library LIB
                               (evd/<interface>/<stamp>-<short>/): conditions, probes,
                               environment, metrics, evidence and lock.json; seal it when checked.
                               Conditions with statistics are a Monte Carlo simulation (a record
                               of the kind mc): its samples run each in a simulation of its own
                               (DIR/runs/<k>) or all in one (batch), as the preset says or else
                               --mc-run; the results of each split into DIR/samples/<k>/results,
                               the analysis script called on each, then evidence/samples.csv and
                               the statistics of each metric computed from it
  toa unseal NAME [--branch B] turn an object that is not published back into a draft, with the
                               unpublished objects that refer to it (7.1)
  toa status [--branch B]      list drafts, object directories whose content does not match their
                               names, objects not on the publication branch, and objects on it
                               that are not here any more (7.1, 6.3). Changes nothing
  toa latest NAME|PATH... | --all [--off] [--branch B] [--commit [--sign] | --dry-run]
                               mark objects latest in the library list (5.2), taking the mark
                               from the others of their series in the same lineage; --off takes
                               it away. --all marks the newest generation of every series that
                               has no mark. For objects published before toa publish gave marks
  toa remove NAME|PATH [--record] [--reason TEXT] [--to ADDRESS] [--keep-notes] [--branch B]
             [--dry-run]
                               remove one copy of an object from the library (6.5): the object
                               directory, its source list, and its notes and timestamp proofs
                               unless kept (--keep-notes keeps both). With
                               --record, leave a removal record where it was, filled from the
                               object as the publication branch holds it (4.2), with the git
                               sources of other places from its source list (this library's
                               address, --to or the remote origin, left out). Refuses an object
                               in no commit, whose files would be lost. It completes, or changes
                               nothing
  toa fetch NAME... [--into DIR] [--workspace FILE] [--offline] [--allow-host HOST]... [--yes]
                               fetch objects by name (7.3): the machine first, then what the
                               workspace list gives; every copy verified from its files before
                               it is used or written into place. Hosts not written in your own
                               workspace list are asked about, or need --allow-host
  toa index [--workspace FILE] [--out DIR] [--full] [--follow-includes] [--offline]
            [--allow-host HOST]...
                               read every library the workspace list includes and write the index
                               (8.2, 8.3; format: toa-repo/doc/index-format.md). Never asks; exit
                               code 4 when some library could not be fetched
  toa cat NAME PATH [--workspace FILE] [--offline] [--allow-host HOST]...
                               write one file of an object to standard output, as raw bytes, after
                               the object was found and verified as toa fetch does
  toa publish [--to ADDRESS] [--branch B] [--commit [--sign] | --dry-run] [--workspace FILE]
              [--volumes] [--keep-latest]
                               before publishing (7.2): no drafts; the objects published this
                               time verified; their source lists tidied by 5.3 (local paths out,
                               this library's address in); the library list written again, its
                               latest marks moved to the new generation of a marked series and
                               given to a series new here (--keep-latest leaves them as they
                               are). With --commit, commits, signed as your git settings say
                               (--sign: it must be signed), tells whether it is signed, and
                               runs the library check; never pushes. With --volumes, the library
                               list is written in volumes (5.2); a list in volumes stays so
  toa check [--rev R] [--base B | --no-base] [--branch NAME] [--workspace FILE]
                               the library check (6.4) of commit R (default HEAD) against the
                               head of the publication branch (default main; the remote-tracking
                               ref when there is one, 6.3). Also hints (info, never errors) at
                               files that are byte for byte those of an earlier object of
                               another author not in base or history (8.3), among the objects
                               here and in the libraries of the workspace list on this machine.
                               Earlier: first along first parents of a library holding both,
                               else by committer dates. Not compared: licence texts,
                               history.json, lock.json, files under 256 bytes, files the
                               object's lineage holds, and common files: held by objects of more
                               than 5 groups (objects linked by lineage or a reference are one
                               group) or of more than 5 authors, or by 200 objects or more. The
                               same code as toa index, which writes it into the index
  toa --version                print the tool's version and the version of TOA it implements
  toa --help                   print this help
options:
  --json                       print JSON Lines on standard output instead of text
  -C DIR                       run as if started in DIR
  --git=auto|system|builtin    which git to use: by default the system git when it is installed and
                               new enough, else the built-in one. The built-in git reads libraries
                               and fetches objects; sealing then hashes the files only, and
                               committing needs the system git
exit codes: 0 done, 1 problems found, 2 usage error, 3 could not run (no git, files unreadable),
            4 not fetched (not found, offline, or a host not allowed)";

struct Out {
    json: bool,
    errors: usize,
    warnings: usize,
}

impl Out {
    fn problem(&mut self, object: &str, p: &Problem) {
        match p.level {
            toa_core::problem::Level::Error => self.errors += 1,
            toa_core::problem::Level::Warning => self.warnings += 1,
            toa_core::problem::Level::Info => {}
        }
        if self.json {
            let mut v = serde_json::to_value(p).unwrap_or_default();
            v["kind"] = "diag".into();
            v["object"] = object.into();
            println!("{v}");
        } else if object.is_empty() {
            eprintln!("{p}");
        } else {
            eprintln!("{object}: {p}");
        }
    }

    fn line(&self, kind: &str, value: serde_json::Value, text: String) {
        if self.json {
            let mut v = value;
            v["kind"] = kind.into();
            println!("{v}");
        } else {
            println!("{text}");
        }
    }

    /// Print the summary (the `result` line in JSON) and give the exit code: 1 when an error was
    /// found, else 0.
    fn finish(&self) -> ExitCode {
        self.summary();
        ExitCode::from(if self.errors > 0 { 1 } else { 0 })
    }

    /// Print the summary as [`Out::finish`] does, and exit with `code` instead (4: not fetched).
    fn finish_as(&self, code: u8) -> ExitCode {
        self.summary();
        ExitCode::from(code)
    }

    fn summary(&self) {
        if self.json {
            println!(
                "{}",
                serde_json::json!({"kind": "result", "ok": self.errors == 0, "errors": self.errors, "warnings": self.warnings})
            );
        } else if self.errors + self.warnings > 0 {
            eprintln!("{} error(s), {} warning(s)", self.errors, self.warnings);
        }
    }
}

static GIT_CHOICE: std::sync::OnceLock<toa_repo::git::Choice> = std::sync::OnceLock::new();

/// The git chosen by `--git`; the user is told when the built-in one stands in.
fn open_git() -> Result<Box<dyn toa_repo::git::Git>, ExitCode> {
    match toa_repo::git::choose(GIT_CHOICE.get().copied().unwrap_or_default()) {
        Ok((g, note)) => {
            if let Some(n) = note {
                eprintln!("note: {n}");
            }
            Ok(g)
        }
        Err(m) => {
            eprintln!("toa: {m}");
            Err(ExitCode::from(3))
        }
    }
}

fn usage_error(msg: &str) -> ExitCode {
    eprintln!("toa: {msg}\n{USAGE}");
    ExitCode::from(2)
}

fn main() -> ExitCode {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    let json = take_flag(&mut args, "--json");
    let choice = match args.iter().position(|a| a == "--git" || a.starts_with("--git=")) {
        Some(i) => {
            let a = args.remove(i);
            let v = match a.strip_prefix("--git=") {
                Some(v) => v.to_string(),
                None if i < args.len() => args.remove(i),
                None => return usage_error("--git takes auto, system or builtin"),
            };
            match v.parse::<toa_repo::git::Choice>() {
                Ok(c) => c,
                Err(m) => return usage_error(&m),
            }
        }
        None => toa_repo::git::Choice::Auto,
    };
    let _ = GIT_CHOICE.set(choice);
    match take_value(&mut args, "-C") {
        Ok(Some(dir)) => {
            if let Err(e) = std::env::set_current_dir(&dir) {
                eprintln!("toa: cannot change to {dir}: {e}");
                return ExitCode::from(3);
            }
        }
        Ok(None) => {}
        Err(m) => return usage_error(&m),
    }
    let Some(cmd) = (!args.is_empty()).then(|| args.remove(0)) else {
        println!("{USAGE}");
        return ExitCode::SUCCESS;
    };
    let mut out = Out { json, errors: 0, warnings: 0 };
    match cmd.as_str() {
        "--version" | "-V" => {
            println!("toa {} (TOA {STANDARD})", env!("CARGO_PKG_VERSION"));
            ExitCode::SUCCESS
        }
        "--help" | "-h" | "help" => {
            println!("{USAGE}");
            ExitCode::SUCCESS
        }
        "hash" => {
            let sha256 = take_flag(&mut args, "--sha256");
            if args.is_empty() || args.iter().any(|a| a.starts_with('-')) {
                return usage_error("hash takes one or more directories");
            }
            for d in args.iter().map(PathBuf::from) {
                if let Err(code) = hash_one(&mut out, &d, sha256) {
                    return code;
                }
            }
            out.finish()
        }
        "verify" => {
            let types = take_flag(&mut args, "--types");
            if args.is_empty() || args.iter().any(|a| a.starts_with('-')) {
                return usage_error("verify takes one or more directories, and --types");
            }
            for d in args.iter().map(PathBuf::from) {
                let shown = d.display().to_string();
                let verified =
                    if types { toa_repo::fs::verify_dir_with_type(&d) } else { toa_repo::fs::verify_dir(&d) };
                match verified {
                    Ok(v) => {
                        for p in &v.problems {
                            out.problem(&shown, p);
                        }
                        let ok = !has_errors(&v.problems);
                        out.line(
                            "verified",
                            serde_json::json!({"path": shown, "ok": ok, "hash": v.hash}),
                            format!("{} {shown}", if ok { "ok  " } else { "FAIL" }),
                        );
                    }
                    Err(e) => {
                        eprintln!("toa: cannot read {shown}: {e}");
                        return ExitCode::from(3);
                    }
                }
            }
            out.finish()
        }
        "source" => {
            if args.first().map(String::as_str) != Some("check") || args.len() < 2 {
                return usage_error("source check takes one or more files");
            }
            for f in &args[1..] {
                let shown = f.clone();
                let bytes = match std::fs::read(f) {
                    Ok(b) => b,
                    Err(e) => {
                        eprintln!("toa: cannot read {shown}: {e}");
                        return ExitCode::from(3);
                    }
                };
                let (kind, limit) = list_kind(f);
                let r = toa_core::source::read(&bytes, true, kind, limit, "");
                for p in &r.problems {
                    out.problem(&shown, p);
                }
                let ok = !has_errors(&r.problems);
                out.line(
                    "checked",
                    serde_json::json!({"path": shown, "ok": ok, "lines": r.lines.len()}),
                    format!("{} {shown}", if ok { "ok  " } else { "FAIL" }),
                );
            }
            out.finish()
        }
        "check" => check(&mut out, args),
        "publish" => publish(&mut out, args),
        "latest" => latest(&mut out, args),
        "seal" => seal(&mut out, args),
        "fetch" => fetch(&mut out, args),
        "index" => index(&mut out, args),
        "cat" => cat(args),
        "unseal" => unseal(&mut out, args),
        "derive" => derive(&mut out, args),
        "assemble" => runs::assemble_cmd(&mut out, args),
        "run" => runs::run_cmd(&mut out, args),
        "status" => status(&mut out, args),
        "remove" => remove(&mut out, args),
        other => usage_error(&format!("unknown command '{other}'")),
    }
}

fn hash_one(out: &mut Out, d: &std::path::Path, sha256: bool) -> Result<(), ExitCode> {
    let shown = d.display().to_string();
    let c = toa_repo::fs::read_dir(d).map_err(|e| {
        eprintln!("toa: cannot read {shown}: {e}");
        ExitCode::from(3)
    })?;
    let mut entries: Vec<toa_core::tree::Entry> = c.dirs.iter().map(toa_core::tree::Entry::dir).collect();
    for (p, b) in &c.files {
        let mut e = toa_core::tree::Entry::file(p.clone(), b.len() as u64);
        e.executable = Some(c.executable.contains(p));
        entries.push(e);
    }
    // only the shape of 3.1 matters for a hash; a missing manifest only makes it no object
    let mut problems = c.problems.clone();
    problems.extend(
        toa_core::tree::check(&entries, toa_core::tree::Source::FileSystem)
            .into_iter()
            .filter(|p| p.code != "E_TREE_NO_MANIFEST"),
    );
    for p in &problems {
        out.problem(&shown, p);
    }
    if has_errors(&problems) {
        return Ok(());
    }
    let files: Vec<(&str, &[u8])> = c.files.iter().map(|(p, b)| (p.as_str(), b.as_slice())).collect();
    let h1 = toa_core::hash::hash_files(Algorithm::Sha1, &files);
    let h2 = if sha256 { Some(toa_core::hash::hash_files(Algorithm::Sha256, &files)) } else { None };
    match (h1, h2.transpose()) {
        (Ok(h), Ok(h256)) => {
            let text = match &h256 {
                Some(x) => format!("{h} {x} {shown}"),
                None => format!("{h} {shown}"),
            };
            out.line("hash", serde_json::json!({"path": shown, "hash": h, "sha256": h256}), text);
        }
        (Err(e), _) | (_, Err(e)) => out.problem(&shown, &Problem::error("E_HASH", "2.2", "", e.to_string())),
    }
    Ok(())
}

/// The kind and size limit of a list from its path (5.2): the library list and its volumes, a
/// list beside an object, or any other.
fn list_kind(path: &str) -> (toa_core::source::Kind, usize) {
    use toa_core::source::{self, Kind};
    let p = path.replace('\\', "/");
    let file = p.rsplit('/').next().unwrap_or("");
    let parent = p.rsplit('/').nth(1).unwrap_or("");
    if file == source::LIBRARY_LIST || parent == source::LIBRARY_VOLUMES {
        (Kind::Library, source::MAX_VOLUME)
    } else if file.strip_suffix(source::SUFFIX).is_some_and(toa_core::name::is_object_name) {
        (Kind::Sidecar, source::MAX_SIDECAR_LIST)
    } else {
        (Kind::Other, source::MAX_VOLUME)
    }
}

fn check(out: &mut Out, mut args: Vec<String>) -> ExitCode {
    use toa_repo::library;
    let (rev, base, branch) =
        match (take_value(&mut args, "--rev"), take_value(&mut args, "--base"), take_value(&mut args, "--branch")) {
            (Ok(r), Ok(b), Ok(br)) => (r, b, br),
            (Err(m), _, _) | (_, Err(m), _) | (_, _, Err(m)) => return usage_error(&m),
        };
    let workspace = match take_value(&mut args, "--workspace") {
        Ok(w) => w.map(PathBuf::from),
        Err(m) => return usage_error(&m),
    };
    let no_base = take_flag(&mut args, "--no-base");
    if !args.is_empty() || (no_base && base.is_some()) {
        return usage_error("check takes --rev, --base or --no-base, --branch and --workspace");
    }
    let git_box = match open_git() {
        Ok(g) => g,
        Err(code) => return code,
    };
    let git = git_box.as_ref();
    let here = std::path::PathBuf::from(".");
    let repo = match git.toplevel(&here) {
        Ok(Some(r)) => r,
        Ok(None) => here.clone(),
        Err(e) => return could_not_run(&e),
    };
    let rev = rev.unwrap_or_else(|| "HEAD".into());
    let branch = branch.unwrap_or_else(|| library::DEFAULT_BRANCH.into());
    let base = if no_base {
        None
    } else if let Some(b) = base {
        Some(b)
    } else {
        match library::publication_head(git, &repo, &branch) {
            Ok(h) => h.map(|h| h.commit),
            Err(e) => {
                eprintln!("toa: {e}");
                return ExitCode::from(3);
            }
        }
    };
    match library::check(git, &repo, &rev, base.as_deref(), &library::Limits::default()) {
        Ok(r) => {
            for p in &r.problems {
                out.problem("", p);
            }
            // the same files without a declared lineage (8.3): hints, within the objects found
            // here and in the libraries of the workspace list on this machine
            let mut found = toa_repo::same::Found::default();
            toa_repo::same::read_library(git, &repo, &rev, "this library", &mut found);
            let here_real = std::fs::canonicalize(&repo).ok();
            if let Some(ws) = workspace.or_else(|| toa_repo::fetch::find_workspace(&repo)) {
                for (dir, label, lib_branch) in toa_repo::same::local_libraries(&ws) {
                    // this library is skipped; another branch of this repository is another
                    // library (5.1)
                    if std::fs::canonicalize(&dir).ok() == here_real && lib_branch == branch {
                        continue;
                    }
                    if let Ok(Some(h)) = library::publication_head(git, &dir, &lib_branch) {
                        toa_repo::same::read_library(git, &dir, &h.commit, &label, &mut found);
                    }
                }
            }
            for p in toa_repo::same::hints(&found) {
                out.problem("", &p);
            }
            for (name, from) in &r.outside {
                out.line(
                    "outside",
                    serde_json::json!({"object": name, "from": from}),
                    format!("outside this library: {name} (from {})", from.join(", ")),
                );
            }
            out.line(
                "checked",
                serde_json::json!({"rev": rev, "base": base, "objects": r.objects, "ok": !has_errors(&r.problems)}),
                format!(
                    "{} {rev}: {} object director{}, base {}",
                    if has_errors(&r.problems) { "FAIL" } else { "ok  " },
                    r.objects,
                    if r.objects == 1 { "y" } else { "ies" },
                    base.as_deref().unwrap_or("none")
                ),
            );
            out.finish()
        }
        Err(e) => could_not_run(&e),
    }
}

fn latest(out: &mut Out, mut args: Vec<String>) -> ExitCode {
    use toa_repo::latest;
    let branch = match take_value(&mut args, "--branch") {
        Ok(b) => b,
        Err(m) => return usage_error(&m),
    };
    let opts = latest::Options {
        off: take_flag(&mut args, "--off"),
        all: take_flag(&mut args, "--all"),
        dry_run: take_flag(&mut args, "--dry-run"),
        commit: take_flag(&mut args, "--commit"),
        sign: take_flag(&mut args, "--sign"),
        branch: branch.unwrap_or_else(|| toa_repo::library::DEFAULT_BRANCH.into()),
    };
    if (args.is_empty() != opts.all)
        || (opts.all && opts.off)
        || args.iter().any(|a| a.starts_with("--"))
        || (opts.commit && opts.dry_run)
        || (opts.sign && !opts.commit)
    {
        return usage_error(
            "latest takes object names or paths (or --all), --off, --branch, and --commit (with --sign) or --dry-run",
        );
    }
    let git_box = match open_git() {
        Ok(g) => g,
        Err(code) => return code,
    };
    match latest::latest(git_box.as_ref(), std::path::Path::new("."), &args, &opts) {
        Ok(r) => {
            for n in &r.notices {
                eprintln!("note: {n}");
            }
            let would = if opts.dry_run { "would " } else { "" };
            for n in &r.marked {
                out.line(
                    "latest",
                    serde_json::json!({"object": n, "latest": true, "dry_run": opts.dry_run}),
                    format!("{would}mark {n} latest"),
                );
            }
            for n in &r.unmarked {
                out.line(
                    "latest",
                    serde_json::json!({"object": n, "latest": false, "dry_run": opts.dry_run}),
                    format!("{would}take the latest mark from {n}"),
                );
            }
            for c in &r.changed {
                out.line(
                    "changed",
                    serde_json::json!({"path": c, "dry_run": opts.dry_run}),
                    format!("{}{c}", if opts.dry_run { "would change " } else { "changed " }),
                );
            }
            for p in &r.check {
                out.problem("", p);
            }
            out.line(
                "marked",
                serde_json::json!({"commit": r.commit, "signature": r.signature.map(|s| s.as_str()), "dry_run": opts.dry_run}),
                match &r.commit {
                    Some(c) => format!(
                        "committed {c} ({}); nothing was pushed",
                        r.signature.map_or("not signed".to_string(), |s| format!("signed, {}", s.as_str()))
                    ),
                    None if r.changed.is_empty() => "the marks were already so; nothing was changed".into(),
                    None if opts.dry_run => "nothing was changed (--dry-run)".into(),
                    None => "not committed (--commit makes a commit)".into(),
                },
            );
            out.finish()
        }
        Err(stop) => stopped(out, stop),
    }
}

fn stopped(out: &mut Out, stop: toa_repo::seal::Stop) -> ExitCode {
    use toa_repo::seal::Stop;
    match stop {
        Stop::Problems(ps) => {
            for p in &ps {
                out.problem("", p);
            }
            eprintln!("toa: nothing was changed");
            out.finish()
        }
        other => could_not_run(&other),
    }
}

fn seal(out: &mut Out, mut args: Vec<String>) -> ExitCode {
    let dry_run = take_flag(&mut args, "--dry-run");
    let workspace = match take_value(&mut args, "--workspace") {
        Ok(w) => w.map(PathBuf::from),
        Err(m) => return usage_error(&m),
    };
    if args.len() > 1 || args.iter().any(|a| a.starts_with('-')) {
        return usage_error("seal takes one directory, --workspace and --dry-run");
    }
    let git_box = match open_git() {
        Ok(g) => g,
        Err(code) => return code,
    };
    let git = git_box.as_ref();
    let dir = match args.first() {
        Some(d) => PathBuf::from(d),
        None => match git.toplevel(std::path::Path::new(".")) {
            Ok(Some(r)) => r,
            _ => PathBuf::from("."),
        },
    };
    let opts = toa_repo::seal::SealOptions {
        dry_run,
        lookup: toa_repo::fetch::cache_objects_dir().into_iter().collect(),
        workspace,
    };
    match toa_repo::seal::seal(git, &dir, &opts) {
        Ok(r) => {
            for n in &r.notices {
                eprintln!("note: {n}");
            }
            for (d, o, p) in &r.objects {
                let shown = p.display().to_string();
                out.line(
                    "sealed",
                    serde_json::json!({"draft": d, "object": o, "path": shown, "dry_run": dry_run}),
                    format!("{}{d} -> {o}  {shown}", if dry_run { "would seal " } else { "" }),
                );
            }
            if r.objects.is_empty() {
                eprintln!("no drafts under {}", dir.display());
            }
            out.finish()
        }
        Err(stop) => stopped(out, stop),
    }
}

fn unseal(out: &mut Out, mut args: Vec<String>) -> ExitCode {
    let branch = match take_value(&mut args, "--branch") {
        Ok(b) => b.unwrap_or_else(|| toa_repo::library::DEFAULT_BRANCH.into()),
        Err(m) => return usage_error(&m),
    };
    if args.len() != 1 || args[0].starts_with('-') {
        return usage_error("unseal takes one object name");
    }
    let git_box = match open_git() {
        Ok(g) => g,
        Err(code) => return code,
    };
    let git = git_box.as_ref();
    let dir = {
        match git.toplevel(std::path::Path::new(".")) {
            Ok(Some(r)) => r,
            _ => PathBuf::from("."),
        }
    };
    match toa_repo::seal::unseal(git, &dir, &args[0], &branch) {
        Ok(r) => {
            for (o, d, p) in &r.drafts {
                let shown = p.display().to_string();
                out.line(
                    "unsealed",
                    serde_json::json!({"object": o, "draft": d, "path": shown}),
                    format!("{o} -> {d}  {shown}"),
                );
            }
            out.finish()
        }
        Err(stop) => stopped(out, stop),
    }
}

fn status(out: &mut Out, mut args: Vec<String>) -> ExitCode {
    use toa_repo::status::Item;
    let branch = match take_value(&mut args, "--branch") {
        Ok(b) => b.unwrap_or_else(|| toa_repo::library::DEFAULT_BRANCH.into()),
        Err(m) => return usage_error(&m),
    };
    if !args.is_empty() {
        return usage_error("status takes --branch");
    }
    let git_box = match open_git() {
        Ok(g) => g,
        Err(code) => return code,
    };
    match toa_repo::status::status(git_box.as_ref(), std::path::Path::new("."), &branch) {
        Ok(s) => {
            for n in &s.notices {
                eprintln!("note: {n}");
            }
            for i in &s.items {
                let (kind, v, text) = match i {
                    Item::Draft { path } => {
                        ("draft", serde_json::json!({"path": path}), format!("draft        {path}"))
                    }
                    Item::Changed { name, path, why } => (
                        "changed",
                        serde_json::json!({"name": name, "path": path, "why": why}),
                        format!("changed      {path}  ({why})"),
                    ),
                    Item::Unpublished { name, path } => {
                        ("unpublished", serde_json::json!({"name": name, "path": path}), format!("unpublished  {path}"))
                    }
                    Item::Gone { name, path } => (
                        "gone",
                        serde_json::json!({"name": name, "path": path}),
                        format!("gone         {path}  (on '{branch}', not here)"),
                    ),
                };
                out.line(kind, v, text);
            }
            let from = s.from.clone().unwrap_or_else(|| "no commits".into());
            out.line(
                "status",
                serde_json::json!({"objects": s.objects, "items": s.items.len(), "branch": branch, "from": s.from}),
                format!("{} object(s) here; {} to tell; published by {from}", s.objects, s.items.len()),
            );
            out.finish()
        }
        Err(stop) => stopped(out, stop),
    }
}

fn remove(out: &mut Out, mut args: Vec<String>) -> ExitCode {
    let (branch, reason, to) =
        match (take_value(&mut args, "--branch"), take_value(&mut args, "--reason"), take_value(&mut args, "--to")) {
            (Ok(b), Ok(r), Ok(t)) => (b.unwrap_or_else(|| toa_repo::library::DEFAULT_BRANCH.into()), r, t),
            (Err(m), _, _) | (_, Err(m), _) | (_, _, Err(m)) => return usage_error(&m),
        };
    let opts = toa_repo::remove::Options {
        record: take_flag(&mut args, "--record"),
        keep_notes: take_flag(&mut args, "--keep-notes"),
        dry_run: take_flag(&mut args, "--dry-run"),
        reason,
        branch,
        to,
    };
    if args.len() != 1 || args[0].starts_with('-') {
        return usage_error("remove takes one object name or object directory");
    }
    if opts.reason.is_some() && !opts.record {
        return usage_error("--reason goes into the removal record: give --record too");
    }
    let git_box = match open_git() {
        Ok(g) => g,
        Err(code) => return code,
    };
    match toa_repo::remove::remove(git_box.as_ref(), std::path::Path::new("."), &args[0], &opts) {
        Ok(r) => {
            let would = if opts.dry_run { "would remove " } else { "removed " };
            for p in &r.removed {
                out.line(
                    "removed",
                    serde_json::json!({"object": r.object, "path": p, "dry_run": opts.dry_run}),
                    format!("{would}{p}"),
                );
            }
            if let Some(rec) = &r.record {
                out.line(
                    "record",
                    serde_json::json!({"object": r.object, "path": rec, "dry_run": opts.dry_run}),
                    format!("{}{rec}", if opts.dry_run { "would write " } else { "wrote " }),
                );
            }
            for n in &r.notices {
                eprintln!("note: {n}");
            }
            out.finish()
        }
        Err(stop) => stopped(out, stop),
    }
}

fn fetch(out: &mut Out, mut args: Vec<String>) -> ExitCode {
    use std::io::IsTerminal;
    use toa_repo::fetch::{self, FetchError, Fetcher};
    let into = take_value(&mut args, "--into");
    let ws = take_value(&mut args, "--workspace");
    let (into, ws) = match (into, ws) {
        (Ok(i), Ok(w)) => (i, w),
        (Err(m), _) | (_, Err(m)) => return usage_error(&m),
    };
    let mut hosts = Vec::new();
    loop {
        match take_value(&mut args, "--allow-host") {
            Ok(Some(h)) => hosts.push(h.to_ascii_lowercase()),
            Ok(None) => break,
            Err(m) => return usage_error(&m),
        }
    }
    let offline = take_flag(&mut args, "--offline");
    let yes = take_flag(&mut args, "--yes");
    if args.is_empty() || args.iter().any(|a| a.starts_with('-')) {
        return usage_error("fetch takes one or more object names");
    }
    let git_box = match open_git() {
        Ok(g) => g,
        Err(code) => return code,
    };
    let git = git_box.as_ref();
    let Some(cache) = fetch::cache_dir() else {
        return could_not_run(&"no cache directory: set TOA_CACHE");
    };
    let mut f = Fetcher::new(git, cache);
    f.offline = offline;
    f.allowed.extend(hosts);
    {
        if let Ok(Some(top)) = git.toplevel(std::path::Path::new(".")) {
            f.local.push(top);
        }
    }
    let ws_file = ws.map(PathBuf::from).or_else(|| fetch::find_workspace(std::path::Path::new(".")));
    if let Some(w) = &ws_file {
        match fetch::read_workspace(w) {
            Ok(w) => f.use_workspace(w),
            Err(e) => eprintln!("toa: the workspace list {} cannot be read: {e}", w.display()),
        }
    }
    if yes {
        f.ask = Some(Box::new(|_: &str| true));
    } else if std::io::stdin().is_terminal() && std::io::stderr().is_terminal() {
        f.ask = Some(Box::new(|host: &str| {
            eprint!("toa: connect to {host}? It is not in your own workspace list. [y/N] ");
            let mut line = String::new();
            std::io::stdin().read_line(&mut line).is_ok() && matches!(line.trim(), "y" | "Y" | "yes")
        }));
    }
    let mut missing = 0;
    for n in &args {
        match f.fetch(n, into.as_deref().map(std::path::Path::new)) {
            Ok(x) => {
                let shown = x.path.display().to_string();
                out.line(
                    "fetched",
                    serde_json::json!({"object": x.name, "path": shown, "from": x.from}),
                    format!("{}  {shown}  (from {})", x.name, x.from),
                );
            }
            Err(FetchError::BadName(m)) => return usage_error(&format!("'{m}' is not an object name")),
            Err(FetchError::Local(m)) => return could_not_run(&m),
            Err(e @ FetchError::NotFound { .. }) => {
                missing += 1;
                out.line("missing", serde_json::json!({"object": n, "why": e.to_string()}), format!("{n}: {e}"));
            }
        }
    }
    if missing > 0 {
        out.finish_as(4)
    } else {
        out.finish()
    }
}

fn derive(out: &mut Out, mut args: Vec<String>) -> ExitCode {
    use std::io::IsTerminal;
    use toa_repo::derive::{self, DeriveError};
    use toa_repo::fetch::{self, FetchError, Fetcher};
    let (tag, into, ws, hosts) = match (
        take_value(&mut args, "--tag"),
        take_value(&mut args, "--into"),
        take_value(&mut args, "--workspace"),
        take_hosts(&mut args),
    ) {
        (Ok(t), Ok(i), Ok(w), Ok(h)) => (t, i, w, h),
        (Err(m), ..) | (_, Err(m), ..) | (_, _, Err(m), _) | (.., Err(m)) => return usage_error(&m),
    };
    let dry_run = take_flag(&mut args, "--dry-run");
    let offline = take_flag(&mut args, "--offline");
    let yes = take_flag(&mut args, "--yes");
    if args.len() != 1 || args[0].starts_with('-') {
        return usage_error("derive takes one object name or object directory");
    }
    let what = &args[0];
    let opts = derive::Options { tag, into: into.map(PathBuf::from).unwrap_or_else(|| PathBuf::from(".")), dry_run };
    // an object directory, or a name found as toa fetch finds it
    let from = if std::path::Path::new(what).is_dir() {
        PathBuf::from(what)
    } else if toa_core::name::is_object_name(what) {
        let git_box = match open_git() {
            Ok(g) => g,
            Err(code) => return code,
        };
        let git = git_box.as_ref();
        let Some(cache) = fetch::cache_dir() else { return could_not_run(&"no cache directory: set TOA_CACHE") };
        let mut f = Fetcher::new(git, cache);
        f.offline = offline;
        f.allowed.extend(hosts);
        if let Ok(Some(top)) = git.toplevel(std::path::Path::new(".")) {
            f.local.push(top);
        }
        if let Some(w) = ws.map(PathBuf::from).or_else(|| fetch::find_workspace(std::path::Path::new("."))) {
            match fetch::read_workspace(&w) {
                Ok(w) => f.use_workspace(w),
                Err(e) => eprintln!("toa: the workspace list {} cannot be read: {e}", w.display()),
            }
        }
        if yes {
            f.ask = Some(Box::new(|_: &str| true));
        } else if std::io::stdin().is_terminal() && std::io::stderr().is_terminal() {
            f.ask = Some(Box::new(|host: &str| {
                eprint!("toa: connect to {host}? It is not in your own workspace list. [y/N] ");
                let mut line = String::new();
                std::io::stdin().read_line(&mut line).is_ok() && matches!(line.trim(), "y" | "Y" | "yes")
            }));
        }
        match f.fetch(what, None) {
            Ok(x) => x.path,
            Err(FetchError::BadName(m)) => return usage_error(&format!("'{m}' is not an object name")),
            Err(FetchError::Local(m)) => return could_not_run(&m),
            Err(e @ FetchError::NotFound { .. }) => {
                out.line("missing", serde_json::json!({"object": what, "why": e.to_string()}), format!("{what}: {e}"));
                return out.finish_as(4);
            }
        }
    } else {
        return usage_error(&format!("'{what}' is neither an object name nor a directory"));
    };
    match derive::derive(&from, &opts) {
        Ok(d) => {
            let shown = d.path.display().to_string();
            out.line(
                "derived",
                serde_json::json!({"object": d.object, "draft": d.draft, "path": shown,
                    "base": if d.base_replaced { "replaced" } else { "added" }, "files": d.files, "dry_run": dry_run}),
                format!("{}{} -> {}  {shown}", if dry_run { "would derive " } else { "" }, d.object, d.draft),
            );
            out.finish()
        }
        Err(DeriveError::BadTag(t)) => usage_error(&format!("'{t}' is not a draft tag (2.4)")),
        Err(DeriveError::Problems(ps)) => {
            for p in &ps {
                out.problem("", p);
            }
            out.finish()
        }
        Err(DeriveError::Io(m)) => could_not_run(&m),
    }
}

fn take_hosts(args: &mut Vec<String>) -> Result<Vec<String>, String> {
    let mut hosts = Vec::new();
    while let Some(h) = take_value(args, "--allow-host")? {
        hosts.push(h.to_ascii_lowercase());
    }
    Ok(hosts)
}

fn index(out: &mut Out, mut args: Vec<String>) -> ExitCode {
    use toa_repo::{fetch, index};
    let (ws, dir, hosts) =
        match (take_value(&mut args, "--workspace"), take_value(&mut args, "--out"), take_hosts(&mut args)) {
            (Ok(w), Ok(o), Ok(h)) => (w, o, h),
            (Err(m), _, _) | (_, Err(m), _) | (_, _, Err(m)) => return usage_error(&m),
        };
    let full = take_flag(&mut args, "--full");
    let follow = take_flag(&mut args, "--follow-includes");
    let offline = take_flag(&mut args, "--offline");
    if !args.is_empty() {
        return usage_error("index takes no other arguments");
    }
    let Some(ws) = ws.map(PathBuf::from).or_else(|| fetch::find_workspace(std::path::Path::new("."))) else {
        return could_not_run(&"no workspace.source.jsonl here or above; name one with --workspace");
    };
    let Some(cache) = fetch::cache_dir() else { return could_not_run(&"no cache directory: set TOA_CACHE") };
    let out_dir = dir
        .map(PathBuf::from)
        .unwrap_or_else(|| ws.parent().map(|p| p.join("toa-index")).unwrap_or_else(|| "toa-index".into()));
    let opts = index::Options {
        workspace: ws,
        out: out_dir.clone(),
        cache,
        full,
        follow_includes: follow,
        offline,
        allowed: hosts.into_iter().collect(),
        tool: format!("toa {}", env!("CARGO_PKG_VERSION")),
    };
    let git_box = match open_git() {
        Ok(g) => g,
        Err(code) => return code,
    };
    let git = git_box.as_ref();
    match index::build(git, &opts) {
        Ok(r) => {
            if out.json {
                for p in &r.problems {
                    println!("{p}");
                }
            }
            for k in &r.unreadable {
                eprintln!("toa: library {k} could not be read; see its record in the index");
            }
            let shown = out_dir.display().to_string();
            out.line(
                "indexed",
                serde_json::json!({"out": shown, "libraries": r.libraries, "objects": r.objects, "valid": r.valid,
                    "problems": r.problems.len(), "unreadable": r.unreadable}),
                format!(
                    "{} librar{}, {} objects ({} valid), {} finding(s) -> {shown}",
                    r.libraries,
                    if r.libraries == 1 { "y" } else { "ies" },
                    r.objects,
                    r.valid,
                    r.problems.len()
                ),
            );
            // findings in libraries are recorded in the index, not failures (index format, section 10)
            if out.json {
                println!("{}", serde_json::json!({"kind": "result", "ok": true, "errors": 0, "warnings": 0}));
            }
            ExitCode::from(if r.not_fetched { 4 } else { 0 })
        }
        Err(m) => could_not_run(&m),
    }
}

fn cat(mut args: Vec<String>) -> ExitCode {
    use std::io::Write;
    use toa_repo::fetch::{self, FetchError, Fetcher};
    let (ws, hosts) = match (take_value(&mut args, "--workspace"), take_hosts(&mut args)) {
        (Ok(w), Ok(h)) => (w, h),
        (Err(m), _) | (_, Err(m)) => return usage_error(&m),
    };
    let offline = take_flag(&mut args, "--offline");
    if args.len() != 2 || args.iter().any(|a| a.starts_with('-')) {
        return usage_error("cat takes an object name and a path in it");
    }
    let (name, rel) = (&args[0], &args[1]);
    let ok_path = !rel.is_empty()
        && !rel.contains('\\')
        && rel.split('/').all(|p| toa_core::tree::component_problem(p, false).is_none());
    if !ok_path {
        return usage_error(&format!("'{rel}' is not a path in an object"));
    }
    let git_box = match open_git() {
        Ok(g) => g,
        Err(code) => return code,
    };
    let git = git_box.as_ref();
    let Some(cache) = fetch::cache_dir() else { return could_not_run(&"no cache directory: set TOA_CACHE") };
    let mut f = Fetcher::new(git, cache);
    f.offline = offline;
    f.allowed.extend(hosts);
    {
        if let Ok(Some(top)) = git.toplevel(std::path::Path::new(".")) {
            f.local.push(top);
        }
    }
    if let Some(w) = ws.map(PathBuf::from).or_else(|| fetch::find_workspace(std::path::Path::new("."))) {
        if let Ok(w) = fetch::read_workspace(&w) {
            f.use_workspace(w);
        }
    }
    match f.fetch(name, None) {
        Ok(x) => {
            let file = x.path.join(rel);
            match std::fs::symlink_metadata(&file) {
                Ok(m) if m.is_file() && m.len() <= toa_core::tree::MAX_FILE_BYTES => match std::fs::read(&file) {
                    Ok(b) => {
                        let _ = std::io::stdout().write_all(&b);
                        ExitCode::SUCCESS
                    }
                    Err(e) => could_not_run(&e),
                },
                _ => {
                    eprintln!("toa: {name} has no file {rel}");
                    ExitCode::from(1)
                }
            }
        }
        Err(FetchError::BadName(m)) => usage_error(&format!("'{m}' is not an object name")),
        Err(FetchError::Local(m)) => could_not_run(&m),
        Err(e) => {
            eprintln!("toa: {name}: {e}");
            ExitCode::from(4)
        }
    }
}

fn publish(out: &mut Out, mut args: Vec<String>) -> ExitCode {
    use toa_repo::publish;
    let (to, branch, ws) = match (
        take_value(&mut args, "--to"),
        take_value(&mut args, "--branch"),
        take_value(&mut args, "--workspace"),
    ) {
        (Ok(t), Ok(b), Ok(w)) => (t, b, w),
        (Err(m), _, _) | (_, Err(m), _) | (_, _, Err(m)) => return usage_error(&m),
    };
    let commit = take_flag(&mut args, "--commit");
    let volumes = take_flag(&mut args, "--volumes");
    let dry_run = take_flag(&mut args, "--dry-run");
    let sign = take_flag(&mut args, "--sign");
    let keep_latest = take_flag(&mut args, "--keep-latest");
    if !args.is_empty() || (commit && dry_run) || (sign && !commit) {
        return usage_error(
            "publish takes --to, --branch, --workspace, --volumes, --keep-latest, and --commit (with --sign) or \
             --dry-run",
        );
    }
    let opts = publish::Options {
        to,
        branch: branch.unwrap_or_else(|| toa_repo::library::DEFAULT_BRANCH.into()),
        commit,
        dry_run,
        workspace: ws.map(PathBuf::from),
        volumes,
        sign,
        keep_latest,
    };
    let git_box = match open_git() {
        Ok(g) => g,
        Err(code) => return code,
    };
    let git = git_box.as_ref();
    match publish::publish(git, std::path::Path::new("."), &opts) {
        Ok(r) => {
            for n in &r.notices {
                eprintln!("note: {n}");
            }
            let verb = if dry_run { "would change" } else { "changed" };
            for c in &r.changed {
                out.line("changed", serde_json::json!({"path": c, "dry_run": dry_run}), format!("{verb} {c}"));
            }
            for (from, to) in &r.latest {
                out.line(
                    "latest",
                    serde_json::json!({"object": to, "from": from, "dry_run": dry_run}),
                    match from {
                        Some(f) => format!("latest: {to} in place of {f} (--keep-latest leaves the marks)"),
                        None => format!("latest: {to}, the first of its series here (--keep-latest leaves the marks)"),
                    },
                );
            }
            if !r.addresses.is_empty() {
                eprintln!(
                    "these addresses of other places are in the lists and become public with them (5.3); remove any \
                     that should not:"
                );
                for a in &r.addresses {
                    eprintln!("  {a}");
                }
            }
            for n in &r.name_only {
                out.line(
                    "name-only",
                    serde_json::json!({"object": n}),
                    format!("{n}: not in this library and no address known; only its name is kept, and others cannot find it by name (5.3)"),
                );
            }
            for (o, a) in &r.lost_ancestors {
                out.line(
                    "ancestor",
                    serde_json::json!({"object": o, "ancestor": a}),
                    format!("{o}: its ancestor {a} is neither in this library nor in a library with an address in your workspace list (5.3)"),
                );
            }
            for p in &r.check {
                out.problem("", p);
            }
            if let (Some(c), true) = (&r.commit, r.check.iter().any(|p| p.is_error())) {
                eprintln!(
                    "the commit {c} does not pass the library check (6.4), so it must not be pushed (7.2). Fix \
                     the problems above, then either commit the fixes, or fold them into this commit with \
                     `git add` and `git commit --amend`; run `toa check` to check again before pushing."
                );
            }
            out.line(
                "published",
                serde_json::json!({"objects": r.objects, "address": r.address, "commit": r.commit,
                    "signature": r.signature.map(|s| s.as_str()), "dry_run": dry_run}),
                format!(
                    "{} object(s) published this time; address {}; {}",
                    r.objects.len(),
                    r.address.as_deref().unwrap_or("none"),
                    match &r.commit {
                        Some(c) => format!(
                            "committed {c} ({}); nothing was pushed",
                            r.signature.map_or("not signed".to_string(), |s| format!("signed, {}", s.as_str()))
                        ),
                        None if dry_run => "nothing was changed (--dry-run)".into(),
                        None => "not committed (--commit makes a commit); nothing was pushed".into(),
                    }
                ),
            );
            out.finish()
        }
        Err(stop) => stopped(out, stop),
    }
}

fn could_not_run(e: &dyn std::fmt::Display) -> ExitCode {
    eprintln!("toa: {e}");
    ExitCode::from(3)
}

/// Take `NAME VALUE` from the arguments.
fn take_value(args: &mut Vec<String>, flag: &str) -> Result<Option<String>, String> {
    let Some(i) = args.iter().position(|a| a == flag) else { return Ok(None) };
    if i + 1 >= args.len() {
        return Err(format!("{flag} takes a value"));
    }
    let v = args.remove(i + 1);
    args.remove(i);
    Ok(Some(v))
}

fn take_flag(args: &mut Vec<String>, flag: &str) -> bool {
    let before = args.len();
    args.retain(|a| a != flag);
    args.len() != before
}
