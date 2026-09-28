use duon_core::ir::BBox;
use duon_parser::adapter::LiteParseAdapter;
use serde_json::json;

#[test]
fn test_liteparse_adapter_conversion() {
    let raw_liteparse = json!({
        "pages": [
            {
                "page": 1,
                "width": 595.32,
                "height": 841.92,
                "text_items": [
                    {
                        "text": "委托制作采购订单",
                        "bbox": [210.50, 45.20, 385.00, 72.80],
                        "confidence": 0.99
                    },
                    {
                        "text": "订单编号：2026-G-D-030",
                        "bbox": [388.63, 142.85, 521.90, 154.49],
                        "confidence": 0.98
                    }
                ],
                "blocks": [
                    {
                        "type": "heading",
                        "text": "委托制作采购订单",
                        "bbox": [210.50, 45.20, 385.00, 72.80],
                        "text_item_indices": [0]
                    }
                ]
            }
        ]
    });

    let doc_sha256 = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";
    let ir = LiteParseAdapter::convert(
        &raw_liteparse,
        doc_sha256,
        "application/pdf",
        Some("order.pdf"),
        "2.14.4",
        "default_config",
    )
    .expect("Conversion from LiteParse raw output should succeed");

    assert_eq!(ir.document_id, format!("doc_{}", doc_sha256));
    assert_eq!(ir.source.sha256, doc_sha256);
    assert_eq!(ir.parser.name, "liteparse");
    assert_eq!(ir.parser.version, "2.14.4");

    assert_eq!(ir.pages.len(), 1);
    let p1 = &ir.pages[0];
    assert_eq!(p1.items.len(), 2);
    assert_eq!(p1.items[0].id, "p1_i0001");
    assert_eq!(p1.items[0].bbox, BBox::new(210.50, 45.20, 385.00, 72.80));
    assert_eq!(p1.items[1].id, "p1_i0002");

    assert_eq!(p1.blocks.len(), 1);
    assert_eq!(p1.blocks[0].id, "p1_b0001");
    assert_eq!(p1.blocks[0].item_ids, vec!["p1_i0001"]);

    assert!(ir.validate().is_ok());
}

#[test]
fn test_liteparse_adapter_conversion_with_xywh_format() {
    let raw_lit_cli_json = json!({
        "pages": [
            {
                "page": 1,
                "width": 595.32,
                "height": 841.92,
                "text_items": [
                    {
                        "text": "委托制作采购订单",
                        "x": 210.50,
                        "y": 45.20,
                        "width": 174.50,
                        "height": 27.60,
                        "confidence": 1
                    },
                    {
                        "text": "订单编号：2026-G-D-030",
                        "x": 388.63,
                        "y": 142.85,
                        "width": 133.27,
                        "height": 11.64,
                        "confidence": 1
                    }
                ]
            }
        ]
    });

    let ir = LiteParseAdapter::convert(
        &raw_lit_cli_json,
        "test_hash",
        "application/pdf",
        Some("test.pdf"),
        "2.0.0",
        "default_config",
    )
    .expect("Conversion from lit CLI output should succeed");

    assert_eq!(ir.pages[0].items.len(), 2);
    assert_eq!(ir.pages[0].items[0].bbox, BBox::new(210.50, 45.20, 385.00, 72.80));
    assert_eq!(ir.pages[0].items[1].bbox, BBox::new(388.63, 142.85, 521.90, 154.49));
    // Blocks should be synthesized automatically
    assert_eq!(ir.pages[0].blocks.len(), 2);
    assert_eq!(ir.pages[0].blocks[0].item_ids, vec!["p1_i0001"]);
    assert_eq!(ir.pages[0].blocks[1].item_ids, vec!["p1_i0002"]);
    assert!(ir.validate().is_ok());
}

