use crate::error::CoreError;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// Bounding Box in absolute points with Top-Left origin: [x0, y0, x1, y1]
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BBox {
    pub x0: f64,
    pub y0: f64,
    pub x1: f64,
    pub y1: f64,
}

impl BBox {
    pub fn new(x0: f64, y0: f64, x1: f64, y1: f64) -> Self {
        Self { x0, y0, x1, y1 }
    }

    pub fn validate(&self) -> Result<(), CoreError> {
        if self.x0 > self.x1 || self.y0 > self.y1 {
            return Err(CoreError::InvalidBBox {
                x0: self.x0,
                y0: self.y0,
                x1: self.x1,
                y1: self.y1,
            });
        }
        Ok(())
    }

    pub fn width(&self) -> f64 {
        self.x1 - self.x0
    }

    pub fn height(&self) -> f64 {
        self.y1 - self.y0
    }
}

impl Serialize for BBox {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        [self.x0, self.y0, self.x1, self.y1].serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for BBox {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let arr = <[f64; 4]>::deserialize(deserializer)?;
        Ok(BBox::new(arr[0], arr[1], arr[2], arr[3]))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SourceMeta {
    pub sha256: String,
    pub mime_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub filename: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub size_bytes: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ParserMeta {
    pub name: String,
    pub version: String,
    pub config_hash: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TextItem {
    pub id: String,
    pub r#type: String,
    pub text: String,
    pub bbox: BBox,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub confidence: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum BlockType {
    Heading,
    Paragraph,
    List,
    Table,
    Figure,
    Header,
    Footer,
    Footnote,
    Other,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Block {
    pub id: String,
    pub r#type: BlockType,
    pub text: String,
    pub bbox: BBox,
    pub item_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TableCell {
    pub row_index: usize,
    pub col_index: usize,
    #[serde(default = "default_span")]
    pub row_span: usize,
    #[serde(default = "default_span")]
    pub col_span: usize,
    pub text: String,
    pub bbox: BBox,
    #[serde(default)]
    pub item_ids: Vec<String>,
}

fn default_span() -> usize {
    1
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Table {
    pub id: String,
    pub bbox: BBox,
    pub rows: usize,
    pub cols: usize,
    pub cells: Vec<TableCell>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ImageItem {
    pub id: String,
    pub bbox: BBox,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub format: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Page {
    pub page_number: u32,
    pub width: f64,
    pub height: f64,
    pub items: Vec<TextItem>,
    #[serde(default)]
    pub blocks: Vec<Block>,
    #[serde(default)]
    pub tables: Vec<Table>,
    #[serde(default)]
    pub images: Vec<ImageItem>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DocumentIR {
    pub document_id: String,
    pub source: SourceMeta,
    pub parser: ParserMeta,
    pub pages: Vec<Page>,
}

impl DocumentIR {
    pub fn validate(&self) -> Result<(), CoreError> {
        let mut seen_ids = std::collections::HashSet::new();

        for page in &self.pages {
            if page.width <= 0.0 || page.height <= 0.0 {
                return Err(CoreError::InvalidPageDimensions {
                    page: page.page_number,
                    width: page.width,
                    height: page.height,
                });
            }

            let mut page_item_ids = std::collections::HashSet::new();

            for item in &page.items {
                if !seen_ids.insert(&item.id) {
                    return Err(CoreError::Validation(format!(
                        "Duplicate ID detected across document: '{}'",
                        item.id
                    )));
                }
                page_item_ids.insert(&item.id);
                item.bbox.validate()?;
            }

            for block in &page.blocks {
                if !seen_ids.insert(&block.id) {
                    return Err(CoreError::Validation(format!(
                        "Duplicate block ID detected across document: '{}'",
                        block.id
                    )));
                }
                block.bbox.validate()?;

                // Referential integrity: blocks must not reference non-existent items
                for ref_id in &block.item_ids {
                    if !page_item_ids.contains(ref_id) {
                        return Err(CoreError::Validation(format!(
                            "Dangling item_id '{}' referenced in block '{}' on page {}",
                            ref_id, block.id, page.page_number
                        )));
                    }
                }
            }

            for table in &page.tables {
                if !seen_ids.insert(&table.id) {
                    return Err(CoreError::Validation(format!(
                        "Duplicate table ID detected: '{}'",
                        table.id
                    )));
                }
                table.bbox.validate()?;
                for cell in &table.cells {
                    cell.bbox.validate()?;
                    for ref_id in &cell.item_ids {
                        if !page_item_ids.contains(ref_id) {
                            return Err(CoreError::Validation(format!(
                                "Dangling item_id '{}' referenced in table cell '{}' on page {}",
                                ref_id, table.id, page.page_number
                            )));
                        }
                    }
                }
            }

            for img in &page.images {
                if !seen_ids.insert(&img.id) {
                    return Err(CoreError::Validation(format!(
                        "Duplicate image ID detected: '{}'",
                        img.id
                    )));
                }
                img.bbox.validate()?;
            }
        }
        Ok(())
    }
}
