//! Clean-room HTML-to-text extractor stripping boilerplate tags while preserving structure.

/// Converts HTML into readable plain text, stripping `<script>`, `<style>`, and markup tags.
pub fn html_to_text(html: &str) -> String {
    let mut out = String::with_capacity(html.len() / 2);
    let bytes = html.as_bytes();
    let mut i = 0usize;
    let mut skip_until_tag: Option<&str> = None;

    while i < bytes.len() {
        if bytes[i] == b'<' {
            let Some(close_rel) = html[i..].find('>') else {
                break;
            };
            let tag_inner = html[i + 1..i + close_rel].trim();
            let lower = tag_inner.to_ascii_lowercase();
            let tag_name = lower
                .trim_start_matches('/')
                .split(|c: char| c.is_whitespace() || c == '/')
                .next()
                .unwrap_or("");

            if let Some(active_skip) = skip_until_tag {
                if lower.starts_with('/') && tag_name == active_skip {
                    skip_until_tag = None;
                }
                i += close_rel + 1;
                continue;
            }

            if !lower.starts_with('/') {
                let skip_tag = match tag_name {
                    "script" => Some("script"),
                    "style" => Some("style"),
                    "noscript" => Some("noscript"),
                    "svg" => Some("svg"),
                    "template" => Some("template"),
                    _ => None,
                };
                if let Some(tag) = skip_tag {
                    if !lower.ends_with('/') {
                        skip_until_tag = Some(tag);
                    }
                    i += close_rel + 1;
                    continue;
                }
            }

            if matches!(
                tag_name,
                "h1" | "h2"
                    | "h3"
                    | "h4"
                    | "h5"
                    | "h6"
                    | "p"
                    | "div"
                    | "li"
                    | "tr"
                    | "br"
                    | "hr"
                    | "pre"
                    | "blockquote"
                    | "section"
                    | "article"
            ) && !out.ends_with('\n')
            {
                out.push('\n');
            }
            i += close_rel + 1;
        } else if skip_until_tag.is_some() {
            let next = html[i..]
                .find('<')
                .map(|rel| i + rel)
                .unwrap_or(bytes.len());
            i = next;
        } else {
            let next = html[i..]
                .find('<')
                .map(|rel| i + rel)
                .unwrap_or(bytes.len());
            let decoded = decode_entities(&html[i..next]);
            out.push_str(&decoded);
            i = next;
        }
    }

    let mut cleaned = Vec::new();
    for line in out.lines() {
        let collapsed: String = line.split_whitespace().collect::<Vec<_>>().join(" ");
        if !collapsed.is_empty() {
            cleaned.push(collapsed);
        }
    }
    cleaned.join("\n")
}

fn decode_entities(text: &str) -> String {
    text.replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&apos;", "'")
        .replace("&nbsp;", " ")
}
