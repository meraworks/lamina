//! Checks that the documentation agrees with the programs.
//!
//! The documents are README.md, CONTRIBUTING.md, docs/*.md and examples/**/README.md.
//! The checks:
//!
//! - Each tool in the README table "What the agent can do" exists. `import_part(s)` means
//!   `import_part` and `import_parts`.
//! - Each "N tools" in a document is the number of tools that `tools/list` returns.
//! - Each tool that a document names exists. A word in backticks is a tool name when:
//!   - the words after it are "tool" or "tools" ("the `notes` tool", "`a` and `b` tools"),
//!   - the words before it are "tools are" or "tool is",
//!   - the next item is a CLI command ("`export_step` and `lamina export-step`"),
//!   - it starts a numbered list item, under a line that says "tool" ("calls the tools in
//!     this order:").
//!
//!   Also, each word in backticks with an underscore must be a tool, a tool parameter, a
//!   field of a project file, a DRC check name, or a word in [`OTHER_WORDS`].
//! - Each `lamina <subcommand>` and `lamina-viewer <subcommand>` is a real subcommand.
//! - Each relative link and image points to a file that exists, and each `#anchor` to a
//!   heading of that file.
//!
//! A failure message says what to change. When you add or remove a tool, change the
//! number of tools in README.md.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use clap::CommandFactory;

/// Words in backticks with an underscore that are not tools, tool parameters, project
/// file fields or DRC check names. Add a word here when the documentation test reports
/// a word that is not a tool.
const OTHER_WORDS: &[&str] = &[
    // A function of lamina.libsonnet.
    "corner_holes",
    // Fields of lamina.json (the workspace file).
    "kicad_footprints",
    "kicad_models",
    // A check of lamina-core (validate.rs), not of lamina-drc.
    "pin_role",
];

/// Board folders that the documents give to `lamina-viewer` as examples. The viewer
/// takes a board folder as its first argument, so these look like subcommands.
const EXAMPLE_BOARDS: &[&str] = &[
    "my-board",
    "c3-sensor",
    "tit-flight-computer",
    "yoink-main-board",
];

/// This file, for the failure messages.
const THIS_FILE: &str = "crates/lamina-mcp/src/doc_check.rs";

/// A document: its path relative to the repository root, with `/`, and its text.
struct Doc {
    path: String,
    text: String,
}

/// What the documents are checked against.
struct Facts {
    tools: BTreeSet<String>,
    /// Tool parameters, project file fields and DRC check names.
    known: BTreeSet<String>,
    lamina: BTreeSet<String>,
    viewer: BTreeSet<String>,
}

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// The documents that the test checks.
fn docs(root: &Path) -> Vec<Doc> {
    let mut paths = vec![root.join("README.md"), root.join("CONTRIBUTING.md")];
    let mut in_docs: Vec<PathBuf> = std::fs::read_dir(root.join("docs"))
        .expect("read docs/")
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().is_some_and(|e| e == "md"))
        .collect();
    in_docs.sort();
    paths.extend(in_docs);
    let mut examples = Vec::new();
    find_readmes(&root.join("examples"), &mut examples);
    examples.sort();
    paths.extend(examples);
    paths
        .into_iter()
        .filter(|p| p.is_file())
        .map(|p| Doc {
            path: p
                .strip_prefix(root)
                .unwrap()
                .to_string_lossy()
                .replace('\\', "/"),
            text: std::fs::read_to_string(&p).unwrap(),
        })
        .collect()
}

fn find_readmes(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for e in entries {
        let p = e.unwrap().path();
        if p.is_dir() {
            find_readmes(&p, out);
        } else if p.file_name().is_some_and(|n| n == "README.md") {
            out.push(p);
        }
    }
}

fn facts() -> Facts {
    let list = crate::server::Lamina::new(None).tools();
    let tools: BTreeSet<String> = list.iter().map(|t| t.name.to_string()).collect();
    let mut known = BTreeSet::new();
    for t in &list {
        property_names(
            &serde_json::Value::Object((*t.input_schema).clone()),
            &mut known,
        );
    }
    for kind in lamina_core::model::SCHEMA_KINDS {
        property_names(&lamina_core::model::schema_for(kind).unwrap(), &mut known);
    }
    known.extend(lamina_drc::RULE_NAMES.iter().map(|s| s.to_string()));

    let mut cmd = crate::Cli::command();
    cmd.build();
    let lamina = cmd
        .get_subcommands()
        .map(|c| c.get_name().to_string())
        .collect();

    // The viewer has no clap parser. Its subcommands are the tests of its first
    // argument in main(): `args.first().is_some_and(|a| a == "shot")`.
    let viewer_main = repo_root().join("crates/lamina-viewer/src/main.rs");
    let src = std::fs::read_to_string(&viewer_main).unwrap();
    let viewer: BTreeSet<String> = src
        .lines()
        .filter(|l| l.contains("args.first()"))
        .filter_map(|l| {
            let s = l.split("a == \"").nth(1)?;
            Some(s.split('"').next()?.to_string())
        })
        .collect();
    assert!(
        !viewer.is_empty(),
        "found no subcommands in {}; update facts() in {THIS_FILE}",
        viewer_main.display()
    );
    Facts {
        tools,
        known,
        lamina,
        viewer,
    }
}

/// All keys of all "properties" objects in a JSON schema.
fn property_names(v: &serde_json::Value, out: &mut BTreeSet<String>) {
    match v {
        serde_json::Value::Object(m) => {
            if let Some(serde_json::Value::Object(props)) = m.get("properties") {
                out.extend(props.keys().cloned());
            }
            m.values().for_each(|x| property_names(x, out));
        }
        serde_json::Value::Array(a) => a.iter().for_each(|x| property_names(x, out)),
        _ => {}
    }
}

/// The lines outside fenced code blocks, with their line numbers (from 1).
/// `<code>` and `</code>` become backticks.
fn prose_lines(text: &str) -> Vec<(usize, String)> {
    let mut out = Vec::new();
    let mut fence = false;
    for (i, line) in text.lines().enumerate() {
        let t = line.trim_start();
        if t.starts_with("```") || t.starts_with("~~~") {
            fence = !fence;
            continue;
        }
        if !fence {
            out.push((i + 1, line.replace("<code>", "`").replace("</code>", "`")));
        }
    }
    out
}

#[derive(Debug, Clone, PartialEq)]
enum Tok {
    Text(String),
    Code(String),
}

/// Split a line into text and code spans. A backtick with no partner is text.
fn spans(line: &str) -> Vec<Tok> {
    let mut out = Vec::new();
    let mut rest = line;
    while let Some(a) = rest.find('`') {
        let Some(b) = rest[a + 1..].find('`') else {
            break;
        };
        if a > 0 {
            out.push(Tok::Text(rest[..a].to_string()));
        }
        out.push(Tok::Code(rest[a + 1..a + 1 + b].to_string()));
        rest = &rest[a + 2 + b..];
    }
    if !rest.is_empty() {
        out.push(Tok::Text(rest.to_string()));
    }
    out
}

/// A word that can be a tool name: lower case, digits and underscores.
fn is_ident(s: &str) -> bool {
    s.starts_with(|c: char| c.is_ascii_lowercase())
        && s.chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
}

fn norm(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// The text between two names in a list of names: "`a`, `b` and `c`".
fn is_list_sep(s: &str) -> bool {
    let s = norm(s);
    matches!(s.as_str(), "," | "and" | ", and" | "or" | ", or" | ", then")
}

/// "tool" or "tools" as the first word of `s`.
fn starts_with_tool(s: &str) -> bool {
    let s = s.trim_start().to_lowercase();
    let rest = s.strip_prefix("tools").or_else(|| s.strip_prefix("tool"));
    rest.is_some_and(|r| !r.starts_with(|c: char| c.is_alphanumeric()))
}

fn ends_with_tools_are(s: &str) -> bool {
    let s = norm(s).to_lowercase();
    s.ends_with("tools are") || s.ends_with("tool is")
}

fn is_numbered_item(t: &str) -> Option<&str> {
    let d = t.chars().take_while(|c| c.is_ascii_digit()).count();
    if d == 0 {
        return None;
    }
    t[d..].strip_prefix(". ")
}

/// Names of a run that starts at `toks[i]`: `a`, `b` and `c`. Returns the indexes of
/// the names and the index of the last name.
fn run_at(toks: &[(usize, Tok)], i: usize) -> (Vec<usize>, usize) {
    let mut names = vec![i];
    let mut j = i;
    while let (Some((_, Tok::Text(sep))), Some((_, Tok::Code(c)))) =
        (toks.get(j + 1), toks.get(j + 2))
    {
        if !is_list_sep(sep) || !is_ident(c) {
            break;
        }
        names.push(j + 2);
        j += 2;
    }
    (names, j)
}

/// The words that a document names as tools, with their line numbers and the reason.
fn tool_claims(doc: &Doc) -> Vec<(usize, String, &'static str)> {
    let mut claims = Vec::new();
    let lines = prose_lines(&doc.text);

    // Numbered lists under a line that says "tool".
    let mut intro = String::new();
    for (n, line) in &lines {
        let t = line.trim_start();
        if t.is_empty() {
            continue;
        }
        if let Some(item) = is_numbered_item(t) {
            if intro.to_lowercase().contains("tool") {
                let toks: Vec<(usize, Tok)> = spans(item).into_iter().map(|t| (*n, t)).collect();
                if let Some((_, Tok::Code(c))) = toks.first() {
                    if is_ident(c) {
                        let (names, _) = run_at(&toks, 0);
                        for k in names {
                            if let (_, Tok::Code(c)) = &toks[k] {
                                claims.push((*n, c.clone(), "it starts a list of tool calls"));
                            }
                        }
                    }
                }
            }
        } else if !line.starts_with([' ', '\t']) {
            intro = t.to_string();
        }
    }

    // All spans of the document, with a space for each line break.
    let mut toks: Vec<(usize, Tok)> = Vec::new();
    for (n, line) in &lines {
        for t in spans(line).into_iter().chain([Tok::Text(" ".into())]) {
            match (toks.last_mut(), t) {
                (Some((_, Tok::Text(a))), Tok::Text(b)) => a.push_str(&b),
                (_, t) => toks.push((*n, t)),
            }
        }
    }
    let mut i = 0;
    while i < toks.len() {
        let (n, Tok::Code(c)) = &toks[i] else {
            i += 1;
            continue;
        };
        if !is_ident(c) {
            i += 1;
            continue;
        }
        // "`export_step` and `lamina export-step`"
        if let (Some((_, Tok::Text(sep))), Some((_, Tok::Code(next)))) =
            (toks.get(i + 1), toks.get(i + 2))
        {
            if norm(sep) == "and" && next.starts_with("lamina ") {
                claims.push((*n, c.clone(), "it is next to a CLI command"));
            }
        }
        let (names, last) = run_at(&toks, i);
        let after = matches!(toks.get(last + 1), Some((_, Tok::Text(t))) if starts_with_tool(t));
        let before = i > 0 && matches!(&toks[i - 1], (_, Tok::Text(t)) if ends_with_tools_are(t));
        if after || before {
            for k in &names {
                if let (n, Tok::Code(c)) = &toks[*k] {
                    claims.push((*n, c.clone(), "the text calls it a tool"));
                }
            }
        }
        i = last + 1;
    }
    claims
}

/// Each tool that the document names exists. Each word in backticks with an underscore
/// is a known name.
fn check_tool_names(doc: &Doc, f: &Facts) -> Vec<String> {
    let mut errs = Vec::new();
    for (n, w, why) in tool_claims(doc) {
        if !f.tools.contains(&w) {
            errs.push(format!(
                "{}:{n}: `{w}` is not a tool ({why}). Use the name of a real tool, \
                 or change the text so that it does not call `{w}` a tool.",
                doc.path
            ));
        }
    }
    for (n, line) in prose_lines(&doc.text) {
        for t in spans(&line) {
            let Tok::Code(w) = t else { continue };
            if is_ident(&w)
                && w.contains('_')
                && !f.tools.contains(&w)
                && !f.known.contains(&w)
                && !OTHER_WORDS.contains(&w.as_str())
            {
                errs.push(format!(
                    "{}:{n}: `{w}` is not a tool, a tool parameter, a project file field or \
                     a DRC check. If it is a tool, use the real name. If it is another word, \
                     add it to OTHER_WORDS in {THIS_FILE}.",
                    doc.path
                ));
            }
        }
    }
    errs
}

/// Each tool in the README table "What the agent can do" exists.
fn check_tool_table(doc: &Doc, f: &Facts) -> Vec<String> {
    let mut errs = Vec::new();
    let lines = prose_lines(&doc.text);
    let Some(start) = lines
        .iter()
        .position(|(_, l)| l.trim() == "## What the agent can do")
    else {
        return vec![format!(
            "{}: no heading \"## What the agent can do\". If it has a new name, change \
             check_tool_table in {THIS_FILE}.",
            doc.path
        )];
    };
    let mut rows = 0;
    for (n, line) in lines[start + 1..]
        .iter()
        .skip_while(|(_, l)| !l.starts_with('|'))
        .take_while(|(_, l)| l.starts_with('|'))
        .skip(2)
    {
        rows += 1;
        for t in spans(line) {
            let Tok::Code(w) = t else { continue };
            let names = match w.strip_suffix("(s)") {
                Some(base) => vec![base.to_string(), format!("{base}s")],
                None => vec![w.clone()],
            };
            for name in names {
                if !f.tools.contains(&name) {
                    errs.push(format!(
                        "{}:{n}: the table \"What the agent can do\" names `{name}`, but there \
                         is no such tool. Use the name of a real tool.",
                        doc.path
                    ));
                }
            }
        }
    }
    if rows == 0 {
        errs.push(format!(
            "{}: no table under \"## What the agent can do\"",
            doc.path
        ));
    }
    errs
}

/// The places where the document says "N tools" or "N MCP tools".
fn tool_counts(doc: &Doc) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    for (n, line) in prose_lines(&doc.text) {
        let words: Vec<String> = line
            .split_whitespace()
            .map(|w| w.trim_matches(|c: char| !c.is_alphanumeric()).to_string())
            .collect();
        for (i, w) in words.iter().enumerate() {
            let Ok(count) = w.parse::<usize>() else {
                continue;
            };
            let next = words.get(i + 1).map(String::as_str);
            let after = words.get(i + 2).map(String::as_str);
            if next == Some("tools") || (next == Some("MCP") && after == Some("tools")) {
                out.push((n, count));
            }
        }
    }
    out
}

fn check_tool_count(doc: &Doc, f: &Facts) -> Vec<String> {
    let real = f.tools.len();
    tool_counts(doc)
        .into_iter()
        .filter(|(_, c)| *c != real)
        .map(|(n, c)| {
            format!(
                "{}:{n}: the text says {c} tools, but the server has {real}. \
                 Change {c} to {real}.",
                doc.path
            )
        })
        .collect()
}

/// Each `lamina <subcommand>` and `lamina-viewer <subcommand>` is real. Code blocks
/// are checked too.
fn check_cli(doc: &Doc, f: &Facts) -> Vec<String> {
    let mut errs = Vec::new();
    for (i, line) in doc.text.lines().enumerate() {
        let n = i + 1;
        let mut pos = 0;
        while let Some(off) = line[pos..].find("lamina") {
            let s = pos + off;
            let e = s + "lamina".len();
            pos = e;
            // The program name stands alone, or at the end of a path.
            if s > 0 && !line[..s].ends_with([' ', '\t', '`', '/', '(', '>', '"']) {
                continue;
            }
            // A path in an environment variable (`LAMINA=target/release/lamina`) is a
            // value, not a command: the next word is the command.
            let word_start = line[..s]
                .rfind([' ', '\t', '`', '(', '>', '"'])
                .map_or(0, |i| i + 1);
            if line[word_start..s].contains('=') {
                continue;
            }
            let (prog, rest) = if let Some(r) = line[e..].strip_prefix("-viewer ") {
                ("lamina-viewer", r)
            } else if let Some(r) = line[e..].strip_prefix(' ') {
                ("lamina", r)
            } else {
                continue;
            };
            let word: String = rest
                .chars()
                .take_while(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | '/'))
                .collect();
            let word = word.trim_end_matches('.');
            // Flags, placeholders, paths and other text are not subcommands.
            if !word.starts_with(|c: char| c.is_ascii_lowercase()) || word.contains(['/', '.']) {
                continue;
            }
            if prog == "lamina" && !f.lamina.contains(word) {
                errs.push(format!(
                    "{}:{n}: `lamina {word}`: there is no such subcommand. The subcommands \
                     are: {}.",
                    doc.path,
                    f.lamina.iter().cloned().collect::<Vec<_>>().join(", ")
                ));
            }
            if prog == "lamina-viewer"
                && !f.viewer.contains(word)
                && !EXAMPLE_BOARDS.contains(&word)
            {
                errs.push(format!(
                    "{}:{n}: `lamina-viewer {word}`: there is no such subcommand. The \
                     subcommands are: {}. If `{word}` is a board folder, add it to \
                     EXAMPLE_BOARDS in {THIS_FILE}.",
                    doc.path,
                    f.viewer.iter().cloned().collect::<Vec<_>>().join(", ")
                ));
            }
        }
    }
    errs
}

/// The anchor of a heading, as GitHub makes it.
fn slug(heading: &str) -> String {
    // [text](url) becomes text.
    let mut text = String::new();
    let mut rest = heading;
    while let Some(a) = rest.find('[') {
        let Some(b) = rest[a..].find("](") else { break };
        let Some(c) = rest[a + b..].find(')') else {
            break;
        };
        text.push_str(&rest[..a]);
        text.push_str(&rest[a + 1..a + b]);
        rest = &rest[a + b + c + 1..];
    }
    text.push_str(rest);
    text.trim()
        .to_lowercase()
        .chars()
        .filter_map(|c| match c {
            ' ' => Some('-'),
            c if c.is_alphanumeric() || c == '-' || c == '_' => Some(c),
            _ => None,
        })
        .collect()
}

/// The anchors of the headings of a Markdown text.
fn anchors(text: &str) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    let mut seen: std::collections::HashMap<String, usize> = Default::default();
    for (_, line) in prose_lines(text) {
        let t = line.trim();
        let heading = if t.starts_with('#') {
            let h = t.trim_start_matches('#');
            if !h.starts_with(' ') || t.len() - h.len() > 6 {
                continue;
            }
            h.trim().to_string()
        } else if let Some(h) = (1..=6).find_map(|k| {
            let open = t.strip_prefix(&format!("<h{k}"))?;
            let inner = &open[open.find('>')? + 1..];
            Some(inner[..inner.find(&format!("</h{k}>"))?].to_string())
        }) {
            h
        } else {
            continue;
        };
        let s = slug(&heading);
        let k = seen.entry(s.clone()).or_insert(0);
        out.insert(if *k == 0 {
            s.clone()
        } else {
            format!("{s}-{k}")
        });
        *k += 1;
    }
    out
}

/// The targets of the links and images of a line: `[a](t)`, `![a](t)`, `src="t"`,
/// `href="t"`. Code spans are left out.
fn link_targets(line: &str) -> Vec<String> {
    let text: String = spans(line)
        .into_iter()
        .filter_map(|t| match t {
            Tok::Text(s) => Some(s),
            Tok::Code(_) => None,
        })
        .collect::<Vec<_>>()
        .join(" ");
    let mut out = Vec::new();
    let mut rest = text.as_str();
    while let Some(a) = rest.find("](") {
        let r = &rest[a + 2..];
        let Some(b) = r.find(')') else { break };
        let t = r[..b].split_whitespace().next().unwrap_or("");
        out.push(t.trim_matches(['<', '>']).to_string());
        rest = &r[b..];
    }
    for attr in ["src=\"", "href=\""] {
        let mut rest = text.as_str();
        while let Some(a) = rest.find(attr) {
            let r = &rest[a + attr.len()..];
            let Some(b) = r.find('"') else { break };
            out.push(r[..b].to_string());
            rest = &r[b..];
        }
    }
    out
}

/// Each relative link and image points to a file that exists. Each anchor is a heading.
fn check_links(doc: &Doc, root: &Path) -> Vec<String> {
    let mut errs = Vec::new();
    let dir = Path::new(&doc.path).parent().unwrap_or(Path::new(""));
    for (n, line) in prose_lines(&doc.text) {
        for t in link_targets(&line) {
            if t.is_empty() || t.contains("://") || t.starts_with("mailto:") {
                continue;
            }
            let (file, anchor) = match t.split_once('#') {
                Some((f, a)) => (f, Some(a)),
                None => (t.as_str(), None),
            };
            let (rel, text) = if file.is_empty() {
                (doc.path.clone(), Some(doc.text.clone()))
            } else {
                let path = root.join(dir).join(file);
                if !path.exists() {
                    errs.push(format!(
                        "{}:{n}: the link `{t}` points to {}, which does not exist. Fix \
                         the path (it is relative to {}).",
                        doc.path,
                        dir.join(file).display(),
                        if dir.as_os_str().is_empty() {
                            "the repository root".to_string()
                        } else {
                            dir.display().to_string()
                        }
                    ));
                    continue;
                }
                let text = path
                    .extension()
                    .is_some_and(|e| e == "md")
                    .then(|| std::fs::read_to_string(&path).ok())
                    .flatten();
                (dir.join(file).display().to_string(), text)
            };
            if let (Some(a), Some(text)) = (anchor, text) {
                if !anchors(&text).contains(a) {
                    errs.push(format!(
                        "{}:{n}: the link `{t}` points to #{a}, but {rel} has no heading \
                         with that anchor. Fix the anchor or the heading.",
                        doc.path
                    ));
                }
            }
        }
    }
    errs
}

fn report(errs: Vec<String>, what: &str) {
    assert!(
        errs.is_empty(),
        "\nThe documentation does not agree with {what}:\n\n{}\n\nThe checks are in {THIS_FILE}.\n",
        errs.join("\n")
    );
}

#[test]
fn readme_tool_table() {
    let f = facts();
    let readme = docs(&repo_root())
        .into_iter()
        .find(|d| d.path == "README.md")
        .unwrap();
    report(check_tool_table(&readme, &f), "the tools of the server");
}

#[test]
fn tool_count() {
    let f = facts();
    let docs = docs(&repo_root());
    let readme = docs.iter().find(|d| d.path == "README.md").unwrap();
    assert!(
        !tool_counts(readme).is_empty(),
        "README.md does not say how many tools there are (\"73 tools\"). If that is on \
         purpose, remove this assert from {THIS_FILE}."
    );
    report(
        docs.iter().flat_map(|d| check_tool_count(d, &f)).collect(),
        "the number of tools",
    );
}

#[test]
fn tool_names() {
    let f = facts();
    report(
        docs(&repo_root())
            .iter()
            .flat_map(|d| check_tool_names(d, &f))
            .collect(),
        "the tools of the server",
    );
}

#[test]
fn cli_subcommands() {
    let f = facts();
    report(
        docs(&repo_root())
            .iter()
            .flat_map(|d| check_cli(d, &f))
            .collect(),
        "the CLI subcommands",
    );
}

#[test]
fn links() {
    let root = repo_root();
    report(
        docs(&root)
            .iter()
            .flat_map(|d| check_links(d, &root))
            .collect(),
        "the files in the repository",
    );
}

/// OTHER_WORDS stays small: each word is in a document and is not a known name.
#[test]
fn other_words_are_needed() {
    let f = facts();
    let docs = docs(&repo_root());
    for w in OTHER_WORDS {
        assert!(
            !f.tools.contains(*w) && !f.known.contains(*w),
            "`{w}` is a known name now. Remove it from OTHER_WORDS in {THIS_FILE}."
        );
        assert!(
            docs.iter().any(|d| d.text.contains(&format!("`{w}`"))),
            "no document has `{w}`. Remove it from OTHER_WORDS in {THIS_FILE}."
        );
    }
}

#[test]
fn checks_find_the_documents() {
    let paths: Vec<String> = docs(&repo_root()).into_iter().map(|d| d.path).collect();
    for p in ["README.md", "docs/guide.md", "examples/c3-sensor/README.md"] {
        assert!(
            paths.iter().any(|x| x == p),
            "{p} is not checked: {paths:?}"
        );
    }
}

#[test]
fn slugs_like_github() {
    assert_eq!(
        slug("Copy the layout of a block"),
        "copy-the-layout-of-a-block"
    );
    assert_eq!(slug("What the agent can do"), "what-the-agent-can-do");
    assert_eq!(
        slug("`lamina` and the [guide](docs/guide.md)!"),
        "lamina-and-the-guide"
    );
    let a = anchors("# A\n## A\n```\n# B\n```\n<h1 align=\"center\">Lamina</h1>\n");
    assert_eq!(
        a,
        ["a", "a-1", "lamina"]
            .map(String::from)
            .into_iter()
            .collect()
    );
}

const BAD_DOC: &str = "\
<h1>Sample</h1>

`lamina` is an MCP server with 99999 tools.

## What the agent can do

| | Tools |
|---|---|
| **Parts** | `search_parts`, `import_part(s)`, `make_coffee` |

The `teleport` tool moves the board. The tools are `open` and `fly_away`.
`frobnicate_board` takes a board. `export_step` and `lamina export-step <dir>` work.
`launch_rocket` and `lamina launch` do the same.

An agent calls the tools in this order:

1. `open`
2. `jump`, then `check`

```
lamina explode boards/x
lamina-viewer warp
target/release/lamina vanish
X=1 target/release/lamina melt
```

See [the guide](docs/no-such-file.md), ![a](docs/images/nope.png), [b](#no-such-heading),
[c](docs/guide.md#no-such-section) and <img src=\"docs/images/missing.png\">.
";

const GOOD_DOC: &str = "\
# Sample

`lamina` is an MCP server with {N} tools.

## What the agent can do

| | Tools |
|---|---|
| **Parts** | `search_parts`, `import_part(s)` |

The `notes` tool reads the notes. The tools are `open` and `check`. `pin_role` and
`silk_to_pad` are checks. `length_min` is a field. `export_step` and `lamina export-step`.

```
lamina mcp
lamina-viewer shot <board> --out x.png
lamina-viewer my-board
target/release/lamina --version
LAMINA=target/release/lamina python3 examples/c3-sensor/test_build.py
env LAMINA=/opt/lamina cargo test
local lamina = import 'lamina.libsonnet';
```

See [the guide](docs/guide.md#viewer), [the top](#sample), [tools](#what-the-agent-can-do)
and <img src=\"docs/images/demo.gif\">. [Web](https://example.com), [mail](mailto:a@b.c).
";

fn sample(text: &str) -> Doc {
    Doc {
        path: "README.md".into(),
        text: text.into(),
    }
}

#[test]
fn good_sample_passes() {
    let f = facts();
    let d = sample(&GOOD_DOC.replace("{N}", &f.tools.len().to_string()));
    let root = repo_root();
    let errs: Vec<String> = [
        check_tool_table(&d, &f),
        check_tool_count(&d, &f),
        check_tool_names(&d, &f),
        check_cli(&d, &f),
        check_links(&d, &root),
    ]
    .concat();
    assert!(errs.is_empty(), "{errs:#?}");
}

#[test]
fn bad_sample_fails() {
    let f = facts();
    let d = sample(BAD_DOC);
    let root = repo_root();
    let has = |errs: &[String], what: &str| {
        assert!(
            errs.iter().any(|e| e.contains(what)),
            "no error for {what}: {errs:#?}"
        );
    };

    let table = check_tool_table(&d, &f);
    has(&table, "`make_coffee`");
    assert_eq!(table.len(), 1, "{table:#?}");

    let count = check_tool_count(&d, &f);
    has(
        &count,
        &format!("says 99999 tools, but the server has {}", f.tools.len()),
    );
    assert_eq!(count.len(), 1, "{count:#?}");

    let names = check_tool_names(&d, &f);
    for w in [
        "`teleport` is not a tool",
        "`fly_away` is not a tool",
        "`frobnicate_board` is not a tool, a tool parameter",
        "`launch_rocket` is not a tool (it is next to a CLI command)",
        "`jump` is not a tool (it starts a list of tool calls)",
        "`make_coffee` is not a tool, a tool parameter",
    ] {
        has(&names, w);
    }
    assert!(
        !names
            .iter()
            .any(|e| e.contains("`open`") || e.contains("`check`"))
    );

    let cli = check_cli(&d, &f);
    has(&cli, "`lamina launch`");
    has(&cli, "`lamina explode`");
    has(&cli, "`lamina-viewer warp`");
    // A path to the program is still checked, also after an assignment word.
    has(&cli, "`lamina vanish`");
    has(&cli, "`lamina melt`");
    assert_eq!(cli.len(), 5, "{cli:#?}");

    let links = check_links(&d, &root);
    has(&links, "docs/no-such-file.md");
    has(&links, "docs/images/nope.png");
    has(&links, "#no-such-heading");
    has(&links, "#no-such-section");
    has(&links, "docs/images/missing.png");
    assert_eq!(links.len(), 5, "{links:#?}");
}
