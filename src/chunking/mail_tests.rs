//! Mail chunker on the block forms found in the Enron corpus.

use super::mail::{chunk_mail as chunk, duplicate_key, embedding_text, MAIL_MAX_CHARS};
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

#[test]
fn an_embedded_mail_is_embedded_with_its_subject_and_sender() {
    let t = format!("{HEAD}Yes.\n\n-----Original Message-----\nFrom: Skilling, Jeff\nSent: Monday\nTo: B\n\nThe plan stands.\n");
    let cs = chunk(&t, 1, 2);
    assert_eq!(embedding_text(&cs[0].text, &cs[0].metadata), cs[0].text, "a mail's own passage has its header");
    assert!(embedding_text(&cs[1].text, &cs[1].metadata).starts_with("Wow › Skilling, Jeff\n\n"));
    assert_eq!(embedding_text("plain", "{}"), "plain");
}

fn quoted(t: &str) -> Vec<NewChunk> {
    let cs = chunk(t, 1, 2);
    assert_offsets(t, &cs);
    cs
}

#[test]
fn german_outlook_block_is_found_with_its_author() {
    let t = format!("{HEAD}Passt für mich.\n\nVon: Maier, Anna <anna.maier@example.com>\nGesendet: Montag, 14. Mai 2026 09:00\nAn: Vogt, Ben <ben.vogt@example.com>\nBetreff: Plan\n\nDer Plan steht.\n");
    let cs = quoted(&t);
    assert_eq!(kinds(&cs), ["mail_message", "mail_quoted"]);
    assert_eq!(meta(&cs[1])["quoted_from"], "Maier, Anna <anna.maier@example.com>");
    assert_eq!(meta(&cs[1])["quoted_date"], "Montag, 14. Mai 2026 09:00");
}

#[test]
fn outlook_html_turned_into_bold_lines_is_found() {
    let t = format!("{HEAD}Passt.\n\n---\n\n**Von:** Maier, Anna <anna.maier@example.com>  \n**Gesendet:** Montag, 14. Mai 2026 09:00  \n**An:** Vogt, Ben  \n**Betreff:** Plan\n\nDer Plan steht.\n");
    let cs = quoted(&t);
    assert_eq!(kinds(&cs), ["mail_message", "mail_quoted"]);
    assert_eq!(meta(&cs[1])["quoted_from"], "Maier, Anna <anna.maier@example.com>");
}

#[test]
fn english_and_french_outlook_blocks_are_found() {
    for block in [
        "From: Anna\nSent: Monday, May 14, 2026 9:00 AM\nTo: Ben\nSubject: Plan",
        "De : Anna\nEnvoyé : lundi 14 mai 2026 09:00\nÀ : Ben\nObjet : Plan",
        "Lähettäjä: Anna\nLähetetty: maanantai 14. toukokuuta 2026 9.00\nVastaanottaja: Ben\nAihe: Plan",
    ] {
        let t = format!("{HEAD}Okay.\n\n{block}\n\nThe plan stands.\n");
        assert_eq!(kinds(&quoted(&t)), ["mail_message", "mail_quoted"], "{block}");
    }
}

#[test]
fn gmail_and_apple_reply_lines_are_found_in_german_and_english() {
    for line in ["On Mon, May 14, 2026 at 9:00 AM Anna Maier <anna@example.com> wrote:", "Am 14.05.2026 um 09:00 schrieb Anna Maier <anna@example.com>:", "Am Mo., 14. Mai 2026 um 09:00 Uhr schrieb Anna Maier <anna@example.com>:"] {
        let t = format!("{HEAD}Okay.\n\n{line}\n> Der Plan steht.\n");
        let cs = quoted(&t);
        assert_eq!(kinds(&cs), ["mail_message", "mail_quoted"], "{line}");
        assert!(meta(&cs[1])["quoted_from"].as_str().unwrap().contains("Anna Maier"), "{line}");
    }
}

#[test]
fn gmail_and_apple_forward_banners_are_found() {
    for line in ["---------- Forwarded message ---------", "---------- Weitergeleitete Nachricht ---------", "Anfang der weitergeleiteten Nachricht:"] {
        let t = format!("{HEAD}Zur Info.\n\n{line}\nVon: Anna\nDatum: 14.05.2026\nBetreff: Plan\nAn: Ben\n\nDer Plan steht.\n");
        assert_eq!(kinds(&quoted(&t)), ["mail_message", "mail_quoted"], "{line}");
    }
}

#[test]
fn a_phone_signature_does_not_tell_copies_apart() {
    let text = "Der Plan steht, und wir gehen wie am Montag besprochen vor. Das gilt auch für den zweiten Standort, sobald die Genehmigungen vorliegen und das Team bereitsteht.\nBitte gebt das an alle anderen weiter, die daran mitarbeiten, und meldet euch bei Fragen.";
    let key = |tail: &str| duplicate_key(&chunk(&format!("{HEAD}{text}\n{tail}"), 1, 2)[0]).unwrap();
    assert_eq!(key(""), key("\nVon meinem iPhone gesendet\n"));
}

#[test]
fn a_german_sentence_with_an_or_von_at_a_line_start_is_no_block() {
    let t = format!("{HEAD}Bitte beachten:\nVon: hier aus sehe ich nichts.\nDas war alles.\nGruss\nA\nB\nC\nD\n");
    assert_eq!(chunk(&t, 1, 2).len(), 1);
}

#[test]
fn a_signature_with_phone_and_mail_lines_is_no_block() {
    let t = format!("{HEAD}Bis morgen.\n\nTel.: +49 69 1234-5678\nFax: +49 69 1234-5679\nMail: anna@example.com\nWeb: www.example.com\n");
    assert_eq!(chunk(&t, 1, 2).len(), 1);
}

#[test]
fn a_reply_line_in_a_language_never_seen_is_found_by_its_form() {
    let t = format!("{HEAD}Dobrze.\n\nW dniu 14.05.2026 o 09:00 Anna Maier <anna@example.com> pisze:\n> Der Plan steht.\n");
    assert_eq!(kinds(&chunk(&t, 1, 2)), ["mail_message", "mail_quoted"]);
}

#[test]
fn japanese_outlook_block_with_a_full_width_colon_is_found() {
    for block in [
        "差出人: 山田太郎 <taro@example.com>\n送信日時: 2026年5月14日 9:00\n宛先: 鈴木花子\n件名: 計画",
        "差出人： 山田太郎 <taro@example.com>\n送信日時： 2026年5月14日\n宛先： 鈴木花子\n件名： 計画",
    ] {
        let t = format!("{HEAD}了解しました。\n\n{block}\n\n計画は確定です。\n");
        let cs = chunk(&t, 1, 2);
        assert_offsets(&t, &cs);
        assert_eq!(kinds(&cs), ["mail_message", "mail_quoted"], "{block}");
        assert_eq!(meta(&cs[1])["quoted_from"], "山田太郎 <taro@example.com>");
    }
}

#[test]
fn a_japanese_reply_line_is_found() {
    let t = format!("{HEAD}了解。\n\n2026年5月14日(木) 9:00 山田太郎 <taro@example.com>:\n> 計画は確定です。\n");
    assert_eq!(kinds(&chunk(&t, 1, 2)), ["mail_message", "mail_quoted"]);
}

#[test]
fn arabic_outlook_block_with_direction_marks_and_arabic_digits_is_found() {
    for date in ["الخميس، ١٤ مايو ٢٠٢٦ ٠٩:٠٠", "الخميس، 14 مايو 2026 09:00"] {
        let t = format!("{HEAD}موافق.\n\n\u{200f}من: أحمد علي <ahmed@example.com>\n\u{200f}أرسلت: {date}\n\u{200f}إلى: سارة\n\u{200f}الموضوع: الخطة\n\nالخطة ثابتة.\n");
        let cs = chunk(&t, 1, 2);
        assert_offsets(&t, &cs);
        assert_eq!(kinds(&cs), ["mail_message", "mail_quoted"], "{date}");
        assert_eq!(meta(&cs[1])["quoted_from"], "أحمد علي <ahmed@example.com>");
    }
}

#[test]
fn a_mail_in_arabic_or_japanese_without_a_block_stays_whole() {
    assert_eq!(chunk(&format!("{HEAD}الخطة ثابتة.\n\nسنبدأ يوم الاثنين.\n"), 1, 2).len(), 1);
    assert_eq!(chunk(&format!("{HEAD}計画は確定です。\n\n月曜日に始めます。\n"), 1, 2).len(), 1);
}

fn starts(t: &str) -> Vec<String> {
    quoted(t).iter().skip(1).map(|c| c.text.lines().next().unwrap().trim().to_string()).collect()
}

#[test]
fn a_one_line_reply_between_two_blocks_keeps_them_apart() {
    let t = format!("{HEAD}Mine.\n\n-----Original Message-----\nFrom: B\nSent: Monday, May 14, 2001 9:00 AM\nTo: C\nSubject: One\n\nJust this line.\n\n-----Original Message-----\nFrom: D\nSent: Friday, May 11, 2001 8:00 AM\nTo: B\nSubject: Two\n\nThe question.\n");
    let cs = quoted(&t);
    assert_eq!(kinds(&cs), ["mail_message", "mail_quoted", "mail_quoted"]);
    assert_eq!(meta(&cs[2])["quoted_from"], "D");
    assert!(cs[1].text.ends_with("Just this line."));
}

#[test]
fn text_right_below_a_header_is_body_not_a_wrapped_line() {
    let t = format!("{HEAD}Mine.\n\nFrom: B\nSent: Monday, May 14, 2001 9:00 AM\nTo: C\nSubject: One\nAre you, by any chance, familiar with it?\n-----Original Message-----\nFrom: D\nSent: Friday, May 11, 2001 8:00 AM\nTo: B\nSubject: Two\n\nThe question.\n");
    let cs = quoted(&t);
    assert_eq!(kinds(&cs), ["mail_message", "mail_quoted", "mail_quoted"]);
    assert!(cs[1].text.ends_with("familiar with it?"));
}

#[test]
fn lotus_header_with_empty_lines_inside_is_found() {
    let t = format!("{HEAD}See below.\n\nFrom:  Lexi Elliott @ ENRON                           10/16/2000 06:33 PM\n\n\nTo: Mike McConnell/HOU/ECT@ECT\n\ncc:  \n\nbcc:\n\nSubject: Hiring\n\nThe text.\n");
    let cs = quoted(&t);
    assert_eq!(kinds(&cs), ["mail_message", "mail_quoted"]);
    assert!(cs[1].text.starts_with("From:  Lexi") && cs[1].text.contains("The text."));
}

#[test]
fn a_recipient_list_may_wrap_over_many_lines() {
    let list = "Anna Maier/HOU/ECT@ECT, Ben Vogt/HOU/ECT@ECT,\n".repeat(8);
    let t = format!("{HEAD}See below.\n\n\nJohn Schwartzenburg\n04/10/2001 01:53 PM\nTo: Group\ncc: {list}Subject: Re: Plan\n\nThe text.\n");
    let cs = quoted(&t);
    assert_eq!(kinds(&cs), ["mail_message", "mail_quoted"]);
    assert_eq!(meta(&cs[1])["quoted_from"], "John Schwartzenburg");
}

#[test]
fn a_header_without_date_or_sender_counts_by_its_recipients() {
    for block in ["To: Jim Steffes\ncc:\nSubject: service of process", "To: Fred Mitro/HOU/ECT@ECT, Rusty Stevens/Corp/Enron@ENRON\ncc:  (bcc: Kathleen Carnahan/NA/Enron)\n\nSubject: Options", "An: anna@example.com\nKopie: ben@example.com\nBetreff: Plan"] {
        let t = format!("{HEAD}Mine.\n\n\n{block}\n\nThe text.\n");
        assert_eq!(kinds(&quoted(&t)), ["mail_message", "mail_quoted"], "{block}");
    }
}

#[test]
fn the_text_of_a_forward_chain_belongs_to_the_last_header() {
    let t = format!("{HEAD}Please look.\n\n---------------------- Forwarded by A/HOU/ECT on 09/06/2000 \n10:49 AM ---------------------------\n\n\nJeff Richter\n09/06/2000 07:39 AM\nTo: A/HOU/ECT@ECT\ncc:  \nSubject: Wow\n\n\n---------------------- Forwarded by Jeff Richter/HOU/ECT on 09/06/2000 09:45 \nAM ---------------------------\n\n\nTommy Hicks\n09/05/2000 10:57 AM\nTo: Jeff Richter/HOU/ECT@ECT\ncc:  \nSubject: Wow\n\nThe text.\n");
    let cs = quoted(&t);
    assert_eq!(cs.len(), 2, "{:?}", cs.iter().map(|c| &c.text).collect::<Vec<_>>());
    assert_eq!(meta(&cs[1])["quoted_from"], "Tommy Hicks");
    assert!(cs[1].text.contains("The text."));
}

#[test]
fn signatures_are_no_blocks_even_with_a_date_or_an_address_above() {
    for sig in [
        "Anna Maier\nanna@example.com\nTel: +49 69 1234-5678\nFax: 069 12345679\nWeb: www.example.com",
        "Sent from the office on 03/12/2001\nPhone: (713) 853-1234\nFax: 713-646-3490\nMail: anna@example.com",
        "Tel: 069 123456789\nMobil: 0171 1234567\nE-Mail: anna@example.com\nAbteilung: Einkauf\nStandort: Frankfurt\nStand: 12.03.2001",
    ] {
        assert_eq!(starts(&format!("{HEAD}Bis morgen.\n\n{sig}\n")), Vec::<String>::new(), "{sig}");
    }
}

#[test]
fn a_reply_line_right_after_a_signature_is_still_found() {
    let t = format!("{HEAD}Bis morgen.\n\nTel: 069 123456789\nFax: 069 123456780\nMail: anna@example.com\n\nAaron Thomas wrote:\n\nHello all.\n");
    assert_eq!(starts(&t), ["Aaron Thomas wrote:"]);
}

#[test]
fn schedules_and_lists_are_no_blocks() {
    for list in [
        "Impact:  EES\nTime:  Sat 10/13/2001 at  2:00:00 PM thru Sat 10/13/2001 at  5:00:00 PM\nOutage:  Upgrade\nPurpose:\nBackout: None",
        "Impact:  EES\nTime:  Fri 6/1/2001 at 8:00 PM\nFri 6/1/2001 at 6:00 PM PT\nOutage:  Link\nPurpose:\nBackout: None",
        "Date: Monday, November 27\nTime: 11:00 a.m.\nLocation: Boardroom",
        "Datum: 12.03.2001\nUhrzeit: 10:00\nOrt: Frankfurt",
        "Time: 8:25 PM\nActivity: Announce\nEvent: Notice",
        "Q: What about 12/03/2001?\nA: Fine.\nQ: And then?\nA: Nothing.",
    ] {
        assert_eq!(starts(&format!("{HEAD}Here is the plan.\n\n{list}\n\nThat is all.\n")), Vec::<String>::new(), "{list}");
    }
}

#[test]
fn a_korean_date_counts() {
    let t = format!("{HEAD}확인했습니다.\n\n보낸 사람: 김철수 <kim@example.com>\n보낸 날짜: 2026년 5월 14일 9:00\n받는 사람: 이영희\n제목: 계획\n\n계획은 확정입니다.\n");
    assert_eq!(kinds(&quoted(&t)), ["mail_message", "mail_quoted"]);
}

fn sender_of(t: &str) -> Option<String> {
    meta(chunk(t, 1, 2).last().unwrap())["quoted_from"].as_str().map(str::to_string)
}

#[test]
fn a_header_that_starts_with_the_subject_names_the_real_sender() {
    let head = HEAD.replace("# Wow", "# Fwd: Plan");
    let thunderbird = format!("{head}See below.\n\n-------- Forwarded Message --------\nSubject: Plan\nDate: Mon, 4 May 2026 09:12:00 +0200\nFrom: Anna Maier <anna@example.com>\nTo: Ben <ben@example.com>\n\nThe plan stands.\n");
    assert_eq!(sender_of(&thunderbird).as_deref(), Some("Anna Maier <anna@example.com>"));
    let cc_mail = format!("{head}See below.\n\n____Reply Separator____\nSubject: Re: Plan\nAuthor: Anna Maier\nDate: 05/01/2001 12:44 PM\n\nThe plan stands.\n");
    assert_eq!(sender_of(&cc_mail).as_deref(), Some("Anna Maier"));
}

#[test]
fn a_sender_line_ends_before_its_date() {
    let t = format!("{HEAD}See below.\n\nAnna Maier/HOU/ECT 05/17/2001 05:31 PM \t To: Ben Roth/HOU/ECT@ECT  cc:   Subject: Plan\n\nThe plan stands.\n");
    assert!(chunk(&t, 1, 2).len() == 1 || sender_of(&t).as_deref() == Some("Anna Maier/HOU/ECT"));
    let t = format!("{HEAD}See below.\n\nFrom: Anna Maier@ECT on 01/10/2001 10:35 AM\nTo: Ben Roth/Corp/Enron@Enron\ncc:\nSubject: Plan\n\nThe plan stands.\n");
    assert_eq!(sender_of(&t).as_deref(), Some("Anna Maier@ECT"));
}

#[test]
fn a_date_is_no_sender() {
    let t = format!("{HEAD}See below.\n\nSent:\tThursday, May 17, 2001 4:20 AM\nTo:\tBen Roth\nSubject:\tPlan\n\nThe plan stands.\n");
    assert_eq!(sender_of(&t), None);
}

#[test]
fn a_last_header_with_nothing_below_stays_with_the_text_above() {
    let t = format!("{HEAD}See below.\n\n-----Original Message-----\nFrom: Anna Maier\nSent: Monday, November 19, 2001 10:03 AM\nTo: Ben Roth\nSubject: Plan\n\n\n");
    let cs = chunk(&t, 1, 2);
    assert_offsets(&t, &cs);
    assert_eq!(kinds(&cs), ["mail_message"]);
    assert!(cs[0].text.contains("Subject: Plan"));
}

#[test]
fn a_lotus_header_that_starts_with_its_date_has_no_sender() {
    let t = format!("{HEAD}See below.\n\n11/30/2000 10:32 AM\n\nTo: Ben Roth/HOU/ECT@ECT\ncc:\n\nSubject: Plan\n\nThe plan stands.\n");
    assert_eq!(sender_of(&t), None);
}
