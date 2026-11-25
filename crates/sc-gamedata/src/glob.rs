//! Glob matching for DataCore record paths (port of `sc-gamedata/src/glob.ts`).
//!
//! The semantics StarBreaker's extraction filter uses, so a domain's filters select the
//! same records whether they pick them out of the archive or out of the loaded pool:
//!
//! - `*` matches within one path segment and does not cross a `/`,
//! - `**` matches across segments, and `**/` also matches zero directories,
//! - `?` matches one character other than `/`.
//!
//! Matching is case-insensitive (the `i` flag of the TypeScript regex) and anchored.

#[derive(Debug, Clone, PartialEq, Eq)]
enum Tok {
    Lit(char),
    /// `?`
    One,
    /// `*`
    Star,
    /// `**` not followed by `/`: anything, slashes included.
    Any,
    /// `**/`: empty, or anything ending in `/`.
    AnyDir,
}

/// A compiled glob.
#[derive(Debug, Clone)]
pub struct Glob {
    toks: Vec<Tok>,
    case_sensitive: bool,
}

impl Glob {
    /// `sc-gamedata`'s glob: case-insensitive.
    pub fn new(glob: &str) -> Self {
        Self::build(glob, false)
    }

    /// StarBreaker's `dcb extract --filter` glob (`glob_match`): case-sensitive.
    pub fn case_sensitive(glob: &str) -> Self {
        Self::build(glob, true)
    }

    fn build(glob: &str, case_sensitive: bool) -> Self {
        let f = |c: char| if case_sensitive { c } else { fold(c) };
        let chars: Vec<char> = glob.chars().collect();
        let mut toks = Vec::new();
        let mut i = 0;
        while i < chars.len() {
            let c = chars[i];
            if c == '*' {
                if chars.get(i + 1) == Some(&'*') {
                    if chars.get(i + 2) == Some(&'/') {
                        toks.push(Tok::AnyDir);
                        i += 3;
                    } else {
                        toks.push(Tok::Any);
                        i += 2;
                    }
                } else {
                    toks.push(Tok::Star);
                    i += 1;
                }
                continue;
            }
            toks.push(if c == '?' { Tok::One } else { Tok::Lit(f(c)) });
            i += 1;
        }
        Self {
            toks,
            case_sensitive,
        }
    }

    pub fn is_match(&self, path: &str) -> bool {
        let s: Vec<char> = if self.case_sensitive {
            path.chars().collect()
        } else {
            path.chars().map(fold).collect()
        };
        let n = s.len();
        // dp[j]: tokens so far can match s[..j]. Classic glob DP, O(tokens * len).
        let mut dp = vec![false; n + 1];
        dp[0] = true;
        for tok in &self.toks {
            let mut next = vec![false; n + 1];
            match tok {
                Tok::Lit(c) => {
                    for j in 0..n {
                        if dp[j] && s[j] == *c {
                            next[j + 1] = true;
                        }
                    }
                }
                Tok::One => {
                    for j in 0..n {
                        if dp[j] && s[j] != '/' {
                            next[j + 1] = true;
                        }
                    }
                }
                Tok::Star => {
                    for j in 0..=n {
                        if dp[j] || (j > 0 && next[j - 1] && s[j - 1] != '/') {
                            next[j] = true;
                        }
                    }
                }
                Tok::Any => {
                    let mut seen = false;
                    for j in 0..=n {
                        seen |= dp[j];
                        next[j] = seen;
                    }
                }
                Tok::AnyDir => {
                    // `(?:.*/)?`: stay, or jump to just after any later `/`.
                    let mut seen = false;
                    for j in 0..=n {
                        if dp[j] {
                            next[j] = true;
                        }
                        if j > 0 && s[j - 1] == '/' && seen {
                            next[j] = true;
                        }
                        if j < n {
                            seen |= dp[j];
                        }
                    }
                }
            }
            dp = next;
        }
        dp[n]
    }
}

/// Simple case fold, as the JavaScript `i` flag does for the paths seen here.
fn fold(c: char) -> char {
    let mut l = c.to_lowercase();
    match (l.next(), l.next()) {
        (Some(x), None) => x,
        _ => c,
    }
}

/// True when `path` matches any of the globs. An empty list matches everything.
pub fn matches_any(path: &str, globs: &[Glob]) -> bool {
    globs.is_empty() || globs.iter().any(|g| g.is_match(path))
}

pub fn compile(globs: &[&str]) -> Vec<Glob> {
    globs.iter().map(|g| Glob::new(g)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn m(glob: &str, path: &str) -> bool {
        Glob::new(glob).is_match(path)
    }

    // Cases from sc-gamedata src/glob.test.ts.
    #[test]
    fn single_star_stays_in_segment() {
        assert!(m("a/*.json", "a/b.json"));
        assert!(!m("a/*.json", "a/b/c.json"));
        assert!(!m(
            "*mineable*",
            "libs/foundry/records/mining/mineableelements/x"
        ));
    }

    #[test]
    fn double_star_crosses_segments() {
        assert!(m(
            "**/mineableelements/**",
            "libs/foundry/records/mining/mineableelements/x"
        ));
        assert!(m(
            "**/scitemmanufacturer/**",
            "libs/foundry/records/scitemmanufacturer/aegs"
        ));
        assert!(m("a/**", "a/b/c"));
        assert!(m("a**z", "a/b/z"));
    }

    #[test]
    fn double_star_slash_matches_zero_dirs() {
        assert!(m("**/x", "x"));
        assert!(m("**/x", "a/b/x"));
        assert!(!m("**/x", "ax"));
    }

    #[test]
    fn question_mark_and_case() {
        assert!(m("a?c", "abc"));
        assert!(!m("a?c", "a/c"));
        assert!(m(
            "**/SCItemManufacturer/**",
            "libs/foundry/records/scitemmanufacturer/x"
        ));
        assert!(m("a.b", "A.B"));
        assert!(!m("a.b", "axb"), "dots are literal");
    }

    #[test]
    fn ts_cases() {
        assert!(m("mining/*", "mining/gold"));
        assert!(!m("mining/*", "mining/ore/gold"));
        assert!(!m("mining", "mining/gold"));
        assert!(m("mining", "mining"));
        assert!(m("**/SciTem/**", "libs/records/scitem/ships/x"));
        assert!(m("s?ip", "ship"));
        assert!(!m("s?ip", "sllip"));
        let filters = compile(&["**/entities/scitem/ships/**", "**/mining/**"]);
        assert!(matches_any(
            "libs/records/entities/scitem/ships/shield/x",
            &filters
        ));
        assert!(matches_any(
            "libs/records/mining/mineableelements/gold",
            &filters
        ));
        assert!(!matches_any(
            "libs/records/entities/scitem/weapons/rifle",
            &filters
        ));
    }

    #[test]
    fn case_sensitive_variant() {
        assert!(Glob::case_sensitive("**/a/**").is_match("x/a/y"));
        assert!(!Glob::case_sensitive("**/a/**").is_match("x/A/y"));
    }

    #[test]
    fn empty_list_matches_everything() {
        assert!(matches_any("anything", &[]));
        assert!(!matches_any("x", &compile(&["y"])));
    }
}
