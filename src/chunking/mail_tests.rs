//! Mail chunker on the block forms found in the Enron corpus.

use super::mail::{chunk_mail as chunk, duplicate_key, MAIL_MAX_CHARS};
use crate::db::models::NewChunk;

const HEAD: &str = "# Wow\n\n**From:** a@example.com  \n**To:** b@example.com  \n**Date:** 2000-09-06 04:46 -07:00\n\n";

fn meta(c: &NewChunk) -> serde_json::Value {
    serde_json::from_str(&c.metadata).unwrap()
}

fn kinds(cs: &[NewChunk]) -> Vec<String> {
    cs.iter().map(|c| meta(c)["chunk_type"].as_str().unwrap().to_string()).collect()
}

fn assert_offsets(t: &str, cs: &[NewChunk]) {
    let mut prev = 0;
    for (i, c) in cs.iter().enumerate() {
        assert_eq!(c.chunk_index, i as i64);
        assert_eq!(&t[c.char_start as usize..c.char_end as usize], c.text);
        assert!(c.char_start >= prev);
        prev = c.char_end;
    }
}

#[test]
fn plain_mail_stays_one_passage_whatever_its_paragraphs() {
    let t = format!("{HEAD}First paragraph.\n\nSecond paragraph.\n\nThird paragraph.\n");
    let cs = chunk(&t, 1, 2);
    assert_offsets(&t, &cs);
    assert_eq!(kinds(&cs), ["mail_message"]);
    assert!(cs[0].text.starts_with("# Wow") && cs[0].text.ends_with("Third paragraph."));
}

#[test]
fn original_message_block_is_its_own_passage_with_author() {
    let t = format!(
        "{HEAD}I agree.\n\n-----Original Message-----\nFrom: \tSkilling, Jeff\nSent:\tMonday, May 14, 2001 9:00 AM\nTo:\tLay, Ken\nSubject:\tPlan\n\nThe plan stands.\n"
    );
    let cs = chunk(&t, 1, 2);
    assert_offsets(&t, &cs);
    assert_eq!(kinds(&cs), ["mail_message", "mail_quoted"]);
    assert!(cs[0].text.ends_with("I agree."));
    assert_eq!(meta(&cs[1])["quoted_from"], "Skilling, Jeff");
    assert_eq!(meta(&cs[1])["quoted_date"], "Monday, May 14, 2001 9:00 AM");
    assert_eq!(meta(&cs[1])["heading_path"], "Wow › Skilling, Jeff");
    assert!(cs[1].text.contains("The plan stands."));
}

#[test]
fn lotus_block_starts_at_sender_and_date_lines() {
    let t = format!(
        "{HEAD}Agree completely.\n\n\n\nLouise Kitchen\n09/14/2000 06:20 PM\nTo: John Arnold/HOU/ECT@ECT\ncc:  \nSubject: Re: file  \n\nIt looks fine.\n"
    );
    let cs = chunk(&t, 1, 2);
    assert_offsets(&t, &cs);
    assert_eq!(kinds(&cs), ["mail_message", "mail_quoted"]);
    assert!(cs[1].text.starts_with("Louise Kitchen"));
    assert_eq!(meta(&cs[1])["quoted_from"], "Louise Kitchen");
    assert_eq!(meta(&cs[1])["quoted_date"], "09/14/2000 06:20 PM");
}

#[test]
fn forward_banner_and_its_header_are_one_block_and_chains_split() {
    let t = format!(
        "{HEAD}---------------------- Forwarded by Phillip Allen/HOU/ECT on 09/06/2000 \n10:49 AM ---------------------------\n\n\nJeff Richter\n09/06/2000 07:39 AM\nTo: Phillip Allen/HOU/ECT@ECT\ncc:  \nSubject: Wow\n\nFirst text.\n\n---------------------- Forwarded by Jeff Richter/HOU/ECT on 09/06/2000 09:45 \nAM ---------------------------\nTo: Mike Swerzbin/HOU/ECT@ECT\ncc: \nSubject: Note\n\nSecond text.\n"
    );
    let cs = chunk(&t, 1, 2);
    assert_offsets(&t, &cs);
    assert_eq!(cs.len(), 2, "{:?}", cs.iter().map(|c| &c.text).collect::<Vec<_>>());
    assert!(cs[0].text.starts_with("# Wow"), "a forward without own text keeps the mail's header");
    assert!(cs[0].text.contains("First text."));
    assert!(cs[1].text.contains("Second text."));
}

#[test]
fn angle_quoted_reply_is_found_behind_the_marks() {
    let t = format!("{HEAD}Yes.\n\n> -----Original Message-----\n> From: X\n> Sent: Today\n> To: Y\n>\n> Why?\n");
    let cs = chunk(&t, 1, 2);
    assert_eq!(kinds(&cs), ["mail_message", "mail_quoted"]);
}

#[test]
fn block_over_the_limit_is_cut_at_paragraphs_only() {
    let para = "word ".repeat(1500);
    let body = format!("{para}\n\n").repeat(5);
    let t = format!("{HEAD}{body}");
    let cs = chunk(&t, 1, 2);
    assert_offsets(&t, &cs);
    assert!(cs.len() > 1);
    for c in &cs {
        assert!(c.text.chars().count() <= MAIL_MAX_CHARS);
        assert!(c.text.ends_with("word"), "cut inside a paragraph");
    }
}

#[test]
fn line_over_the_limit_falls_back_to_sentences() {
    let t = format!("{HEAD}{}", "Prices rose again today. ".repeat(2000));
    let cs = chunk(&t, 1, 2);
    assert_offsets(&t, &cs);
    assert!(cs.len() > 1);
    assert!(cs[0].text.starts_with("# Wow") && cs[0].text.contains("Prices rose"));
    assert!(cs.iter().all(|c| c.text.chars().count() <= MAIL_MAX_CHARS + HEAD.len() && c.text.ends_with("today.")));
}

#[test]
fn a_to_line_in_prose_without_cc_is_no_block() {
    let t = format!("{HEAD}To: whom it may concern\nwe ship on Monday.\nRegards\nA\nB\nC\nD\n");
    assert_eq!(chunk(&t, 1, 2).len(), 1);
}

#[test]
fn text_without_header_still_chunks() {
    let cs = chunk("just text\n", 1, 2);
    assert_eq!(cs.len(), 1);
    assert_eq!(cs[0].text, "just text");
}

#[test]
fn filing_lines_land_in_the_metadata_of_every_passage() {
    let t = format!(
        "# Wow\n\n**From:** a@example.com  \n**Date:** 2000-09-06  \n**Folder:** \\\\Arnold\\Sent Items  \n**Mailbox:** Arnold-J\n\nI agree.\n\n-----Original Message-----\nFrom: X\nSent: Today\nTo: Y\n\nWhy?\n"
    );
    let cs = chunk(&t, 1, 2);
    assert_eq!(cs.len(), 2);
    for c in &cs {
        assert_eq!(meta(c)["folder_kind"], "sent");
        assert_eq!(meta(c)["mailbox"], "Arnold-J");
    }
}

#[test]
fn the_same_mail_in_two_folders_has_one_key_and_a_changed_text_another() {
    let body = "The plan stands, and we go ahead with it as discussed on Monday. ".repeat(5);
    let filed = |folder: &str, text: &str| {
        let t = format!("# Wow\n\n**From:** a@example.com  \n**Folder:** {folder}  \n**Mailbox:** A\n\n{text}\n");
        duplicate_key(&chunk(&t, 1, 2)[0]).unwrap()
    };
    assert_eq!(filed("Inbox", &body), filed("Projects", &body));
    assert_ne!(filed("Inbox", &body), filed("Inbox", &format!("{body} Changed.")));
}

#[test]
fn a_quoted_block_has_the_same_key_behind_quote_marks() {
    let block = "From: X\nSent: Today\nTo: Y\n\nThe plan stands, and we go ahead with it as discussed on Monday and Tuesday. And more of the same text.\nA second line that makes the block long enough to count as a repeat worth tracking.\n";
    let plain = format!("{HEAD}Yes.\n\n-----Original Message-----\n{block}");
    let marked = format!("{HEAD}No.\n\n> -----Original Message-----\n{}", block.lines().map(|l| format!("> {l}\n")).collect::<String>());
    let (a, b) = (chunk(&plain, 1, 2), chunk(&marked, 1, 2));
    assert_eq!(duplicate_key(&a[1]).unwrap(), duplicate_key(&b[1]).unwrap());
}

#[test]
fn short_passages_have_no_key() {
    assert!(duplicate_key(&chunk(&format!("{HEAD}Thanks.\n"), 1, 2)[0]).is_none());
}

#[test]
fn a_mail_and_its_quote_in_a_reply_have_one_key() {
    let text = "The plan stands, and we go ahead with it as discussed on Monday and Tuesday of this week.\nIt also holds for the second site, where the same crew starts as soon as the permits are in.\nPlease tell the others.";
    let own = format!("{HEAD}{text}\n");
    let reply = format!("{HEAD}Agreed.\n\n-----Original Message-----\nFrom: A\nSent: Monday\nTo: B\nSubject: Wow\n\n{text}\n");
    let lotus = format!("{HEAD}Agreed.\n\n\n\nJeff Richter\n09/06/2000 07:39 AM\nTo: B/HOU/ECT@ECT\ncc:  \nSubject: Wow\n\n{text}\n");
    let wrapped = format!("{HEAD}No.\n\n> -----Original Message-----\n> From: A\n> Sent: Monday\n> To: B\n>\n> {}\n", text.replace(" as discussed", "\n> as discussed").replace('\n', "\n> ").replace("> > ", "> "));
    let key = |t: &str, i: usize| duplicate_key(&chunk(t, 1, 2)[i]).unwrap();
    assert_eq!(key(&own, 0), key(&reply, 1));
    assert_eq!(key(&own, 0), key(&lotus, 1));
    assert_eq!(key(&own, 0), key(&wrapped, 1));
}

#[test]
fn footers_and_empty_markers_at_the_end_do_not_tell_copies_apart() {
    let text = "The plan stands, and we go ahead with it as discussed on Monday and Tuesday of this week.\nIt also holds for the second site, where the same crew starts as soon as the permits are in.\nPlease tell the others.";
    let key = |tail: &str| duplicate_key(&chunk(&format!("{HEAD}{text}\n{tail}"), 1, 2)[0]).unwrap();
    let plain = key("");
    assert_eq!(plain, key("\n**********************************************************************\nThis e-mail is the property of Enron Corp. and may contain confidential information.\n"));
    assert_eq!(plain, key("\n<Embedded StdOleLink>\n"));
    assert_eq!(plain, key("\n_________________________________________________________________\nGet your FREE download of MSN Explorer\n"));
    assert_ne!(plain, key("\nAnd one more sentence that belongs to the mail itself and changes what it says.\n"));
}

#[test]
fn importance_line_is_part_of_an_embedded_header() {
    let text = "The plan stands, and we go ahead with it as discussed on Monday and Tuesday of this week.\nIt also holds for the second site, where the same crew starts as soon as the permits are in.\nPlease tell the others.";
    let a = format!("{HEAD}Yes.\n\n-----Original Message-----\nFrom: A\nSent: Monday\nTo: B\nSubject: Wow\nImportance: High\n\n{text}\n");
    let own = format!("{HEAD}{text}\n");
    assert_eq!(duplicate_key(&chunk(&a, 1, 2)[1]).unwrap(), duplicate_key(&chunk(&own, 1, 2)[0]).unwrap());
}

#[test]
fn lotus_sender_above_a_line_of_tabs_belongs_to_the_block() {
    let t = format!(
        "{HEAD}Can you confirm that?\n\n\n\n\tJohn Malowney\n\t07/17/2000 11:25 AM\n\t\t \n\t\t To: Tana Jones/HOU/ECT@ECT\n\t\t cc: Jan M King/HOU/ECT@ECT\n\t\t Subject: ISDA\n\nThey are a public utility.\n"
    );
    let cs = chunk(&t, 1, 2);
    assert_offsets(&t, &cs);
    assert_eq!(kinds(&cs), ["mail_message", "mail_quoted"]);
    assert!(cs[0].text.ends_with("Can you confirm that?"), "{}", cs[0].text);
    assert_eq!(meta(&cs[1])["quoted_from"], "John Malowney");
    assert_eq!(meta(&cs[1])["quoted_date"], "07/17/2000 11:25 AM");
}
