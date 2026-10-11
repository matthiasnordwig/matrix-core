//! Repairs PDF text whose characters were written as UTF-8 but decoded as
//! Windows-1252 by the PDF producer ("â€‰" instead of a thin space,
//! "durchÂ­zuführen" instead of a soft hyphen). The PDF's own character map is
//! wrong, so every extractor returns the same garbage; this is the one place
//! that undoes it, applied per extracted line before offsets are computed.
//!
//! Only sequences that decode to a narrow, plausible target range are
//! replaced (2-byte leads C2–C5 → U+0080–U+017F, 3-byte lead E2 →
//! U+2000–U+2FFF): genuine German text like „Fuß“ (ß + closing quote would
//! otherwise decode as an NKo letter) stays untouched. Afterwards soft hyphens
//! are dropped, typographic spaces become plain spaces, and a stray word-
//! internal or word-final "Â" (left over when x-sorting moved it away from its
//! soft hyphen, "WiederÂa­ufbau") is removed; a sequence whose three glyphs
//! x-sorting reversed ("‰€â") is decoded as well.

/// Windows-1252 byte for `c`, if `c` is in the code page's upper half.
fn cp1252_byte(c: char) -> Option<u8> {
    let u = c as u32;
    if (0xA0..=0xFF).contains(&u) {
        return Some(u as u8);
    }
    let b = match c {
        '€' => 0x80, '‚' => 0x82, 'ƒ' => 0x83, '„' => 0x84, '…' => 0x85, '†' => 0x86,
        '‡' => 0x87, 'ˆ' => 0x88, '‰' => 0x89, 'Š' => 0x8A, '‹' => 0x8B, 'Œ' => 0x8C,
        'Ž' => 0x8E, '‘' => 0x91, '’' => 0x92, '“' => 0x93, '”' => 0x94, '•' => 0x95,
        '–' => 0x96, '—' => 0x97, '˜' => 0x98, '™' => 0x99, 'š' => 0x9A, '›' => 0x9B,
        'œ' => 0x9C, 'ž' => 0x9E, 'Ÿ' => 0x9F,
        // Undefined cp1252 slots some decoders pass through as C1 controls.
        '\u{81}' => 0x81, '\u{8D}' => 0x8D, '\u{8F}' => 0x8F, '\u{90}' => 0x90, '\u{9D}' => 0x9D,
        _ => return None,
    };
    Some(b)
}

/// Decodes one UTF-8 sequence starting at `bytes[0]` if it lies in the
/// accepted target range; returns the char and the number of bytes used.
fn decode_plausible(bytes: &[u8]) -> Option<(char, usize)> {
    let len = match bytes.first()? {
        0xC2..=0xC5 => 2,
        0xE2 => 3,
        _ => return None,
    };
    let s = std::str::from_utf8(bytes.get(..len)?).ok()?;
    let c = s.chars().next()?;
    Some((c, len))
}

/// x-sorting occasionally reorders the three zero-width glyphs of one
/// sequence ("‰€â" instead of "â€‰"): retry a 3-byte window in the orders that
/// put the E2 lead first, accepting only a General Punctuation result.
fn decode_shuffled(bytes: &[u8]) -> Option<(char, usize)> {
    if bytes.len() != 3 || !bytes.contains(&0xE2) {
        return None;
    }
    let (a, b, c) = (bytes[0], bytes[1], bytes[2]);
    [[a, b, c], [a, c, b], [b, a, c], [b, c, a], [c, a, b], [c, b, a]]
        .iter()
        .filter(|p| p[0] == 0xE2)
        .filter_map(|p| decode_plausible(p))
        // Several orders can be valid UTF-8; only General Punctuation (the
        // spaces/dashes PDFs actually carry) is a believable original.
        .find(|(ch, _)| ('\u{2000}'..='\u{206F}').contains(ch))
}

/// Undoes UTF-8-read-as-Windows-1252 sequences and normalises the PDF
/// typography artefacts described in the module comment.
pub fn repair_mojibake(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut decoded: Vec<char> = Vec::with_capacity(chars.len());
    let mut i = 0;
    while i < chars.len() {
        // Collect up to 3 cp1252 bytes from the current position on.
        let mut bytes = Vec::with_capacity(3);
        for c in chars[i..].iter().take(3) {
            match cp1252_byte(*c) {
                Some(b) => bytes.push(b),
                None => break,
            }
        }
        if let Some((c, used)) = decode_plausible(&bytes).or_else(|| decode_shuffled(&bytes)) {
            decoded.push(c);
            i += used;
        } else {
            decoded.push(chars[i]);
            i += 1;
        }
    }

    let mut out = String::with_capacity(text.len());
    for (k, &c) in decoded.iter().enumerate() {
        match c {
            '\u{AD}' => {}
            '\u{2002}' | '\u{2003}' | '\u{2007}' | '\u{2009}' | '\u{200A}' | '\u{202F}' => out.push(' '),
            // Previous char taken from `out`: a soft hyphen before it is already gone.
            'Â' if out.chars().last().is_some_and(char::is_lowercase)
                && decoded.get(k + 1).map_or(true, |n| n.is_lowercase() || n.is_whitespace()) => {}
            _ => out.push(c),
        }
    }
    out
}
