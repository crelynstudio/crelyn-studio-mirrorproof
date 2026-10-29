use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Scope {
    #[serde(default)]
    pub exclusions: Vec<String>,
}

impl Scope {
    pub fn validate(&self) -> Result<(), String> {
        if self.exclusions.len() > 32
            || self.exclusions.iter().any(|rule| {
                rule.is_empty()
                    || rule.len() > 240
                    || rule.starts_with(['/', '\\'])
                    || rule.chars().any(|c| c.is_control() || ":<>\"|".contains(c))
                    || rule
                        .replace('\\', "/")
                        .split('/')
                        .any(|p| p == ".." || p == ".")
            })
        {
            return Err("invalidExclusions".into());
        }
        Ok(())
    }

    pub fn excludes(&self, path: &str) -> bool {
        if self.exclusions.is_empty() {
            return false;
        }
        let path = path.replace('\\', "/").to_lowercase();
        self.exclusions.iter().any(|rule| {
            let rule = rule.replace('\\', "/").to_lowercase();
            let folder = rule.ends_with('/');
            let rule = rule.trim_end_matches('/');
            if rule.contains('/') {
                glob(rule, &path) || (folder && path.starts_with(&format!("{rule}/")))
            } else {
                path.split('/').any(|part| glob(rule, part))
            }
        })
    }
}

fn glob(pattern: &str, value: &str) -> bool {
    // Bounded dynamic programming avoids exponential wildcard backtracking.
    let p: Vec<char> = pattern.chars().collect();
    let v: Vec<char> = value.chars().collect();
    let mut previous = vec![false; v.len() + 1];
    previous[0] = true;
    let mut i = 0;
    while i < p.len() {
        let mut next = vec![false; v.len() + 1];
        if p[i] == '*' {
            let recursive = p.get(i + 1) == Some(&'*');
            if recursive {
                i += 1;
            }
            if recursive && p.get(i + 1) == Some(&'/') {
                let mut reachable = false;
                for j in 0..=v.len() {
                    reachable |= previous[j];
                    next[j] = previous[j] || (j > 0 && v[j - 1] == '/' && reachable);
                }
                previous = next;
                i += 2;
                continue;
            }
            next[0] = previous[0];
            for j in 1..=v.len() {
                next[j] = previous[j] || (next[j - 1] && (recursive || v[j - 1] != '/'));
            }
        } else {
            for j in 1..=v.len() {
                next[j] = previous[j - 1] && (p[i] == v[j - 1] || (p[i] == '?' && v[j - 1] != '/'));
            }
        }
        previous = next;
        i += 1;
    }
    previous[v.len()]
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn scope_matches_basename_folders_and_recursive_paths() {
        let scope = Scope {
            exclusions: vec![
                "*.tmp".into(),
                "cache/".into(),
                "build/**".into(),
                "**/log?.txt".into(),
            ],
        };
        scope.validate().unwrap();
        for path in [
            "a.TMP",
            "nested/a.tmp",
            "cache",
            "cache/file",
            "nested/cache",
            "build/deep/a",
            "log1.txt",
            "a/log2.txt",
        ] {
            assert!(scope.excludes(path), "{path}");
        }
        for path in [
            "tmp/file.txt",
            "cache-other/a",
            "build-other/a",
            "log12.txt",
            "main.txt",
        ] {
            assert!(!scope.excludes(path), "{path}");
        }
        for rule in ["../secret", "C:/secret", "/root", "bad\0"] {
            assert!(Scope {
                exclusions: vec![rule.into()]
            }
            .validate()
            .is_err());
        }
    }
}
