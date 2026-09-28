use duon_core::ir::{BBox, DocumentIR, TextItem};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct EvidenceItem {
    pub page: u32,
    pub item_ids: Vec<String>,
    pub bbox: BBox,
    pub quote: String,
    pub confidence: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ValidationError {
    pub path: String,
    pub message: String,
    pub code: String,
}

pub struct EvidenceBinder;

impl EvidenceBinder {
    /// Binds LLM citations to real document locations and strips out hallucinated IDs.
    pub fn bind(
        doc_ir: &DocumentIR,
        citations: &HashMap<String, Vec<String>>,
    ) -> (HashMap<String, Vec<EvidenceItem>>, Vec<ValidationError>) {
        let mut bound_evidence = HashMap::new();
        let mut errors = Vec::new();

        // Index all items by ID for quick O(1) lookup
        let mut item_map: HashMap<&str, (&TextItem, u32)> = HashMap::new();
        for page in &doc_ir.pages {
            for item in &page.items {
                item_map.insert(&item.id, (item, page.page_number));
            }
        }

        for (pointer, ids) in citations {
            let mut valid_items = Vec::new();
            let mut page_num = None;

            for id in ids {
                if let Some(&(item, page)) = item_map.get(id.as_str()) {
                    if page_num.is_none() {
                        page_num = Some(page);
                    }
                    valid_items.push((item, page));
                } else {
                    errors.push(ValidationError {
                        path: pointer.clone(),
                        message: format!("Citation ID '{}' does not exist in document IR", id),
                        code: "HALLUCINATED_EVIDENCE_ID".to_string(),
                    });
                }
            }

            if !valid_items.is_empty() {
                let target_page = page_num.unwrap_or(1);
                let quotes: Vec<&str> = valid_items.iter().map(|(it, _)| it.text.as_str()).collect();
                let joined_quote = quotes.join(" ");

                // Compute union bounding box
                let mut min_x = f64::INFINITY;
                let mut min_y = f64::INFINITY;
                let mut max_x = f64::NEG_INFINITY;
                let mut max_y = f64::NEG_INFINITY;
                let mut total_conf = 0.0;
                let mut conf_count = 0;

                for (item, _) in &valid_items {
                    min_x = min_x.min(item.bbox.x0);
                    min_y = min_y.min(item.bbox.y0);
                    max_x = max_x.max(item.bbox.x1);
                    max_y = max_y.max(item.bbox.y1);

                    if let Some(c) = item.confidence {
                        total_conf += c;
                        conf_count += 1;
                    }
                }

                let avg_conf = if conf_count > 0 {
                    total_conf / (conf_count as f64)
                } else {
                    0.99
                };

                let union_bbox = BBox::new(min_x, min_y, max_x, max_y);
                let item_id_list: Vec<String> = valid_items.iter().map(|(it, _)| it.id.clone()).collect();

                let item = EvidenceItem {
                    page: target_page,
                    item_ids: item_id_list,
                    bbox: union_bbox,
                    quote: joined_quote,
                    confidence: avg_conf,
                };

                bound_evidence.entry(pointer.clone()).or_insert_with(Vec::new).push(item);
            }
        }

        (bound_evidence, errors)
    }
}
