-- schema_v82: the seeded structural profile "Legal Texts (EU/MaRisk)" knows the module labels of
-- BaFin circulars beyond "AT": BT, BTO, BTR (MaRisk "BTO 1.2 Anforderungen an die Prozesse …").
-- Before, a BTO heading was no heading, so its passages kept the section of the AT module before.
--
-- Only the heading pattern exactly as seeded (services `seed.rs`) is changed; a pattern the user
-- edited stays as it is. Takes effect when a document is chunked again.
-- Plain UPDATE, runs through db/mod.rs::migrate's default arm.

UPDATE structural_patterns
   SET regex = '^((?:Article|Artikel|Art\.|§|AT|BTO|BTR|BT|Kapitel|Abschnitt|TITEL|TITLE|CHAPTER)\s*[\d.a-zA-Z]+)\s*(.*)'
 WHERE role = 'heading_l1'
   AND regex = '^((?:Article|Artikel|Art\.|§|AT|Kapitel|Abschnitt|TITEL|TITLE|CHAPTER)\s*[\d.a-zA-Z]+)\s*(.*)';
