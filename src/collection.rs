//! Plain-text request collections — a file of named curl commands.
//!
//! Format:
//! ```text
//! @base = http://localhost:3000
//! @token = dev-token
//!
//! ## Usuarios
//! ### get user
//! curl {{base}}/users/1 -H "Authorization: Bearer {{token}}"
//!
//! ### create user
//! curl -X POST {{base}}/users -d '{"name":"Juan"}'
//! ```
//!
//! `###` starts a named request, `##` a group (label only), `@name = value`
//! declares a variable, `#` is a comment. A request's curl may span several
//! lines using a trailing `\`.

use std::fs;

#[derive(Debug, Clone, PartialEq)]
pub struct Request {
    pub name: String,
    pub group: Option<String>,
    pub curl: String,
}

impl Request {
    /// `group/name` when the request belongs to a group, else just `name`.
    /// This is what `list` prints and what `call`/`--only` accept.
    pub fn label(&self) -> String {
        match &self.group {
            Some(group) => format!("{}/{}", group, self.name),
            None => self.name.clone(),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Collection {
    pub variables: Vec<(String, String)>,
    pub requests: Vec<Request>,
}

impl Collection {
    pub fn load(path: &str) -> Result<Self, String> {
        let text = fs::read_to_string(path)
            .map_err(|e| format!("cannot read collection '{}': {}", path, e))?;
        Ok(Self::parse(&text))
    }

    pub fn parse(text: &str) -> Self {
        let mut col = Collection::default();
        let mut group: Option<String> = None;
        let mut cur: Option<(String, Vec<String>)> = None;

        for raw in text.lines() {
            let trimmed = raw.trim();

            if let Some(name) = trimmed.strip_prefix("###") {
                Self::flush(&mut col, &group, &mut cur);
                cur = Some((name.trim().to_string(), Vec::new()));
                continue;
            }
            if let Some(name) = trimmed.strip_prefix("##") {
                Self::flush(&mut col, &group, &mut cur);
                group = Some(name.trim().to_string());
                continue;
            }
            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            }
            if let Some((key, value)) = trimmed.strip_prefix('@').and_then(|r| r.split_once('=')) {
                let key = key.trim().to_string();
                let value = value.trim().to_string();
                // Last one wins, matching merge_vars and git's `--get`. Without
                // this, substitute() would keep whichever line came first.
                match col.variables.iter_mut().find(|(k, _)| *k == key) {
                    Some(slot) => slot.1 = value,
                    None => col.variables.push((key, value)),
                }
                continue;
            }
            if let Some((_, lines)) = cur.as_mut() {
                // A trailing backslash continues the curl command on the next line.
                let line = trimmed.strip_suffix('\\').unwrap_or(trimmed);
                lines.push(line.trim().to_string());
            }
        }
        Self::flush(&mut col, &group, &mut cur);
        col
    }

    fn flush(
        col: &mut Collection,
        group: &Option<String>,
        cur: &mut Option<(String, Vec<String>)>,
    ) {
        if let Some((name, lines)) = cur.take() {
            let curl = lines.join(" ");
            if !curl.is_empty() {
                col.requests.push(Request {
                    name,
                    group: group.clone(),
                    curl,
                });
            }
        }
    }

    /// Looks a request up by name. An exact match wins; otherwise the query is
    /// read as `group/name`, split on the last `/`.
    pub fn get(&self, query: &str) -> Option<&Request> {
        if let Some(found) = self.requests.iter().find(|r| r.name == query) {
            return Some(found);
        }
        let (group, name) = query.rsplit_once('/')?;
        self.requests
            .iter()
            .find(|r| r.name == name && r.group.as_deref() == Some(group))
    }

    /// Load variables from an environment file (same `@name = value` syntax).
    pub fn load_vars(path: &str) -> Result<Vec<(String, String)>, String> {
        let text =
            fs::read_to_string(path).map_err(|e| format!("cannot read env '{}': {}", path, e))?;
        Ok(Self::parse(&text).variables)
    }
}

/// Replaces `{{name}}` placeholders using the given variables.
///
/// shortcut: single pass, no recursive expansion — variables whose values
/// contain other `{{...}}` are not re-expanded. Upgrade if nesting is needed.
pub fn substitute(text: &str, vars: &[(String, String)]) -> String {
    let mut out = text.to_string();
    for (key, value) in vars {
        out = out.replace(&format!("{{{{{}}}}}", key), value);
    }
    out
}

/// Merges `extra` into `base`; values in `extra` win, keys stay unique.
pub fn merge_vars(base: &[(String, String)], extra: &[(String, String)]) -> Vec<(String, String)> {
    let mut out = base.to_vec();
    for (key, value) in extra {
        match out.iter_mut().find(|(k, _)| k == key) {
            Some(slot) => slot.1 = value.clone(),
            None => out.push((key.clone(), value.clone())),
        }
    }
    out
}

/// Span of the `### name` block: the header through the line before the next
/// `##` (which also covers `###`, since a request block ends at the next one).
fn request_range(lines: &[String], name: &str) -> Option<(usize, usize)> {
    let start = lines.iter().position(|line| {
        line.trim()
            .strip_prefix("###")
            .map(|rest| rest.trim() == name)
            .unwrap_or(false)
    })?;
    let end = lines[start + 1..]
        .iter()
        .position(|line| line.trim_start().starts_with("##"))
        .map(|offset| start + 1 + offset)
        .unwrap_or(lines.len());
    Some((start, end))
}

/// Header line of a real group (`## name`), not a request (`### name`).
fn is_group_header(line: &str) -> bool {
    line.trim()
        .strip_prefix("##")
        .map(|rest| !rest.starts_with('#'))
        .unwrap_or(false)
}

fn is_group_line(line: &str, group: &str) -> bool {
    is_group_header(line)
        && line
            .trim()
            .strip_prefix("##")
            .is_some_and(|rest| rest.trim() == group)
}

/// Lines of one request block, ending with the blank separator.
fn block_lines(name: &str, curl: &str) -> Vec<String> {
    let mut out = vec![format!("### {}", name)];
    out.extend(curl.lines().map(|line| line.to_string()));
    out.push(String::new());
    out
}

/// Writes `### name` into `path`, replacing an existing block with that name
/// instead of appending a second one. With `group`, the block is placed at the
/// end of that group, created at the bottom when missing. Returns `true` when an
/// existing block was replaced.
pub fn upsert_request(
    path: &str,
    name: &str,
    curl: &str,
    group: Option<&str>,
) -> Result<bool, String> {
    let text = fs::read_to_string(path).unwrap_or_default();
    let mut lines: Vec<String> = text.lines().map(|line| line.to_string()).collect();
    let block = block_lines(name, curl);
    let existing = request_range(&lines, name);
    let replaced = existing.is_some();

    // Collapse duplicates left by older versions: keep the first block, drop
    // the rest. Indices before `first_end` stay valid.
    if let Some((_, first_end)) = existing {
        let at = first_end;
        while let Some((start, end)) = request_range(&lines[at..], name) {
            lines.drain(at + start..at + end);
        }
    }

    match (group, existing) {
        // No group asked for: replace it where it is, or append at the bottom.
        (None, Some((start, end))) => {
            lines.splice(start..end, block);
        }
        (None, None) => append_block(&mut lines, &block),

        // Already under that group: keep its position instead of moving it.
        (Some(group), Some((start, end))) if in_group(&lines, start, group) => {
            lines.splice(start..end, block);
        }
        (Some(group), existing) => {
            if let Some((start, end)) = existing {
                lines.drain(start..end);
            }
            insert_in_group(&mut lines, group, &block);
        }
    }

    while lines.last().is_some_and(|line| line.is_empty()) {
        lines.pop();
    }
    drop_empty_groups(&mut lines);
    let mut body = lines.join("\n");
    body.push('\n');
    fs::write(path, body).map_err(|e| format!("cannot write '{}': {}", path, e))?;
    Ok(replaced)
}

/// Removes `## name` headers left with no requests under them, so `list` never
/// shows an empty section.
fn drop_empty_groups(lines: &mut Vec<String>) {
    let mut index = 0;
    while index < lines.len() {
        if is_group_header(&lines[index]) {
            let end = lines[index + 1..]
                .iter()
                .position(|line| is_group_header(line))
                .map(|offset| index + 1 + offset)
                .unwrap_or(lines.len());
            if lines[index + 1..end]
                .iter()
                .all(|line| line.trim().is_empty())
            {
                lines.drain(index..end);
                continue;
            }
        }
        index += 1;
    }
}

fn append_block(lines: &mut Vec<String>, block: &[String]) {
    if lines.last().is_some_and(|line| !line.is_empty()) {
        lines.push(String::new());
    }
    lines.extend(block.iter().cloned());
}

/// True when the block starting at `start` sits under `## group`.
fn in_group(lines: &[String], start: usize, group: &str) -> bool {
    lines[..start]
        .iter()
        .rposition(|line| is_group_header(line))
        .is_some_and(|header| is_group_line(&lines[header], group))
}

/// Appends `block` at the end of `## group`, creating the group at the bottom
/// when it does not exist yet.
fn insert_in_group(lines: &mut Vec<String>, group: &str, block: &[String]) {
    let Some(header) = lines.iter().position(|line| is_group_line(line, group)) else {
        append_block(lines, &[format!("## {}", group)]);
        lines.extend(block.iter().cloned());
        return;
    };

    let end = lines[header + 1..]
        .iter()
        .position(|line| is_group_header(line))
        .map(|offset| header + 1 + offset)
        .unwrap_or(lines.len());

    let mut body: Vec<String> = lines[header + 1..end].to_vec();
    while body.last().is_some_and(|line| line.is_empty()) {
        body.pop();
    }
    if !body.is_empty() {
        body.push(String::new());
    }
    body.extend(block.iter().cloned());
    lines.splice(header + 1..end, body);
}

/// Deletes every `### name` block from `path`. Returns how many were removed.
pub fn remove_request(path: &str, name: &str) -> Result<usize, String> {
    let text = fs::read_to_string(path).map_err(|e| format!("cannot read '{}': {}", path, e))?;
    let mut lines: Vec<String> = text.lines().map(|line| line.to_string()).collect();

    let mut removed = 0usize;
    while let Some((start, end)) = request_range(&lines, name) {
        lines.drain(start..end);
        removed += 1;
    }

    if removed > 0 {
        while lines.last().is_some_and(|line| line.is_empty()) {
            lines.pop();
        }
        drop_empty_groups(&mut lines);
        let mut body = lines.join("\n");
        body.push('\n');
        fs::write(path, body).map_err(|e| format!("cannot write '{}': {}", path, e))?;
    }
    Ok(removed)
}

/// Key of an `@key = value` line, trimmed. `None` for any other line.
fn variable_key(line: &str) -> Option<&str> {
    line.trim()
        .strip_prefix('@')?
        .split_once('=')
        .map(|(key, _)| key.trim())
}

/// Writes `@key = value` into `path`, collapsing every existing `@key` line
/// into one. A new key joins the variables block, so it stays visible at the
/// top. Returns `true` when an existing line was replaced.
pub fn upsert_variable(path: &str, key: &str, value: &str) -> Result<bool, String> {
    let text = fs::read_to_string(path).map_err(|e| format!("cannot read '{}': {}", path, e))?;
    let new_line = format!("@{} = {}", key, value);

    let mut lines: Vec<String> = Vec::new();
    let mut replaced = false;
    for line in text.lines() {
        if variable_key(line) == Some(key) {
            if !replaced {
                lines.push(new_line.clone());
                replaced = true;
            }
            continue; // one line per key
        }
        lines.push(line.to_string());
    }

    if !replaced {
        let at = lines
            .iter()
            .rposition(|line| variable_key(line).is_some())
            .map(|index| index + 1)
            .unwrap_or(0);
        lines.insert(at, new_line);
    }

    let mut body = lines.join("\n");
    body.push('\n');
    fs::write(path, body).map_err(|e| format!("cannot write '{}': {}", path, e))?;
    Ok(replaced)
}

/// Deletes every `@key` line from `path`. Returns how many were removed.
pub fn remove_variable(path: &str, key: &str) -> Result<usize, String> {
    let text = fs::read_to_string(path).map_err(|e| format!("cannot read '{}': {}", path, e))?;

    let mut kept: Vec<&str> = Vec::new();
    let mut removed = 0usize;
    for line in text.lines() {
        if variable_key(line) == Some(key) {
            removed += 1;
        } else {
            kept.push(line);
        }
    }

    if removed > 0 {
        let mut body = kept.join("\n");
        body.push('\n');
        fs::write(path, body).map_err(|e| format!("cannot write '{}': {}", path, e))?;
    }
    Ok(removed)
}

// ─────────────────────────────────────────────────────────────────────────────
// Project layout: .pcurl/collection.pcurl + .pcurl/secrets/
// ─────────────────────────────────────────────────────────────────────────────

pub const DOT_DIR: &str = ".pcurl";
pub const COLLECTION_FILE: &str = ".pcurl/collection.pcurl";
pub const SECRETS_FILE: &str = ".pcurl/secrets/secrets.env";
pub const LEGACY_FILE: &str = "pcurl.collection";

const COLLECTION_TEMPLATE: &str = "\
# One request per ### block. Run `pcurl list` to see them.
@base = http://127.0.0.1:8080

## Sistema
### health
curl {{base}}/health
";

const SECRETS_TEMPLATE: &str = "\
# Secrets for this project. Never committed.
# Loaded automatically and overrides the collection's variables.
@token =
";

/// Creates `.pcurl/` with a collection and a self-ignoring secrets folder.
pub fn init_skeleton() -> Result<Vec<String>, String> {
    let dir = std::path::Path::new(DOT_DIR);
    if dir.exists() {
        return Err(format!("{} already exists — left untouched", DOT_DIR));
    }

    let secrets = dir.join("secrets");
    fs::create_dir_all(&secrets)
        .map_err(|e| format!("cannot create {}: {}", secrets.display(), e))?;

    let collection = dir.join("collection.pcurl");
    fs::write(&collection, COLLECTION_TEMPLATE)
        .map_err(|e| format!("cannot write {}: {}", collection.display(), e))?;

    // `*` makes git ignore everything inside, even if the project has no rule.
    let ignore = secrets.join(".gitignore");
    fs::write(&ignore, "*\n").map_err(|e| format!("cannot write {}: {}", ignore.display(), e))?;

    let env = secrets.join("secrets.env");
    fs::write(&env, SECRETS_TEMPLATE)
        .map_err(|e| format!("cannot write {}: {}", env.display(), e))?;

    Ok(vec![
        collection.display().to_string(),
        ignore.display().to_string(),
        env.display().to_string(),
    ])
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "\
# a comment
@base = http://localhost:3000
@token = dev

## Usuarios
### get user
curl {{base}}/users/1 -H \"Authorization: Bearer {{token}}\"

### create user
curl -X POST {{base}}/users -d '{\"name\":\"Juan\"}'
";

    #[test]
    fn parses_variables_groups_and_requests() {
        let col = Collection::parse(SAMPLE);
        assert_eq!(col.variables.len(), 2);
        assert_eq!(
            col.variables[0],
            ("base".to_string(), "http://localhost:3000".to_string())
        );
        assert_eq!(col.requests.len(), 2);
        assert_eq!(col.requests[0].name, "get user");
        assert_eq!(col.requests[0].group.as_deref(), Some("Usuarios"));
        assert_eq!(col.requests[1].name, "create user");
    }

    #[test]
    fn joins_continued_lines() {
        let col = Collection::parse("### up\ncurl -X POST https://x \\\n  -d '{}'\n");
        assert_eq!(col.requests[0].curl, "curl -X POST https://x -d '{}'");
    }

    #[test]
    fn ignores_requests_without_command() {
        let col = Collection::parse("### empty\n\n### real\ncurl https://x\n");
        assert_eq!(col.requests.len(), 1);
        assert_eq!(col.requests[0].name, "real");
    }

    #[test]
    fn substitutes_variables() {
        let vars = vec![("base".to_string(), "http://x".to_string())];
        assert_eq!(substitute("curl {{base}}/a", &vars), "curl http://x/a");
    }

    #[test]
    fn merge_lets_extra_win() {
        let base = vec![("k".to_string(), "old".to_string())];
        let extra = vec![
            ("k".to_string(), "new".to_string()),
            ("n".to_string(), "1".to_string()),
        ];
        let merged = merge_vars(&base, &extra);
        assert_eq!(
            merged,
            vec![
                ("k".to_string(), "new".to_string()),
                ("n".to_string(), "1".to_string())
            ]
        );
        assert_eq!(substitute("{{k}}", &merged), "new");
    }

    fn temp_file(tag: &str) -> String {
        let path = std::env::temp_dir().join(format!("pcurl-col-{}-{}", tag, std::process::id()));
        let _ = std::fs::remove_file(&path);
        path.to_string_lossy().to_string()
    }

    #[test]
    fn get_accepts_a_group_qualified_name() {
        let col =
            Collection::parse("## auth\n### register\ncurl a\n\n## pay\n### register\ncurl b\n");

        assert_eq!(col.get("auth/register").unwrap().curl, "curl a");
        assert_eq!(col.get("pay/register").unwrap().curl, "curl b");
        // An exact name match wins over the group interpretation.
        assert_eq!(col.get("register").unwrap().curl, "curl a");
        assert_eq!(col.requests[1].label(), "pay/register");
        assert!(col.get("nope/register").is_none());
        assert!(col.get("register/name").is_none());
    }

    #[test]
    fn duplicate_variables_keep_the_last_value() {
        let col = Collection::parse("@base = uno\n@base = dos\n### x\ncurl {{base}}\n");
        assert_eq!(col.variables, vec![("base".to_string(), "dos".to_string())]);
        assert_eq!(
            substitute("{{base}}", &col.variables),
            "dos",
            "last line wins, like `git config --get`"
        );
    }

    #[test]
    fn upsert_replaces_in_place_without_duplicating() {
        let path = temp_file("upsert");
        std::fs::write(&path, "@base = uno\n@otra = x\n\n### req\ncurl {{base}}\n").unwrap();

        assert!(upsert_variable(&path, "base", "dos").unwrap());

        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.starts_with("@base = dos\n"), "keeps its position");
        assert_eq!(text.matches("@base").count(), 1);
        assert!(text.contains("@otra = x"));
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn upsert_inserts_before_the_first_request() {
        let path = temp_file("insert");
        std::fs::write(&path, "### req\ncurl {{base}}\n").unwrap();

        assert!(!upsert_variable(&path, "base", "http://x").unwrap());

        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "@base = http://x\n### req\ncurl {{base}}\n"
        );
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn upsert_joins_the_existing_variables_block() {
        let path = temp_file("block");
        std::fs::write(&path, "@base = uno\n\n## Grupo\n### req\ncurl {{base}}\n").unwrap();

        assert!(!upsert_variable(&path, "email", "a@b.com").unwrap());

        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "@base = uno\n@email = a@b.com\n\n## Grupo\n### req\ncurl {{base}}\n"
        );
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn upsert_collapses_existing_duplicates() {
        let path = temp_file("collapse");
        std::fs::write(&path, "@k = a\n@k = b\n").unwrap();

        assert!(upsert_variable(&path, "k", "c").unwrap());

        assert_eq!(std::fs::read_to_string(&path).unwrap(), "@k = c\n");
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn remove_variable_drops_every_matching_line() {
        let path = temp_file("remove");
        std::fs::write(&path, "@k = a\n@k = b\n@otra = z\n").unwrap();

        assert_eq!(remove_variable(&path, "k").unwrap(), 2);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "@otra = z\n");
        assert_eq!(remove_variable(&path, "k").unwrap(), 0);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn upsert_request_replaces_in_place_and_keeps_the_group() {
        let path = temp_file("req-upsert");
        std::fs::write(
            &path,
            "@base = x\n\n## Auth\n### a\ncurl a\n\n### b\ncurl b\n",
        )
        .unwrap();

        assert!(upsert_request(&path, "a", "curl a2", Some("Auth")).unwrap());

        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "@base = x\n\n## Auth\n### a\ncurl a2\n\n### b\ncurl b\n"
        );
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn upsert_request_appends_at_the_end_of_the_group() {
        let path = temp_file("req-group");
        std::fs::write(
            &path,
            "@base = x\n\n## Auth\n### a\ncurl a\n\n## Other\n### z\ncurl z\n",
        )
        .unwrap();

        assert!(!upsert_request(&path, "b", "curl b", Some("Auth")).unwrap());

        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "@base = x\n\n## Auth\n### a\ncurl a\n\n### b\ncurl b\n\n## Other\n### z\ncurl z\n"
        );
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn upsert_request_creates_the_group_at_the_bottom() {
        let path = temp_file("req-newgroup");
        std::fs::write(&path, "@base = x\n").unwrap();

        assert!(!upsert_request(&path, "a", "curl a", Some("Auth")).unwrap());

        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "@base = x\n\n## Auth\n### a\ncurl a\n"
        );
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn upsert_request_moves_a_block_between_groups() {
        let path = temp_file("req-move");
        std::fs::write(&path, "## A\n### a\ncurl a\n\n## B\n### b\ncurl b\n").unwrap();

        assert!(upsert_request(&path, "a", "curl a2", Some("B")).unwrap());

        let text = std::fs::read_to_string(&path).unwrap();
        assert!(
            text.ends_with("## B\n### b\ncurl b\n\n### a\ncurl a2\n"),
            "{text}"
        );
        assert!(!text.contains("## A"), "empty group left behind: {text}");
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn upsert_request_collapses_duplicates() {
        let path = temp_file("req-dup");
        std::fs::write(&path, "## A\n### a\ncurl 1\n\n### a\ncurl 2\n").unwrap();

        assert!(upsert_request(&path, "a", "curl 3", None).unwrap());

        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "## A\n### a\ncurl 3\n"
        );
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn remove_request_drops_every_block_with_that_name() {
        let path = temp_file("req-rm");
        std::fs::write(
            &path,
            "## A\n### a\ncurl a\n\n### a\ncurl a-dup\n\n## B\n### b\ncurl b\n",
        )
        .unwrap();

        assert_eq!(remove_request(&path, "a").unwrap(), 2);
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "## B\n### b\ncurl b\n"
        );
        assert_eq!(remove_request(&path, "a").unwrap(), 0);
        let _ = std::fs::remove_file(&path);
    }
}
