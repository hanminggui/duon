pub mod canonical;
pub mod error;
pub mod ir;

pub use canonical::{canonicalize_json, compute_result_hash};
pub use error::CoreError;
pub use ir::{BBox, Block, BlockType, DocumentIR, Page, ParserMeta, SourceMeta, Table, TableCell, TextItem};
