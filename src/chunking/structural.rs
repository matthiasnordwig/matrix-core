use pdf_oxide::PdfDocument;
use regex::RegexBuilder;
use std::collections::BTreeMap;

use super::sentences::split_sentences;
use crate::db::models::{NewChunk, StructuralProfile};

#[derive(Debug, Clone)]
struct CompiledPattern {
    role: String,
    regex: regex::Regex,
    priority: i64,
}

#[derive(Debug, Clone)]
struct ClassifiedLine {
    text: String,
    y: i32,
    page_idx: usize,
    max_font_size: f32,
    matched_role: Option<String>,
    extracted_abschnitt: Option<String>,
    extracted_titel: Option<String>,
}

pub fn chunk_pdf_structurally(
    bytes: &[u8],
    context_id: i64,
    document_id: i64,
    profile: Option<StructuralProfile>,
) -> Result<Vec<NewChunk>, String> {
    let doc = PdfDocument::from_bytes(bytes.to_vec()).map_err(|e| e.to_string())?;
    
    let (min_chunk_chars, max_chunk_chars, compiled_patterns) = if let Some(p) = profile {
        let mut comps = Vec::new();
        for pat in p.patterns {
            let mut builder = RegexBuilder::new(&pat.regex);
            builder.case_insensitive(pat.flags.contains('i'));
            builder.multi_line(pat.flags.contains('m'));
            if let Ok(re) = builder.build() {
                comps.push(CompiledPattern {
                    role: pat.role,
                    regex: re,
                    priority: pat.priority,
                });
            }
        }
        comps.sort_by(|a, b| b.priority.cmp(&a.priority));
        (p.min_chunk_chars as usize, p.max_chunk_chars as usize, comps)
    } else {
        let mut comps = Vec::new();
        comps.push(CompiledPattern {
            role: "ignore".to_string(),
            regex: RegexBuilder::new(r"(?:Seite|Page|Bundesgesetzblatt|Amtsblatt|BAnz)").case_insensitive(true).build().unwrap(),
            priority: 200,
        });
        comps.push(CompiledPattern {
            role: "heading_l1".to_string(),
            regex: RegexBuilder::new(r"^((?:Article|Art\.|§|AT|Kapitel|Abschnitt|TITEL|TITLE|CHAPTER)\s*[\d.a-zA-Z]+)\s*(.*)").case_insensitive(true).build().unwrap(),
            priority: 100,
        });
        comps.push(CompiledPattern {
            role: "definition".to_string(),
            regex: RegexBuilder::new(r"\b(?:means|shall mean|bezeichnet|gilt als|im Sinne)").case_insensitive(true).build().unwrap(),
            priority: 50,
        });
        (200, 1500, comps)
    };

    let mut all_chunks = Vec::new();
    let mut chunk_index = 0;
    
    let mut current_abschnitt: Option<String> = None;
    let mut current_titel: Option<String> = None;
    
    let mut current_chunk_text = String::new();
    let mut current_chunk_start_page: Option<usize> = None;
    let mut current_chunk_char_start: usize = 0;
    let mut total_chars_processed = 0;

    let mut flush_chunk = |text: &mut String, abschnitt: &Option<String>, titel: &Option<String>, page: Option<usize>, char_start: usize| {
        let trimmed = text.trim();
        if !trimmed.is_empty() {
            if trimmed.len() < min_chunk_chars && !all_chunks.is_empty() {
                // Backward Merge
                let last_chunk: &mut NewChunk = all_chunks.last_mut().unwrap();
                last_chunk.text.push('\n');
                last_chunk.text.push_str(trimmed);
                last_chunk.char_end += (1 + trimmed.len()) as i64;
                text.clear();
                return;
            }
            let sig_text: String = trimmed.chars().take(80).collect();
            let mut meta_map = serde_json::Map::new();
            if let Some(a) = abschnitt {
                meta_map.insert("section".to_string(), serde_json::Value::String(a.clone()));
            } else {
                meta_map.insert("section".to_string(), serde_json::Value::String("".to_string()));
            }
            if let Some(t) = titel {
                meta_map.insert("title".to_string(), serde_json::Value::String(t.clone()));
            } else {
                meta_map.insert("title".to_string(), serde_json::Value::String("".to_string()));
            }
            if let Some(p) = page {
                meta_map.insert("page".to_string(), serde_json::json!(p + 1));
            }
            let metadata = serde_json::Value::Object(meta_map).to_string();

            all_chunks.push(NewChunk {
                context_id,
                document_id,
                chunk_index,
                char_start: char_start as i64,
                char_end: (char_start + text.len()) as i64,
                text: text.clone(),
                signature: Some(sig_text),
                is_omitted: false,
                metadata,
            });
            chunk_index += 1;
            text.clear();
        }
    };

    let page_count = doc.page_count().unwrap_or(0);
    let mut classified_lines = Vec::new();
    
    for page_idx in 0..page_count {
        let chars = doc.extract_chars(page_idx).unwrap_or_default();
        if chars.is_empty() { continue; }

        let mut lines_by_y: BTreeMap<i32, Vec<&pdf_oxide::layout::TextChar>> = BTreeMap::new();
        for ch in &chars {
            let y_key = (ch.bbox.y * 10.0).round() as i32;
            lines_by_y.entry(y_key).or_default().push(ch);
        }

        let total_chars = chars.len();
        let avg_font_size = if total_chars > 0 {
            chars.iter().map(|c| c.font_size).sum::<f32>() / total_chars as f32
        } else {
            12.0
        };

        let mut keys: Vec<i32> = lines_by_y.keys().copied().collect();
        if keys.is_empty() { continue; }
        keys.reverse();

        for &y in &keys {
            let mut line_chars = lines_by_y[&y].clone();
            line_chars.sort_by(|a, b| a.bbox.x.partial_cmp(&b.bbox.x).unwrap());
            
            let mut line_text = String::new();
            let mut bold_chars = 0;
            let mut max_font_size = 0.0_f32;
            let mut prev_x = -1.0;

            for ch in &line_chars {
                if prev_x >= 0.0 && ch.bbox.x - prev_x > ch.font_size * 3.0 {
                    line_text.push_str("    "); 
                }
                line_text.push(ch.char);
                if ch.font_name.to_lowercase().contains("bold") {
                    bold_chars += 1;
                }
                if ch.font_size > max_font_size {
                    max_font_size = ch.font_size;
                }
                prev_x = ch.bbox.x + ch.bbox.width;
            }

            let is_mostly_bold = bold_chars > line_chars.len() / 2;
            let line_text_trimmed = line_text.trim();
            if line_text_trimmed.is_empty() {
                continue;
            }

            if line_text_trimmed.contains("...") && line_text_trimmed.chars().last().unwrap().is_numeric() {
                continue; 
            }

            let mut matched_role = None;
            let mut extracted_abschnitt = None;
            let mut extracted_titel = None;
            
            for pat in &compiled_patterns {
                if let Some(caps) = pat.regex.captures(line_text_trimmed) {
                    matched_role = Some(pat.role.clone());
                    
                    if pat.role.starts_with("heading") {
                        if caps.len() >= 3 {
                            extracted_abschnitt = caps.get(1).map(|m| m.as_str().trim().to_string());
                            extracted_titel = caps.get(2).map(|m| m.as_str().trim().to_string());
                        } else if caps.len() == 2 {
                            extracted_abschnitt = caps.get(1).map(|m| m.as_str().trim().to_string());
                        } else {
                            extracted_titel = Some(line_text_trimmed.to_string());
                        }
                    }
                    break;
                }
            }
            
            if matched_role.is_none() && (max_font_size > avg_font_size * 1.2 || is_mostly_bold) && line_text_trimmed.len() < 100 {
                matched_role = Some("heading_l1".to_string());
                extracted_titel = Some(line_text_trimmed.to_string());
            }

            if matched_role.as_deref() == Some("ignore") {
                continue;
            }

            classified_lines.push(ClassifiedLine {
                text: line_text_trimmed.to_string(),
                y,
                page_idx,
                max_font_size,
                matched_role,
                extracted_abschnitt,
                extracted_titel,
            });
        }
    }
    
    let mut prev_y = -1;
    let mut prev_page = 0;
    
    for line in classified_lines.iter() {
        let line_text_trimmed = line.text.trim();
        
        let is_new_paragraph = if prev_y >= 0 && line.page_idx == prev_page {
            let delta_y = (prev_y - line.y).abs() as f32 / 10.0;
            delta_y > line.max_font_size * 1.5
        } else {
            false
        };
        
        prev_y = line.y;
        prev_page = line.page_idx;

        let role = line.matched_role.as_deref();
        let is_heading = role.map_or(false, |r| r.starts_with("heading"));
        let is_definition = role == Some("definition");
        
        let starts_with_number = line_text_trimmed.chars().next().map_or(false, |c| c.is_ascii_digit());
        let is_list_item = is_new_paragraph && starts_with_number;
        
        let mut force_flush = false;
        
        if is_heading {
            if !current_chunk_text.trim().is_empty() {
                force_flush = true;
            }
        } else if is_definition || is_list_item {
            if current_chunk_text.len() >= min_chunk_chars {
                force_flush = true;
            }
        } else if is_new_paragraph && current_chunk_text.len() >= max_chunk_chars {
            force_flush = true;
        }

        if force_flush {
            flush_chunk(&mut current_chunk_text, &current_abschnitt, &current_titel, current_chunk_start_page, current_chunk_char_start);
            current_chunk_start_page = None;
            current_chunk_char_start = total_chars_processed;
        }
        
        if is_heading {
            if let Some(ref a) = line.extracted_abschnitt {
                current_abschnitt = Some(a.clone());
            }
            if let Some(ref t) = line.extracted_titel {
                current_titel = Some(t.clone());
            } else if line.extracted_abschnitt.is_none() {
                current_titel = Some(line_text_trimmed.to_string());
            }
        }

        if current_chunk_text.is_empty() {
            current_chunk_start_page = Some(line.page_idx);
            current_chunk_char_start = total_chars_processed;
        }
        
        current_chunk_text.push_str(line_text_trimmed);
        current_chunk_text.push('\n');
        
        total_chars_processed += line_text_trimmed.len() + 1;
        
        if current_chunk_text.len() > max_chunk_chars * 2 {
            let sentences = split_sentences(&current_chunk_text);
            if sentences.len() > 1 {
                let mut split_idx = sentences.len() - 1;
                for (j, s) in sentences.iter().enumerate() {
                    if s.byte_end > max_chunk_chars {
                        split_idx = j.max(1);
                        break;
                    }
                }
                
                let keep_text = current_chunk_text[..sentences[split_idx].byte_end].to_string();
                let remainder_text = current_chunk_text[sentences[split_idx].byte_end..].to_string();
                
                let mut temp = keep_text;
                flush_chunk(&mut temp, &current_abschnitt, &current_titel, current_chunk_start_page, current_chunk_char_start);
                
                current_chunk_text = remainder_text.trim_start().to_string();
                current_chunk_start_page = Some(line.page_idx);
                current_chunk_char_start = total_chars_processed - current_chunk_text.len();
            }
        }
    }

    flush_chunk(&mut current_chunk_text, &current_abschnitt, &current_titel, current_chunk_start_page, current_chunk_char_start);

    Ok(all_chunks)
}
