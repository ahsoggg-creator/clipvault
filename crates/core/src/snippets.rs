//! Snippets: text blocks the user keeps on purpose (folders, tags, a keyword, a hotkey) with placeholders.

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Snippet {
    /// 0 = not stored yet.
    pub id: u64,
    pub name: String,
    pub text: String,
    #[serde(default)]
    pub folder: String,
    #[serde(default)]
    pub tags: Vec<String>,
    /// Typed in the popup's search box, it offers this snippet first.
    #[serde(default)]
    pub keyword: String,
    /// Global hotkey that pastes the snippet, e.g. `Ctrl+Alt+1`.
    #[serde(default)]
    pub hotkey: String,
    #[serde(default)]
    pub updated: u64,
}

/// Local wall-clock time for `{date}` and `{time}`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Clock {
    pub year: i32,
    pub month: u32,
    pub day: u32,
    pub hour: u32,
    pub minute: u32,
}

/// Replaces `{date}` (`2026-10-15`), `{time}` (`18:04`) and `{clipboard}` in one pass, so a clipboard that itself
/// contains `{date}` is not expanded again. Unknown `{words}` stay as they are.
pub fn expand(template: &str, now: &Clock, clipboard: &str) -> String {
    let mut out = String::with_capacity(template.len());
    let mut rest = template;
    while let Some(i) = rest.find('{') {
        out.push_str(&rest[..i]);
        let tail = &rest[i..];
        let replacement = [
            ("{date}", format!("{:04}-{:02}-{:02}", now.year, now.month, now.day)),
            ("{time}", format!("{:02}:{:02}", now.hour, now.minute)),
            ("{clipboard}", clipboard.to_owned()),
        ]
        .into_iter()
        .find(|(p, _)| tail.starts_with(p));
        match replacement {
            Some((p, r)) => {
                out.push_str(&r);
                rest = &tail[p.len()..];
            }
            None => {
                out.push('{');
                rest = &tail[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

impl Snippet {
    /// Does the search text match the name, folder, keyword or a tag?
    pub fn matches(&self, query: &str) -> bool {
        let q = query.trim().to_lowercase();
        if q.is_empty() {
            return true;
        }
        let hay = |s: &str| s.to_lowercase().contains(&q);
        hay(&self.name) || hay(&self.folder) || hay(&self.keyword) || self.tags.iter().any(|t| hay(t))
    }
}

/// `a, b ,c` -> `["a", "b", "c"]`
pub fn parse_tags(s: &str) -> Vec<String> {
    s.split(',')
        .map(str::trim)
        .filter(|t| !t.is_empty())
        .map(str::to_owned)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: Clock = Clock {
        year: 2026,
        month: 10,
        day: 5,
        hour: 9,
        minute: 7,
    };

    #[test]
    fn placeholders() {
        assert_eq!(expand("Today {date} at {time}", &NOW, ""), "Today 2026-10-05 at 09:07");
        assert_eq!(expand("[{clipboard}]", &NOW, "copied"), "[copied]");
        assert_eq!(
            expand("{clipboard}", &NOW, "{date}"),
            "{date}",
            "no second round of expansion"
        );
        assert_eq!(expand("{unknown} {date", &NOW, ""), "{unknown} {date");
        assert_eq!(
            expand("no placeholders, ünïcode ✓", &NOW, ""),
            "no placeholders, ünïcode ✓"
        );
        assert_eq!(expand("{date}{date}", &NOW, ""), "2026-10-052026-10-05");
        assert_eq!(expand("", &NOW, "x"), "");
    }

    #[test]
    fn matching_and_tags() {
        let s = Snippet {
            name: "Signature".into(),
            folder: "Mail".into(),
            tags: vec!["work".into(), "formal".into()],
            keyword: "sig".into(),
            ..Default::default()
        };
        for q in ["", "signa", "MAIL", "form", "sig"] {
            assert!(s.matches(q), "{q}");
        }
        assert!(!s.matches("zzz"));
        assert_eq!(parse_tags(" a, b ,,c "), vec!["a", "b", "c"]);
    }
}
