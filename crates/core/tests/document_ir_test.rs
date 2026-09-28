use duon_core::ir::{BBox, DocumentIR};
use std::fs;
use std::path::Path;

#[test]
fn test_deserialize_sample_order_ir() {
    let mock_path = Path::new("../../specs/mocks/sample_order_ir.json");
    let content = fs::read_to_string(mock_path)
        .or_else(|_| fs::read_to_string("specs/mocks/sample_order_ir.json"))
        .expect("Failed to read sample_order_ir.json mock");

    let doc_ir: DocumentIR = serde_json::from_str(&content)
        .expect("sample_order_ir.json should deserialize into DocumentIR");

    assert_eq!(doc_ir.pages.len(), 1);
    let p1 = &doc_ir.pages[0];
    assert_eq!(p1.page_number, 1);
    assert_eq!(p1.items.len(), 8);
    assert_eq!(p1.blocks.len(), 3);

    // Verify first item
    let first_item = &p1.items[0];
    assert_eq!(first_item.id, "p1_i0001");
    assert_eq!(first_item.text, "委托制作采购订单");
    assert_eq!(first_item.bbox, BBox::new(210.50, 45.20, 385.00, 72.80));

    // Verify validation passes
    assert!(doc_ir.validate().is_ok());
}

#[test]
fn test_bbox_invariants() {
    // Valid bbox
    let valid_bbox = BBox::new(10.0, 20.0, 30.0, 40.0);
    assert!(valid_bbox.validate().is_ok());

    // Inverted X: x0 > x1
    let inv_x = BBox::new(50.0, 20.0, 30.0, 40.0);
    assert!(inv_x.validate().is_err());

    // Inverted Y: y0 > y1
    let inv_y = BBox::new(10.0, 50.0, 30.0, 40.0);
    assert!(inv_y.validate().is_err());
}

#[test]
fn test_referential_integrity_catches_dangling_item_id() {
    let mock_path = Path::new("../../specs/mocks/sample_order_ir.json");
    let content = fs::read_to_string(mock_path)
        .or_else(|_| fs::read_to_string("specs/mocks/sample_order_ir.json"))
        .expect("Failed to read sample_order_ir.json mock");

    let mut doc_ir: DocumentIR = serde_json::from_str(&content).expect("Invalid IR");

    // Introduce a dangling item_id in block
    doc_ir.pages[0].blocks[0].item_ids.push("p1_i9999".to_string());

    let val_res = doc_ir.validate();
    assert!(val_res.is_err());
    let err_msg = val_res.unwrap_err().to_string();
    assert!(err_msg.contains("Dangling item_id 'p1_i9999'"));
}

#[test]
fn test_duplicate_id_rejected() {
    let mock_path = Path::new("../../specs/mocks/sample_order_ir.json");
    let content = fs::read_to_string(mock_path)
        .or_else(|_| fs::read_to_string("specs/mocks/sample_order_ir.json"))
        .expect("Failed to read sample_order_ir.json mock");

    let mut doc_ir: DocumentIR = serde_json::from_str(&content).expect("Invalid IR");

    // Duplicate an item ID
    doc_ir.pages[0].items[1].id = doc_ir.pages[0].items[0].id.clone();

    let val_res = doc_ir.validate();
    assert!(val_res.is_err());
    assert!(val_res.unwrap_err().to_string().contains("Duplicate ID detected"));
}
