use duon_core::ir::DocumentIR;

pub struct ContextBuilder;

impl ContextBuilder {
    /// Formats Document IR into a clean, compact context with item ID markers.
    pub fn build(doc_ir: &DocumentIR) -> String {
        let mut out = String::new();

        for page in &doc_ir.pages {
            out.push_str(&format!("--- PAGE {} ---\n", page.page_number));

            // If blocks exist, we output blocks with their items
            if !page.blocks.is_empty() {
                for block in &page.blocks {
                    out.push_str(&format!("[{}] {}\n", block.id, block.text));
                    for item_id in &block.item_ids {
                        if let Some(item) = page.items.iter().find(|i| &i.id == item_id) {
                            out.push_str(&format!("  [{}] {}\n", item.id, item.text));
                        }
                    }
                }
            } else {
                // Otherwise list individual text items
                for item in &page.items {
                    out.push_str(&format!("[{}] {}\n", item.id, item.text));
                }
            }

            out.push('\n');
        }

        out
    }
}
