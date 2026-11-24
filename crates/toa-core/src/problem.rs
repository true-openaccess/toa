//! What a check finds: a problem with a stable code and the section of the standard it breaks.

use serde::Serialize;

/// How bad a finding is. An error makes an object, a list or a library invalid; a warning does not.
/// An info is a hint for people, never a reason to judge anything invalid (such as 8.3, "the same
/// files without a declared lineage").
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Level {
    Error,
    Warning,
    Info,
}

/// One finding. `code` is stable once released; `section` is the section of the standard.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Problem {
    pub level: Level,
    pub code: &'static str,
    pub section: &'static str,
    /// Where: a path relative to the object, list or library root, with `/` between levels.
    pub path: String,
    pub message: String,
}

impl Problem {
    pub fn error(
        code: &'static str,
        section: &'static str,
        path: impl Into<String>,
        message: impl Into<String>,
    ) -> Self {
        Problem { level: Level::Error, code, section, path: path.into(), message: message.into() }
    }

    pub fn warning(
        code: &'static str,
        section: &'static str,
        path: impl Into<String>,
        message: impl Into<String>,
    ) -> Self {
        Problem { level: Level::Warning, code, section, path: path.into(), message: message.into() }
    }

    pub fn info(
        code: &'static str,
        section: &'static str,
        path: impl Into<String>,
        message: impl Into<String>,
    ) -> Self {
        Problem { level: Level::Info, code, section, path: path.into(), message: message.into() }
    }

    pub fn is_error(&self) -> bool {
        self.level == Level::Error
    }
}

impl std::fmt::Display for Problem {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let level = match self.level {
            Level::Error => "error",
            Level::Warning => "warning",
            Level::Info => "info",
        };
        if self.path.is_empty() {
            write!(f, "{level}[{}]: {} ({})", self.code, self.message, self.section)
        } else {
            write!(f, "{level}[{}] {}: {} ({})", self.code, self.path, self.message, self.section)
        }
    }
}

/// True when any of the problems is an error.
pub fn has_errors(problems: &[Problem]) -> bool {
    problems.iter().any(Problem::is_error)
}
