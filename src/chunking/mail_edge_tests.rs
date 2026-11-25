//! Edge cases of the mail chunker, one test per rule: mails without text, signatures, schedules,
//! where a header ends, reply and banner lines, Lotus sender lines, senders of embedded mails,
//! long mails, and what the duplicate key and the boilerplate check leave out. The typical block
//! forms are in `mail_tests.rs`.

use super::mail::{chunk_mail as chunk, duplicate_key, is_boilerplate, MAIL_MAX_CHARS};
use super::mail_tests::{assert_offsets, kinds, meta, quoted, sender_of, starts, HEAD};

#[test]
fn a_mail_without_text_or_with_a_huge_header_only_still_chunks() {
    assert_eq!(chunk("# Wow\n", 1, 2)[0].text, "# Wow");
    let head_only = chunk(HEAD, 1, 2);
    assert_eq!(duplicate_key(&head_only[0]), None, "nothing below the header");
    let list = "anna.maier@example.com, ".repeat(800);
    let t = format!("# Wow\n\n**From:** a@example.com  \n**To:** {list}\n");
    let cs = chunk(&t, 1, 2);
    assert_offsets(&t, &cs);
    assert!(cs.len() > 1 && cs.iter().all(|c| c.text.chars().count() <= MAIL_MAX_CHARS));
    let three_lines = chunk("Zeile eins\nZeile zwei\nZeile drei\n", 1, 2);
    assert_eq!(duplicate_key(&three_lines[0]), None);
}

#[test]
fn phone_numbers_opening_with_plus_or_bracket_mark_a_signature() {
    for sig in ["Tel: +49 69 1234 5678\nFax: +49 69 1234 5679\nStand: 12.03.2001", "Phone: (713) 853 1234\nFax: (713) 646 3490\nUpdated: 03/12/2001"] {
        assert_eq!(starts(&format!("{HEAD}Bis morgen.\n\n{sig}\n")), Vec::<String>::new(), "{sig}");
    }
}

#[test]
fn a_time_range_in_one_line_is_a_schedule_and_a_wrapped_time_still_a_header() {
    let list = "Impact:  EES\nTime:  Fri 6/1/2001 8:00 PM to 10:00 PM\nOutage:  Link\nBackout: None";
    assert_eq!(starts(&format!("{HEAD}Here is the plan.\n\n{list}\n\nThat is all.\n")), Vec::<String>::new());
    let t = format!("{HEAD}Mine.\n\nFrom: Anna\nSent: Monday, May 14, 2001\n9:00 AM\nTo: Ben\nSubject: Plan\n\nThe plan stands.\n");
    assert_eq!(starts(&t), ["From: Anna"]);
}

#[test]
fn a_header_ends_after_two_lines_without_a_label() {
    // Two wrapped recipient lines belong to the header; a third plain line ends it, so a later
    // label line (inside a quote) cannot swallow the reply line above it.
    let wrapped = format!("{HEAD}Mine.\n\nTo: Anna Maier\nBen Vogt\nCarl Duck\nSubject: Plan\nDate: 14.05.2026\n\nThe plan stands.\n");
    assert_eq!(starts(&wrapped), ["To: Anna Maier"]);
    let t = format!("{HEAD}Mine.\n\nFrom: B\nSent: Monday, May 14, 2001 9:00 AM\nTo: C\nSubject: One\nText one.\nText two.\nText three.\nOn Mon, May 14, 2001 Anna wrote:\n> Hinweis: bitte lesen\n");
    assert_eq!(starts(&t), ["From: B", "On Mon, May 14, 2001 Anna wrote:"]);
}

#[test]
fn recipients_on_wrapped_lines_make_a_header_without_date() {
    let t = format!("{HEAD}Mine.\n\n\nTo: Group\nanna@example.com,\nben@example.com\nSubject: Plan\nPriority: High\n\nThe text.\n");
    assert_eq!(starts(&t), ["To: Group"]);
}

#[test]
fn a_reply_line_needs_a_colon_a_time_or_address_and_a_quote_below() {
    let t = format!("{HEAD}Okay.\n\nAm 14.05.2026 um 09:00 schrieb Anna Maier:\n> Der Plan steht.\n");
    assert_eq!(starts(&t), ["Am 14.05.2026 um 09:00 schrieb Anna Maier:"]);
    for text in ["Folgendes ist wichtig:\nDer Plan steht.\n", "Siehe unten\n> zitierter Text\n", "Stand 14.05.2026:\nDer Plan steht.\n"] {
        assert_eq!(chunk(&format!("{HEAD}{text}"), 1, 2).len(), 1, "{text}");
    }
}

#[test]
fn a_banner_or_announcing_line_up_to_two_blank_lines_above_belongs_to_the_block() {
    let t = format!("{HEAD}Danke.\n\n---- Weitergeleitet ----\n\n\nVon: Anna\nGesendet: 14.05.2026 09:00\nAn: Ben\nBetreff: Plan\n\nDer Plan steht.\n");
    assert_eq!(starts(&t), ["---- Weitergeleitet ----"]);
    let t = format!("{HEAD}Danke.\n\nZur Info, siehe unten:\n\nVon: Anna\nGesendet: 14.05.2026 09:00\nAn: Ben\nBetreff: Plan\n\nDer Plan steht.\n");
    assert_eq!(starts(&t), ["Zur Info, siehe unten:"]);
}

#[test]
fn lotus_sender_lines_reach_two_blank_lines_and_three_lines_above_to() {
    let far = format!("{HEAD}Mine.\n\nAnna Maier\n05/14/2001 09:00 AM\n\n\n\n\nTo: Ben Roth/HOU/ECT@ECT\ncc:\nSubject: Plan\n\nThe text.\n");
    assert_eq!(starts(&far), ["To: Ben Roth/HOU/ECT@ECT"], "more than two blank lines above To:");
    let t = format!("{HEAD}Mine.\nLine a\nLine b\nLine c\nLine d\nAnna Maier\n05/14/2001 09:00 AM\nTo: Ben Roth/HOU/ECT@ECT\ncc:\nSubject: Plan\n\nThe text.\n");
    let cs = quoted(&t);
    assert!(cs[0].text.ends_with("Line c"), "text above the sender stays with the mail: {:?}", cs[0].text);
}

#[test]
fn a_header_without_date_needs_a_sender_above_or_mostly_filled_lines() {
    let lotus = format!("{HEAD}Mine.\n\nAnna Maier\n05/14/2001 09:00 AM\nTo: Ben Roth\nSubject: Plan\nPriority: High\n\nThe text.\n");
    assert_eq!(starts(&lotus), ["Anna Maier"]);
    assert_eq!(chunk(&format!("{HEAD}Mine.\n\n\nTo: Jim Steffes\ncc:\nbcc:\n\nThe text.\n"), 1, 2).len(), 1, "two of three labels empty");
}

#[test]
fn a_one_line_forward_banner_without_rule_keeps_the_next_block() {
    let t = format!("{HEAD}Mine.\n\n--- Forwarded by Anna on 05/14/2001\nNotiz dazu.\n\n-----Original Message-----\nFrom: B\nSent: Monday, May 14, 2001 9:00 AM\nTo: C\nSubject: One\n\nThe text.\n");
    assert_eq!(kinds(&quoted(&t)), ["mail_message", "mail_quoted", "mail_quoted"]);
}

#[test]
fn senders_of_embedded_mails_in_every_form() {
    let head = HEAD.replace("# Wow", "# Fwd: Plan");
    let chain = format!("{head}See below.\n\n---------------------- Forwarded by Ben Roth/HOU/ECT on 05/14/2001\n09:00 AM ---------------------------\n-------- Original Message --------\nSubject: Plan\nDate: Mon, 14 May 2001 09:00:00\nFrom: Anna Maier <anna@example.com>\nTo: Ben\n\nThe plan stands.\n");
    assert_eq!(sender_of(&chain).as_deref(), Some("Anna Maier <anna@example.com>"), "sender on the sixth line");
    let to_first = format!("{HEAD}Mine.\n\n\nTo: Ben Roth/HOU/ECT@ECT\ncc: Carl Duck/HOU/ECT@ECT\nSubject: Plan\n\nThe text.\n");
    assert_eq!(sender_of(&to_first), None, "a recipient is no sender");
    let reply = format!("{HEAD}Okay.\n\nAm 14.05.2026 um 09:00 schrieb Anna Maier:\n> Der Termin am 20.05.2026 steht.\n");
    assert!(sender_of(&reply).unwrap().contains("Anna Maier"), "a date in the quote is no sender line");
    let bare = format!("{HEAD}Mine.\n\n-----Original Message-----\nAnna Maier\nTo: Ben\nSubject: Plan\n\nThe text.\n");
    assert_eq!(sender_of(&bare).as_deref(), Some("Anna Maier"));
    let with_date = format!("{HEAD}Mine.\n\n-----Original Message-----\nFrom: Anna Maier on 05/14/2001 09:00 AM\nTo: Ben\nSubject: Plan\n\nThe text.\n");
    assert_eq!(sender_of(&with_date).as_deref(), Some("Anna Maier"), "a name without an address before the date");
}

#[test]
fn a_long_mail_is_cut_at_its_paragraphs_with_numbered_parts() {
    let para = |i: usize| format!("{}ENDE{i}", "wort wort wort wort wort wort wort wort wort wort\n".repeat(30));
    let t = format!("{HEAD}{}\n", (0..14).map(para).collect::<Vec<_>>().join("\n\n"));
    let cs = quoted(&t);
    assert!(cs.len() > 1);
    let n = cs.len() as u64;
    for (i, c) in cs.iter().enumerate() {
        assert!(c.text.chars().count() <= MAIL_MAX_CHARS);
        assert!(c.text.lines().last().unwrap().starts_with("ENDE"), "part {i} ends inside a paragraph");
        assert_eq!((meta(c)["part"].as_u64(), meta(c)["parts"].as_u64()), (Some(i as u64 + 1), Some(n)));
    }
    // One paragraph over the limit: the header stays with its first piece (on top of the limit).
    let t = format!("{HEAD}{}\n", "wort ".repeat(4000));
    let cs = quoted(&t);
    assert!(cs[0].text.starts_with("# Wow") && cs[0].text.contains("**From:**") && cs[0].text.contains("wort"));
    assert!(cs.iter().all(|c| c.text.chars().count() <= MAIL_MAX_CHARS + HEAD.len()));
}

const LONG_TEXT: &str = "The plan stands, and we go ahead with it as discussed on Monday and Tuesday of this week.\nIt also holds for the second site, where the same crew starts as soon as the permits are in.\nPlease tell the others.";

#[test]
fn the_key_skips_every_kind_of_header_line() {
    let key = |t: &str, i: usize| duplicate_key(&chunk(t, 1, 2)[i]).unwrap();
    let own = key(&format!("{HEAD}{LONG_TEXT}\n"), 0);
    let attached = HEAD.replacen("**From:**", "**Attachments (2):** plan.pdf  \n**From:**", 1);
    assert_eq!(key(&format!("{attached}{LONG_TEXT}\n"), 0), own, "a bold line that is no Label: line");
    let reply = format!("{HEAD}Okay.\n\nAm 14.05.2026 um 09:00 schrieb Anna Maier:\n{}\n", LONG_TEXT.lines().map(|l| format!("> {l}")).collect::<Vec<_>>().join("\n"));
    assert_eq!(key(&reply, 1), own, "a reply line");
    let first = |line: &str| key(&format!("{HEAD}{line}\nHinweis: bitte lesen\n{LONG_TEXT}\n"), 0);
    assert_ne!(first("Erste Zeile A."), first("Erste Zeile B."), "the body's first line is text, not a Lotus sender");
}

/// Found by the key test above: a forward banner counted as a header line, so the Lotus sender
/// and date lines below it were read as body and the forwarded copy got a key of its own.
#[test]
fn regression_rule_dup_key_lotus_sender_below_a_forward_banner() {
    let key = |t: &str, i: usize| duplicate_key(&chunk(t, 1, 2)[i]).unwrap();
    let own = key(&format!("{HEAD}{LONG_TEXT}\n"), 0);
    for banner in [
        "--- Forwarded by Ben Roth on 05/14/2001",
        "---------------------- Forwarded by Ben Roth/HOU/ECT on 05/14/2001 09:45 AM ---------------------------",
    ] {
        let forward = format!("{HEAD}Agreed.\n\n{banner}\n\nAnna Maier\n05/13/2001 08:00 AM\nTo: Ben Roth/HOU/ECT@ECT\ncc:\nSubject: Wow\n\n{LONG_TEXT}\n");
        assert_eq!(key(&forward, 1), own, "{banner}");
    }
}

#[test]
fn a_short_mail_in_two_folders_has_one_key() {
    let filed = |folder: &str| {
        let t = format!("# Ein etwas längerer Betreff für die Ablage\n\n**From:** anna.maier@example.com  \n**To:** ben.vogt@example.com  \n**Cc:** carl.duck@example.com, dora.ente@example.com  \n**Date:** 2000-09-06 04:46 -07:00  \n**Folder:** {folder}  \n**Mailbox:** A\n\nKurz und gut, danke.\n");
        duplicate_key(&chunk(&t, 1, 2)[0]).unwrap()
    };
    assert_eq!(filed("Inbox"), filed("Projects"));
}

#[test]
fn a_rule_far_above_the_end_starts_no_footer() {
    let key = |t: &str| duplicate_key(&chunk(t, 1, 2)[0]).unwrap();
    let rule = "*".repeat(30);
    let long_tail = format!("{LONG_TEXT}\n").repeat(12);
    assert_ne!(key(&format!("{HEAD}{LONG_TEXT}\n{rule}\n{long_tail}")), key(&format!("{HEAD}{LONG_TEXT}\n")));
}

#[test]
fn boilerplate_paragraphs_of_every_kind() {
    for p in [
        "Sent from my iPhone",
        "<Embedded StdOleLink>",
        "# Betreff der Mail",
        "**Gesendet:** Montag, 14. Mai 2026",
        "--- Forwarded by Ben Roth/HOU/ECT on 05/14/2001",
        "____Reply Separator____",
        "Anna Maier\nTel.: +49 69 1234 5678",
        "Anna Maier\n+49 69 1234 5678",
        "Anna Maier\nanna@example.com",
    ] {
        assert!(is_boilerplate(p), "{p}");
    }
    assert!(!is_boilerplate("Danke, das passt so."));
}
