use thiserror::Error;

#[derive(Error, Debug, PartialEq)]
pub enum CoreError {
    #[error("Invalid bounding box: x0 ({x0}) > x1 ({x1}) or y0 ({y0}) > y1 ({y1})")]
    InvalidBBox {
        x0: f64,
        y0: f64,
        x1: f64,
        y1: f64,
    },

    #[error("Invalid page dimensions: width ({width}) <= 0 or height ({height}) <= 0 on page {page}")]
    InvalidPageDimensions {
        page: u32,
        width: f64,
        height: f64,
    },

    #[error("Invalid ID format: '{0}', expected pattern like 'p1_i0001'")]
    InvalidIdFormat(String),

    #[error("Validation error: {0}")]
    Validation(String),

    #[error("Serialization error: {0}")]
    Serialization(String),
}
