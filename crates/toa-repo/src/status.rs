//! The state of a library's working directory against its publication branch (7.1, 6.3). It
//! changes nothing.

use std::collections::BTreeMap;
use std::path::Path;

use crate::git::Git;
use crate::library;
use crate::seal::{scan, shown, Stop};

/// One thing to tell, paths relative to the root of the library, `/` between levels.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Item {
    /// A draft, to be sealed before publishing.
    Draft { path: String },
    /// A directory with an object name whose content does not match it, or not well-formed.
    Changed { name: String, path: String, why: String },
    /// An object here that is not on the publication branch.
    Unpublished { name: String, path: String },
    /// An object on the publication branch that is not in the working directory any more.
    Gone { name: String, path: String },
}

#[derive(Debug, Default)]
pub struct Status {
    pub items: Vec<Item>,
    /// The ref that decided "published", or `None` in a repository without commits.
    pub from: Option<String>,
    /// The number of objects here.
    pub objects: usize,
    pub notices: Vec<String>,
}

/// The status of the library that holds `dir`, with `branch` as its publication branch.
pub fn status(git: &dyn Git, dir: &Path, branch: &str) -> Result<Status, Stop> {
    let Some(root) = git.toplevel(dir).map_err(|e| Stop::Git(e.to_string()))? else {
        return Err(Stop::Git(format!("{} is not in a git repository: a library is one (6.1)", shown(dir))));
    };
    let rel = |p: &Path| shown(p.strip_prefix(&root).unwrap_or(p));
    let found = scan(&root).map_err(|e| Stop::Io { message: e.to_string(), restored: true })?;
    let mut out = Status { objects: found.objects.len(), ..Default::default() };
    let head = library::publication_head(git, &root, branch).map_err(|e| Stop::Git(e.to_string()))?;
    let mut published: BTreeMap<String, Vec<String>> = match &head {
        Some(h) => library::objects_in(git, &root, &h.commit, 2_000_000).map_err(|e| Stop::Git(e.to_string()))?,
        None => BTreeMap::new(),
    };
    if let Some(h) = &head {
        out.from = Some(h.from.clone());
        // a remote, but this branch was never fetched from it or pushed to it
        if h.from.starts_with("refs/heads/") && git.remote_url(&root, "origin").ok().flatten().is_some() {
            out.notices.push(format!(
                "the branch '{branch}' has no remote-tracking ref, so what is committed only here counts as published (6.3)"
            ));
        }
    }

    for d in &found.drafts {
        out.items.push(Item::Draft { path: rel(d) });
    }
    for o in &found.objects {
        let name = o.file_name().and_then(|n| n.to_str()).unwrap_or_default().to_string();
        let path = rel(o);
        match crate::fs::verify_dir(o) {
            Ok(v) if toa_core::problem::has_errors(&v.problems) => {
                let why = v.problems.iter().find(|p| p.is_error()).map(|p| p.to_string()).unwrap_or_default();
                out.items.push(Item::Changed { name: name.clone(), path: path.clone(), why });
            }
            Ok(_) => {}
            Err(e) => out.items.push(Item::Changed { name: name.clone(), path: path.clone(), why: e.to_string() }),
        }
        match published.get_mut(&name) {
            // the copies on the branch that are here; the rest are gone
            Some(paths) => paths.retain(|p| *p != path),
            None => out.items.push(Item::Unpublished { name, path }),
        }
    }
    let here: std::collections::BTreeSet<String> =
        found.objects.iter().filter_map(|o| o.file_name().and_then(|n| n.to_str()).map(str::to_string)).collect();
    for (name, paths) in published {
        // an object moved within the library is here under another path: not gone
        if here.contains(&name) {
            continue;
        }
        for path in paths {
            out.items.push(Item::Gone { name: name.clone(), path });
        }
    }
    Ok(out)
}
