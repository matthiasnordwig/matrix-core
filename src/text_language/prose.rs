//! The part of a text that is prose: what language detection (`text_language.rs`) counts. Address
//! headers of a mail (`From:`, `To:`, `Cc:` …, also as Markdown `**To:**`, with their wrapped
//! continuation lines), e-mail addresses, URLs, domains (`enron.com`, `www.…`) and paths drop out —
//! a mail with a long distribution list has `com` dozens of times and was taken for Portuguese.

/// Labels of address headers (lower case, without the colon), English, German, French, Spanish.
const ADDRESS_LABELS: &[&str] = &[
    "from", "to", "cc", "bcc", "reply-to", "sender", "von", "an", "kopie", "blindkopie", "de", "à", "pour", "para", "copia",
];
/// Last labels that make a dotted word a domain, not a sentence run together (`end.Die`).
const TLDS: &[&str] = &[
    "com", "net", "org", "edu", "gov", "int", "info", "biz", "io", "co", "eu", "uk", "de", "at", "ch", "fr", "es", "it", "nl", "pt", "br", "us", "ca", "ru", "cn", "jp",
];

/// At most `max_chars` characters of `text` without the non-prose parts, lines joined by a newline.
pub(super) fn prose(text: &str, max_chars: usize) -> String {
    let mut out = String::new();
    let mut in_header = false;
    for line in text.lines() {
        if out.chars().count() >= max_chars {
            break;
        }
        let wrapped = line.starts_with([' ', '\t']);
        if in_header && wrapped {
            continue;
        }
        in_header = is_address_header(line);
        if in_header {
            continue;
        }
        for word in line.split_whitespace().filter(|w| !is_non_prose(w)) {
            out.push_str(word);
            out.push(' ');
        }
        out.push('\n');
    }
    out
}

/// `From: …`, `**To:** …`, `> cc: …`: an address label at the line start, bold marks and quote
/// marks aside.
fn is_address_header(line: &str) -> bool {
    let s = line.trim_start_matches(|c: char| c.is_whitespace() || matches!(c, '*' | '_' | '>'));
    let Some((label, _)) = s.split_once(':') else { return false };
    let label = label.trim_end_matches(['*', '_']).to_lowercase();
    ADDRESS_LABELS.contains(&label.as_str()) || label.starts_with("x-")
}

fn is_non_prose(word: &str) -> bool {
    let w = word.trim_matches(|c: char| matches!(c, '<' | '>' | '(' | ')' | '[' | ']' | '{' | '}' | '"' | '\'' | ',' | ';' | ':' | '*' | '_' | '.'));
    w.contains('@') || w.contains("://") || is_path(w) || is_domain(w)
}

fn is_path(w: &str) -> bool {
    w.starts_with(['/', '\\']) && w.len() > 1
        || w.starts_with("~/")
        || w.starts_with("./")
        || w.starts_with("../")
        || w.as_bytes().get(1..3) == Some(b":\\")
        || w.matches('/').count() >= 2
}

/// `www.x`, or dotted labels of letters, digits and hyphens ending in a known top-level domain.
fn is_domain(w: &str) -> bool {
    let labels: Vec<&str> = w.split('.').collect();
    labels.len() >= 2
        && labels.iter().all(|l| !l.is_empty() && l.chars().all(|c| c.is_alphanumeric() || c == '-'))
        && (labels[0].eq_ignore_ascii_case("www") || TLDS.contains(&labels[labels.len() - 1].to_lowercase().as_str()))
}
