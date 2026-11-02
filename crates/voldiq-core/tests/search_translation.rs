use voldiq_core::{index::Query, search::translate};

#[test]
fn translates_explicit_spanish_and_english_conditions_with_the_shared_parser() {
    for (source, expected) in [
        ("archivos mayores de 1 GiB", "type:file size>1GiB"),
        (
            "files at least 1.5GB and smaller than 2GiB",
            "type:file size>=1.5GB size<2GiB",
        ),
        (
            "vídeos modificados entre 2024-02-29 y 2024-03-01",
            "type:file category:video modified>=2024-02-29 modified<=2024-03-01",
        ),
        (
            "images modified before 2024-03-01",
            "type:file category:image modified<2024-03-01",
        ),
        (
            "modified between 2024-03-01T12:00:00Z and 2024-03-01",
            "modified>=2024-03-01T12:00:00Z modified<=2024-03-01",
        ),
        (
            "archivos llamados \"Informe final\" sin extensión",
            "type:file name:\"Informe final\" ext:none",
        ),
        (
            "files path contains \"C:\\My Files\" extension mp4",
            "type:file path:\"C:\\My Files\" ext:\"mp4\"",
        ),
        ("cloud placeholders", "cloud:placeholder"),
    ] {
        let proposal = translate(source).unwrap();
        assert_eq!(proposal.expression, expected, "{source}");
        assert!(proposal.unparsed.is_empty(), "{source}");
        assert!(
            Query {
                expression: proposal.expression,
                ..Query::default()
            }
            .compile()
            .is_ok()
        );
    }
}

#[test]
fn preserves_uninterpreted_words_and_original_unicode_byte_spans() {
    for source in [
        "vídeos antiguos",
        "files not larger than 1GiB",
        "cachés de una aplicación",
        "files OR folders",
        "files delete now",
    ] {
        let proposal = translate(source).unwrap();
        assert!(!proposal.unparsed.is_empty(), "{source}");
        for span in &proposal.unparsed {
            assert_eq!(&source[span.start..span.end], span.text);
        }
    }
    let old = translate("vídeos antiguos").unwrap();
    assert_eq!(old.expression, "type:file category:video");
    assert_eq!(old.unparsed[0].text, "antiguos");
    let unknown = translate("cachés de una aplicación").unwrap();
    assert_eq!(unknown.expression, "");
    assert_eq!(unknown.unparsed[0].text, "cachés de una aplicación");
    let empty = translate("   ").unwrap();
    assert_eq!(empty.expression, "");
    assert_eq!(empty.conditions, 0);
}

#[test]
fn rejects_invalid_bounds_dates_quoting_and_oversized_input_without_echoing_values() {
    for source in [
        "files larger than 18446744073709551616B",
        "files larger than -1GB",
        "modified before 2023-02-29",
        "modified between 2024-03-01 and 2024-02-29",
        "named \"unclosed",
        "named \"\"",
        "files\0secret",
    ] {
        let error = translate(source).expect_err(source);
        assert!(error.offset <= source.len());
        assert!(!error.message.contains("secret"));
    }
    assert!(translate(&"a".repeat(4097)).is_err());
    assert!(translate(&"a ".repeat(129)).is_err());
    assert!(translate(&"é".repeat(2049)).is_err());
    assert!(translate(&"files ".repeat(65)).is_err());
}
