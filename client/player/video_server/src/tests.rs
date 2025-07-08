use super::*;

#[test]
fn test_range_struct_methods() {
    let file_size = 1000;

    // Both Some
    let r1 = Range {
        start: Some(100),
        end: Some(200),
    };
    assert_eq!(r1.start(file_size), 100);
    assert_eq!(r1.end(file_size), 200);
    assert_eq!(r1.length(file_size), 101);

    // Start None, End Some (suffix length)
    let r2 = Range {
        start: None,
        end: Some(100),
    }; // bytes=-100 means last 100 bytes
    assert_eq!(r2.start(file_size), 900); // file_size - 100
    assert_eq!(r2.end(file_size), 999); // file_size - 1
    assert_eq!(r2.length(file_size), 100);

    // Start Some, End None
    let r3 = Range {
        start: Some(900),
        end: None,
    }; // bytes=900-
    assert_eq!(r3.start(file_size), 900);
    assert_eq!(r3.end(file_size), 999); // min(u64::MAX, file_size - 1)
    assert_eq!(r3.length(file_size), 100);

    // Both None (full range, though your Range::default might not be used like this by parser)
    // According to your code, this case in process_request becomes Range::default() -> start=0, end=size-1
    let r_default = Range::default();
    assert_eq!(r_default.start(file_size), 0);
    assert_eq!(r_default.end(file_size), file_size - 1);
    assert_eq!(r_default.length(file_size), file_size);

    // End capped by file_size
    let r4 = Range {
        start: Some(100),
        end: Some(2000),
    };
    assert_eq!(r4.start(file_size), 100);
    assert_eq!(r4.end(file_size), 999); // Capped at file_size - 1
    assert_eq!(r4.length(file_size), 900);
}

#[test]
fn test_parse_range() {
    let res = Range {
        start: Some(0),
        end: Some(100),
    };
    assert_eq!(parse_range("0-100"), Ok(("", res)));
    let res = Range {
        start: Some(500),
        end: None,
    };
    assert_eq!(parse_range("500-"), Ok(("", res)));
    let res = Range {
        start: None,
        end: Some(200),
    };
    assert_eq!(parse_range("-200"), Ok(("", res)));
}

#[test]
fn test_parse_range_header_valid() {
    let input = "Range: bytes=0-499";
    let expected = vec![Range {
        start: Some(0),
        end: Some(499),
    }];
    assert_eq!(parse_range_header(input), Ok(("", expected)));
}

#[test]
fn test_parse_range_header_multiple() {
    let input = "Range: bytes=0-499, 500-999";
    let expected = vec![
        Range {
            start: Some(0),
            end: Some(499),
        },
        Range {
            start: Some(500),
            end: Some(999),
        },
    ];
    assert_eq!(parse_range_header(input), Ok(("", expected)));
}

#[test]
fn test_parse_range_header_case_insensitive_and_spaces() {
    let input = "range:    bytes=0-10";
    let expected = vec![Range {
        start: Some(0),
        end: Some(10),
    }];
    assert_eq!(parse_range_header(input), Ok(("", expected)));
}

#[test]
fn test_parse_range_header_invalid() {
    let input = "Range: 0-499"; // Missing "bytes="
    assert!(parse_range_header(input).is_err());

    let input = "Range: bytes=abc-def";
    assert!(parse_range_header(input).is_err());
}
