//! Minimal RFC 4180 CSV reader/writer (password export/import).

/// Quotes a field when needed.
pub fn field(s: &str) -> String {
    if s.contains([',', '"', '\n', '\r']) || s.starts_with(' ') || s.ends_with(' ') {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s.to_string()
    }
}

pub fn row(fields: &[&str]) -> String {
    let mut out = fields.iter().map(|f| field(f)).collect::<Vec<_>>().join(",");
    out.push_str("\r\n");
    out
}

/// Parses CSV text into rows of fields. Handles quoted fields with embedded
/// commas, quotes and newlines; tolerates a UTF-8 BOM and `\n` or `\r\n`.
pub fn parse(text: &str) -> Vec<Vec<String>> {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let mut rows = Vec::new();
    let mut row: Vec<String> = Vec::new();
    let mut cur = String::new();
    let mut in_quotes = false;
    let mut chars = text.chars().peekable();
    let mut row_has_content = false;
    while let Some(c) = chars.next() {
        if in_quotes {
            match c {
                '"' if chars.peek() == Some(&'"') => {
                    cur.push('"');
                    chars.next();
                }
                '"' => in_quotes = false,
                _ => cur.push(c),
            }
            continue;
        }
        match c {
            '"' => {
                in_quotes = true;
                row_has_content = true;
            }
            ',' => {
                row.push(std::mem::take(&mut cur));
                row_has_content = true;
            }
            '\r' => {}
            '\n' => {
                if row_has_content || !cur.is_empty() {
                    row.push(std::mem::take(&mut cur));
                    rows.push(std::mem::take(&mut row));
                }
                row_has_content = false;
            }
            _ => {
                cur.push(c);
                row_has_content = true;
            }
        }
    }
    if row_has_content || !cur.is_empty() {
        row.push(cur);
        rows.push(row);
    }
    rows
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip() {
        let fields = ["plain", "has,comma", "has \"quote\"", "multi\nline", "", " padded "];
        let text = row(&fields) + &row(&["a", "b", "c", "d", "e", "f"]);
        let parsed = parse(&text);
        assert_eq!(parsed.len(), 2);
        assert_eq!(parsed[0], fields.iter().map(|s| s.to_string()).collect::<Vec<_>>());
    }

    #[test]
    fn tolerant_parsing() {
        let parsed = parse("\u{feff}a,b\n\n1,\"2\"\n3,4");
        assert_eq!(parsed, vec![vec!["a", "b"], vec!["1", "2"], vec!["3", "4"]]);
        assert_eq!(parse(",\n"), vec![vec!["", ""]]);
        assert!(parse("").is_empty());
    }
}
