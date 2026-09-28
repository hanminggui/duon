use crate::error::ParserError;
use duon_core::ir::{
    BBox, Block, BlockType, DocumentIR, Page, ParserMeta, SourceMeta, Table, TableCell, TextItem,
};
use serde_json::Value;

pub struct LiteParseAdapter;

impl LiteParseAdapter {
    pub fn convert(
        raw: &Value,
        doc_sha256: &str,
        mime_type: &str,
        filename: Option<&str>,
        parser_version: &str,
        config_hash: &str,
    ) -> Result<DocumentIR, ParserError> {
        let raw_pages = raw
            .get("pages")
            .and_then(|v| v.as_array())
            .ok_or_else(|| {
                ParserError::CorruptedDocument(
                    "LiteParse raw output missing 'pages' array".to_string(),
                )
            })?;

        let mut pages = Vec::with_capacity(raw_pages.len());

        for (p_idx, p_val) in raw_pages.iter().enumerate() {
            let page_num = p_val
                .get("page")
                .and_then(|v| v.as_u64())
                .map(|v| v as u32)
                .unwrap_or((p_idx + 1) as u32);

            let width = p_val
                .get("width")
                .and_then(|v| v.as_f64())
                .unwrap_or(595.32);
            let height = p_val
                .get("height")
                .and_then(|v| v.as_f64())
                .unwrap_or(841.92);

            // Parse text items
            let raw_items = p_val
                .get("text_items")
                .and_then(|v| v.as_array())
                .cloned()
                .unwrap_or_default();

            let mut items = Vec::with_capacity(raw_items.len());
            for (i_idx, item_val) in raw_items.iter().enumerate() {
                let id = format!("p{}_i{:04}", page_num, i_idx + 1);
                let text = item_val
                    .get("text")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();

                let bbox = parse_bbox_from_value(item_val.get("bbox").or(Some(item_val)))?;
                let confidence = item_val.get("confidence").and_then(|v| v.as_f64());

                items.push(TextItem {
                    id,
                    r#type: "text".to_string(),
                    text,
                    bbox,
                    confidence,
                });
            }

            // Parse blocks
            let raw_blocks = p_val
                .get("blocks")
                .and_then(|v| v.as_array())
                .cloned()
                .unwrap_or_default();

            let mut blocks = Vec::with_capacity(raw_blocks.len());
            for (b_idx, b_val) in raw_blocks.iter().enumerate() {
                let id = format!("p{}_b{:04}", page_num, b_idx + 1);
                let text = b_val
                    .get("text")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();

                let bbox = parse_bbox_from_value(b_val.get("bbox").or(Some(b_val)))?;
                let block_type = parse_block_type(b_val.get("type").and_then(|v| v.as_str()));

                // Map text item indices if provided, or derive from bbox overlap
                let item_ids = if let Some(indices) = b_val.get("text_item_indices").and_then(|v| v.as_array()) {
                    indices
                        .iter()
                        .filter_map(|idx| idx.as_u64().map(|i| format!("p{}_i{:04}", page_num, i + 1)))
                        .collect()
                } else if let Some(existing_ids) = b_val.get("item_ids").and_then(|v| v.as_array()) {
                    existing_ids
                        .iter()
                        .filter_map(|v| v.as_str().map(|s| s.to_string()))
                        .collect()
                } else {
                    Vec::new()
                };

                blocks.push(Block {
                    id,
                    r#type: block_type,
                    text,
                    bbox,
                    item_ids,
                });
            }

            // If blocks array was absent in raw parser output, synthesize blocks from text items
            if blocks.is_empty() && !items.is_empty() {
                for (i_idx, item) in items.iter().enumerate() {
                    blocks.push(Block {
                        id: format!("p{}_b{:04}", page_num, i_idx + 1),
                        r#type: BlockType::Paragraph,
                        text: item.text.clone(),
                        bbox: item.bbox.clone(),
                        item_ids: vec![item.id.clone()],
                    });
                }
            }

            // Parse tables
            let raw_tables = p_val
                .get("tables")
                .and_then(|v| v.as_array())
                .cloned()
                .unwrap_or_default();

            let mut tables = Vec::with_capacity(raw_tables.len());
            for (t_idx, t_val) in raw_tables.iter().enumerate() {
                let id = format!("p{}_t{:04}", page_num, t_idx + 1);
                let bbox = parse_bbox_from_value(t_val.get("bbox").or(Some(t_val)))?;
                let rows = t_val.get("rows").and_then(|v| v.as_u64()).unwrap_or(1) as usize;
                let cols = t_val.get("cols").and_then(|v| v.as_u64()).unwrap_or(1) as usize;

                let mut cells = Vec::new();
                if let Some(raw_cells) = t_val.get("cells").and_then(|v| v.as_array()) {
                    for cell_val in raw_cells {
                        let row_index = cell_val.get("row_index").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
                        let col_index = cell_val.get("col_index").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
                        let row_span = cell_val.get("row_span").and_then(|v| v.as_u64()).unwrap_or(1) as usize;
                        let col_span = cell_val.get("col_span").and_then(|v| v.as_u64()).unwrap_or(1) as usize;
                        let text = cell_val.get("text").and_then(|v| v.as_str()).unwrap_or("").to_string();
                        let cell_bbox = parse_bbox_from_value(cell_val.get("bbox").or(Some(cell_val)))?;
                        let item_ids = cell_val
                            .get("item_ids")
                            .and_then(|v| v.as_array())
                            .map(|arr| arr.iter().filter_map(|s| s.as_str().map(|s| s.to_string())).collect())
                            .unwrap_or_default();

                        cells.push(TableCell {
                            row_index,
                            col_index,
                            row_span,
                            col_span,
                            text,
                            bbox: cell_bbox,
                            item_ids,
                        });
                    }
                }

                tables.push(Table {
                    id,
                    bbox,
                    rows,
                    cols,
                    cells,
                });
            }

            pages.push(Page {
                page_number: page_num,
                width: (width * 100.0).round() / 100.0,
                height: (height * 100.0).round() / 100.0,
                items,
                blocks,
                tables,
                images: Vec::new(),
            });
        }

        let doc_ir = DocumentIR {
            document_id: format!("doc_{}", doc_sha256),
            source: SourceMeta {
                sha256: doc_sha256.to_string(),
                mime_type: mime_type.to_string(),
                filename: filename.map(|s| s.to_string()),
                size_bytes: None,
            },
            parser: ParserMeta {
                name: "liteparse".to_string(),
                version: parser_version.to_string(),
                config_hash: config_hash.to_string(),
            },
            pages,
        };

        doc_ir.validate()?;
        Ok(doc_ir)
    }
}

fn parse_bbox_from_value(val: Option<&Value>) -> Result<BBox, ParserError> {
    let val = val.ok_or_else(|| ParserError::CorruptedDocument("Missing bbox".to_string()))?;

    // 1. Direct array [x0, y0, x1, y1]
    if let Some(arr) = val.as_array() {
        if arr.len() == 4 {
            let x0 = arr[0].as_f64().unwrap_or(0.0).max(0.0);
            let y0 = arr[1].as_f64().unwrap_or(0.0).max(0.0);
            let x1 = arr[2].as_f64().unwrap_or(0.0).max(x0);
            let y1 = arr[3].as_f64().unwrap_or(0.0).max(y0);

            let bbox = BBox::new(
                (x0 * 100.0).round() / 100.0,
                (y0 * 100.0).round() / 100.0,
                (x1 * 100.0).round() / 100.0,
                (y1 * 100.0).round() / 100.0,
            );
            bbox.validate()?;
            return Ok(bbox);
        }
    }

    // 2. Nested "bbox" field
    if let Some(nested_bbox) = val.get("bbox") {
        return parse_bbox_from_value(Some(nested_bbox));
    }

    // 3. Object with x, y, width, height (standard liteparse CLI output)
    if let (Some(x), Some(y), Some(w), Some(h)) = (
        val.get("x").and_then(|v| v.as_f64()),
        val.get("y").and_then(|v| v.as_f64()),
        val.get("width").and_then(|v| v.as_f64()),
        val.get("height").and_then(|v| v.as_f64()),
    ) {
        let x0 = x.max(0.0);
        let y0 = y.max(0.0);
        let x1 = (x + w).max(x0);
        let y1 = (y + h).max(y0);

        let bbox = BBox::new(
            (x0 * 100.0).round() / 100.0,
            (y0 * 100.0).round() / 100.0,
            (x1 * 100.0).round() / 100.0,
            (y1 * 100.0).round() / 100.0,
        );
        bbox.validate()?;
        return Ok(bbox);
    }

    Err(ParserError::CorruptedDocument(
        "Missing or invalid bbox array / coordinates".to_string(),
    ))
}

fn parse_block_type(s: Option<&str>) -> BlockType {
    match s {
        Some("heading") => BlockType::Heading,
        Some("paragraph") => BlockType::Paragraph,
        Some("list") => BlockType::List,
        Some("table") => BlockType::Table,
        Some("figure") => BlockType::Figure,
        Some("header") => BlockType::Header,
        Some("footer") => BlockType::Footer,
        Some("footnote") => BlockType::Footnote,
        _ => BlockType::Other,
    }
}

