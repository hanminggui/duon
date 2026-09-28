use duon_parser::cli::LiteParseCliParser;
use duon_parser::traits::{DocumentParser, ParseOptions};
use std::fs;
use std::path::Path;

#[tokio::test]
async fn test_liteparse_cli_parser_on_real_pdf() {
    let parser = LiteParseCliParser::new();
    if !parser.is_available().await {
        println!("lit CLI not found in PATH, skipping real CLI test");
        return;
    }

    let pdf_path = Path::new("/Users/aduang/Downloads/2026-G-D-030.pdf");
    if !pdf_path.exists() {
        println!("Test PDF does not exist at {:?}, skipping", pdf_path);
        return;
    }

    let bytes = fs::read(pdf_path).expect("Failed to read test PDF");
    let opts = ParseOptions {
        ocr_enabled: false,
        filename: Some(pdf_path.to_str().unwrap().to_string()),
        mime_type: Some("application/pdf".to_string()),
    };

    let ir = parser.parse(&bytes, &opts).await.expect("Failed to parse via lit CLI");
    assert_eq!(ir.pages.len(), 1);

    let page1 = &ir.pages[0];
    println!("Total text items parsed: {}", page1.items.len());
    println!("Total blocks synthesized: {}", page1.blocks.len());

    // Verify it contains all items, not just the 8 items from mock
    assert!(page1.items.len() >= 20, "Expected at least 20 text items, got {}", page1.items.len());

    // Check specific texts from the full order
    let texts: Vec<&str> = page1.items.iter().map(|it| it.text.as_str()).collect();
    assert!(texts.iter().any(|t| t.contains("鸣潮 6_心月狐")));
    assert!(texts.iter().any(|t| t.contains("1600")));
    assert!(texts.iter().any(|t| t.contains("付款信息")));
    assert!(texts.iter().any(|t| t.contains("甲方盖章")));
    assert!(texts.iter().any(|t| t.contains("乙方盖章")));

    assert!(ir.validate().is_ok());
}
