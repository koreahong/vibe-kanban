//! Bidirectional converter between Atlassian Document Format (ADF) and Markdown.

use serde_json::Value;

// --- ADF → Markdown ---

pub fn adf_to_markdown(adf: &Value) -> String {
    let content = match adf.get("content").and_then(|c| c.as_array()) {
        Some(arr) => arr,
        None => return String::new(),
    };
    content.iter().map(node_to_md).collect::<Vec<_>>().join("\n")
}

fn node_to_md(node: &Value) -> String {
    let node_type = node.get("type").and_then(|t| t.as_str()).unwrap_or("");
    match node_type {
        "paragraph" => {
            format!("{}\n", inline_to_md(node.get("content")))
        }
        "heading" => {
            let level = node
                .get("attrs")
                .and_then(|a| a.get("level"))
                .and_then(|l| l.as_u64())
                .unwrap_or(1) as usize;
            format!(
                "{} {}\n",
                "#".repeat(level),
                inline_to_md(node.get("content"))
            )
        }
        "bulletList" => {
            let items = node
                .get("content")
                .and_then(|c| c.as_array())
                .cloned()
                .unwrap_or_default();
            let result: String = items
                .iter()
                .map(|li| list_item_to_md(li, "- "))
                .collect();
            format!("{result}\n")
        }
        "orderedList" => {
            let items = node
                .get("content")
                .and_then(|c| c.as_array())
                .cloned()
                .unwrap_or_default();
            let result: String = items
                .iter()
                .enumerate()
                .map(|(i, li)| list_item_to_md(li, &format!("{}. ", i + 1)))
                .collect();
            format!("{result}\n")
        }
        "codeBlock" => {
            let lang = node
                .get("attrs")
                .and_then(|a| a.get("language"))
                .and_then(|l| l.as_str())
                .unwrap_or("");
            let code = inline_to_md(node.get("content"));
            format!("```{lang}\n{code}\n```\n")
        }
        "blockquote" => {
            let inner = node
                .get("content")
                .and_then(|c| c.as_array())
                .map(|arr| arr.iter().map(node_to_md).collect::<String>())
                .unwrap_or_default();
            let quoted: String = inner
                .lines()
                .filter(|l| !l.is_empty())
                .map(|l| format!("> {l}"))
                .collect::<Vec<_>>()
                .join("\n");
            format!("{quoted}\n")
        }
        "rule" => "---\n".to_string(),
        _ => {
            if let Some(content) = node.get("content").and_then(|c| c.as_array()) {
                content.iter().map(node_to_md).collect()
            } else {
                String::new()
            }
        }
    }
}

fn list_item_to_md(li: &Value, prefix: &str) -> String {
    let inner = li
        .get("content")
        .and_then(|c| c.as_array())
        .map(|arr| arr.iter().map(node_to_md).collect::<String>())
        .unwrap_or_default();
    format!("{prefix}{}\n", inner.trim())
}

fn inline_to_md(content: Option<&Value>) -> String {
    let arr = match content.and_then(|c| c.as_array()) {
        Some(a) => a,
        None => return String::new(),
    };

    arr.iter()
        .map(|node| {
            let node_type = node.get("type").and_then(|t| t.as_str()).unwrap_or("");
            match node_type {
                "text" => {
                    let mut text = node
                        .get("text")
                        .and_then(|t| t.as_str())
                        .unwrap_or("")
                        .to_string();

                    if let Some(marks) = node.get("marks").and_then(|m| m.as_array()) {
                        for mark in marks {
                            let mark_type =
                                mark.get("type").and_then(|t| t.as_str()).unwrap_or("");
                            match mark_type {
                                "strong" => text = format!("**{text}**"),
                                "em" => text = format!("*{text}*"),
                                "code" => text = format!("`{text}`"),
                                "strike" => text = format!("~~{text}~~"),
                                "link" => {
                                    let href = mark
                                        .get("attrs")
                                        .and_then(|a| a.get("href"))
                                        .and_then(|h| h.as_str())
                                        .unwrap_or("");
                                    text = format!("[{text}]({href})");
                                }
                                _ => {}
                            }
                        }
                    }
                    text
                }
                "hardBreak" => "\n".to_string(),
                "mention" => {
                    let name = node
                        .get("attrs")
                        .and_then(|a| a.get("text"))
                        .and_then(|t| t.as_str())
                        .unwrap_or("user");
                    format!("@{name}")
                }
                "emoji" => node
                    .get("attrs")
                    .and_then(|a| a.get("shortName"))
                    .and_then(|s| s.as_str())
                    .unwrap_or("")
                    .to_string(),
                _ => String::new(),
            }
        })
        .collect()
}

// --- Markdown → ADF ---

pub fn markdown_to_adf(md: &str) -> Value {
    if md.trim().is_empty() {
        return serde_json::json!({
            "version": 1,
            "type": "doc",
            "content": [{"type": "paragraph", "content": []}]
        });
    }

    let lines: Vec<&str> = md.lines().collect();
    let mut content = Vec::new();
    let mut i = 0;

    while i < lines.len() {
        let line = lines[i];

        // Empty line
        if line.trim().is_empty() {
            i += 1;
            continue;
        }

        // Heading
        if let Some(rest) = line.strip_prefix('#') {
            let level = 1 + rest.chars().take_while(|&c| c == '#').count();
            let text = line[level..].trim_start_matches('#').trim();
            content.push(serde_json::json!({
                "type": "heading",
                "attrs": {"level": level},
                "content": parse_inline(text)
            }));
            i += 1;
            continue;
        }

        // Code block
        if line.starts_with("```") {
            let lang = line[3..].trim();
            let mut code_lines = Vec::new();
            i += 1;
            while i < lines.len() && !lines[i].starts_with("```") {
                code_lines.push(lines[i]);
                i += 1;
            }
            i += 1; // skip closing ```
            let mut block = serde_json::json!({
                "type": "codeBlock",
                "content": [{"type": "text", "text": code_lines.join("\n")}]
            });
            if !lang.is_empty() {
                block["attrs"] = serde_json::json!({"language": lang});
            }
            content.push(block);
            continue;
        }

        // Horizontal rule
        if line.trim().starts_with("---") && line.trim().chars().all(|c| c == '-') {
            content.push(serde_json::json!({"type": "rule"}));
            i += 1;
            continue;
        }

        // Blockquote
        if line.starts_with("> ") {
            let mut quote_lines = Vec::new();
            while i < lines.len() && lines[i].starts_with("> ") {
                quote_lines.push(&lines[i][2..]);
                i += 1;
            }
            content.push(serde_json::json!({
                "type": "blockquote",
                "content": [{
                    "type": "paragraph",
                    "content": parse_inline(&quote_lines.join("\n"))
                }]
            }));
            continue;
        }

        // Bullet list
        if line.starts_with("- ") || line.starts_with("* ") {
            let mut items = Vec::new();
            while i < lines.len()
                && (lines[i].starts_with("- ") || lines[i].starts_with("* "))
            {
                let text = &lines[i][2..];
                items.push(serde_json::json!({
                    "type": "listItem",
                    "content": [{"type": "paragraph", "content": parse_inline(text)}]
                }));
                i += 1;
            }
            content.push(serde_json::json!({
                "type": "bulletList",
                "content": items
            }));
            continue;
        }

        // Ordered list
        if line.len() > 2 && line.chars().next().map_or(false, |c| c.is_ascii_digit()) {
            if let Some(pos) = line.find(". ") {
                if line[..pos].chars().all(|c| c.is_ascii_digit()) {
                    let mut items = Vec::new();
                    while i < lines.len() {
                        let l = lines[i];
                        if let Some(p) = l.find(". ") {
                            if l[..p].chars().all(|c| c.is_ascii_digit()) {
                                let text = &l[p + 2..];
                                items.push(serde_json::json!({
                                    "type": "listItem",
                                    "content": [{"type": "paragraph", "content": parse_inline(text)}]
                                }));
                                i += 1;
                                continue;
                            }
                        }
                        break;
                    }
                    content.push(serde_json::json!({
                        "type": "orderedList",
                        "content": items
                    }));
                    continue;
                }
            }
        }

        // Paragraph (default)
        content.push(serde_json::json!({
            "type": "paragraph",
            "content": parse_inline(line)
        }));
        i += 1;
    }

    serde_json::json!({
        "version": 1,
        "type": "doc",
        "content": content
    })
}

fn parse_inline(text: &str) -> Value {
    // Simple implementation: just return text node
    // A full implementation would parse bold, italic, code, links, etc.
    if text.is_empty() {
        return serde_json::json!([]);
    }

    // For now, return the text as a single text node.
    // Bold/italic/code patterns are preserved as markdown in ADF text.
    serde_json::json!([{"type": "text", "text": text}])
}
