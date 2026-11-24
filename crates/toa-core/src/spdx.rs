//! The syntax of an SPDX license expression (SPDX 2.3, annex D), as the field `license` takes it
//! (3.2, with the spec line's answer 6): identifiers, `LicenseRef-`, `AND`, `OR`, `WITH`,
//! parentheses and `+`. `DocumentRef-` is not allowed, nor `NONE` and `NOASSERTION`.

/// What an expression holds, once its syntax is right.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Expression {
    /// License identifiers, without `+` (for example `MIT`, `GPL-2.0-only`).
    pub licenses: Vec<String>,
    /// `LicenseRef-` references, written in full.
    pub refs: Vec<String>,
    /// Exception identifiers after `WITH`.
    pub exceptions: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Token {
    Id(String),
    And,
    Or,
    With,
    Open,
    Close,
}

fn is_id_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '-' || c == '.'
}

fn tokens(s: &str) -> Result<Vec<Token>, String> {
    let mut out = Vec::new();
    let mut chars = s.char_indices().peekable();
    while let Some(&(i, c)) = chars.peek() {
        if c == ' ' {
            chars.next();
        } else if c == '(' {
            out.push(Token::Open);
            chars.next();
        } else if c == ')' {
            out.push(Token::Close);
            chars.next();
        } else if is_id_char(c) {
            let mut end = i;
            while let Some(&(j, d)) = chars.peek() {
                if is_id_char(d) || d == ':' || d == '+' {
                    end = j + d.len_utf8();
                    chars.next();
                } else {
                    break;
                }
            }
            let word = &s[i..end];
            out.push(match word {
                "AND" | "and" => Token::And,
                "OR" | "or" => Token::Or,
                "WITH" | "with" => Token::With,
                _ => Token::Id(word.to_string()),
            });
        } else {
            return Err(format!("the character {c:?} has no place in a license expression"));
        }
    }
    Ok(out)
}

struct P {
    t: Vec<Token>,
    i: usize,
    out: Expression,
}

impl P {
    fn peek(&self) -> Option<&Token> {
        self.t.get(self.i)
    }

    fn expr(&mut self) -> Result<(), String> {
        self.term()?;
        while self.peek() == Some(&Token::Or) {
            self.i += 1;
            self.term()?;
        }
        Ok(())
    }

    fn term(&mut self) -> Result<(), String> {
        self.factor()?;
        while self.peek() == Some(&Token::And) {
            self.i += 1;
            self.factor()?;
        }
        Ok(())
    }

    fn factor(&mut self) -> Result<(), String> {
        match self.t.get(self.i).cloned() {
            Some(Token::Open) => {
                self.i += 1;
                self.expr()?;
                if self.peek() != Some(&Token::Close) {
                    return Err("a '(' is not closed".into());
                }
                self.i += 1;
                Ok(())
            }
            Some(Token::Id(id)) => {
                self.i += 1;
                self.simple(&id)?;
                if self.peek() == Some(&Token::With) {
                    self.i += 1;
                    match self.t.get(self.i).cloned() {
                        Some(Token::Id(e)) if !e.contains([':', '+']) && !e.starts_with("LicenseRef-") => {
                            self.i += 1;
                            self.out.exceptions.push(e);
                        }
                        _ => return Err("an exception identifier follows WITH".into()),
                    }
                }
                Ok(())
            }
            _ => Err("a license identifier or '(' is expected".into()),
        }
    }

    fn simple(&mut self, id: &str) -> Result<(), String> {
        if id.starts_with("DocumentRef-") {
            return Err("DocumentRef- points outside the object and is not allowed".into());
        }
        if id == "NONE" || id == "NOASSERTION" {
            return Err(format!("{id} is not a license expression"));
        }
        if id.contains(':') {
            return Err(format!("'{id}' is not a license identifier"));
        }
        if let Some(rest) = id.strip_prefix("LicenseRef-") {
            if rest.is_empty() || rest.contains('+') {
                return Err(format!("'{id}' is not a LicenseRef"));
            }
            self.out.refs.push(id.to_string());
            return Ok(());
        }
        let base = id.strip_suffix('+').unwrap_or(id);
        if base.is_empty() || base.contains('+') {
            return Err(format!("'{id}' is not a license identifier"));
        }
        self.out.licenses.push(base.to_string());
        Ok(())
    }
}

/// Parse an expression; `Err` says what is wrong with its syntax.
pub fn parse(s: &str) -> Result<Expression, String> {
    if s.trim().is_empty() {
        return Err("a license expression is not empty".into());
    }
    let mut p = P { t: tokens(s)?, i: 0, out: Expression::default() };
    p.expr()?;
    if p.i != p.t.len() {
        return Err("the expression goes on after its end".into());
    }
    Ok(p.out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expressions() {
        for s in [
            "MIT",
            "Apache-2.0 OR MIT",
            "(MIT AND BSD-3-Clause) or GPL-2.0+",
            "GPL-2.0-only WITH Classpath-exception-2.0",
        ] {
            assert!(parse(s).is_ok(), "{s}");
        }
        assert_eq!(parse("LicenseRef-acme AND CC0-1.0").unwrap().refs, ["LicenseRef-acme"]);
        for s in
            ["", "MIT AND", "(MIT", "MIT Apache-2.0", "NONE", "DocumentRef-x:LicenseRef-y", "MIT/Apache", "MIT WITH"]
        {
            assert!(parse(s).is_err(), "{s}");
        }
    }
}
