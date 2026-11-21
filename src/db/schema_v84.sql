-- schema_v84: the seeded heading pattern of "Legal Texts (EU/MaRisk)" needs a number (or a Roman numeral)
-- after the label, as the default pattern in chunking/structural.rs does. Before, `\s*[\d.a-zA-Z]+` let
-- "Attention …", "Artikels …", "Kapitel bestehen …" or "BTO Anforderungen …" open a section. A title glued to
-- the number, as PDFs of German laws extract it ("§ 25fBesondere Anforderungen …"), stays a heading: the
-- number may end in one lower-case letter, then the title starts with a space, an upper-case letter or "(".
--
-- Only the pattern exactly as seeded since v82 (services `seed.rs`) is changed; a pattern the user edited
-- stays as it is. Takes effect when a document is chunked again.
-- Plain UPDATE, runs through db/mod.rs::migrate's default arm.

UPDATE structural_patterns
   SET regex = '^((?:Article|Artikel|Art\.|§|AT|BTO|BTR|BT|Kapitel|Abschnitt|TITEL|TITLE|CHAPTER)(?:\s*\d[\d.]*(?-i:[a-z])?|\s+(?-i:[IVXLC]+)))((?:\s|(?-i:[A-ZÄÖÜ(])).*|)$'
 WHERE role = 'heading_l1'
   AND regex = '^((?:Article|Artikel|Art\.|§|AT|BTO|BTR|BT|Kapitel|Abschnitt|TITEL|TITLE|CHAPTER)\s*[\d.a-zA-Z]+)\s*(.*)';
