// Concern: spells each block of a sheet as its own A1-named grid file | Non-concern: choosing the blocks, reading the source, writing to disk | IO: (&SheetSource + blocks) -> Vec<(name, content)>

use fsa1_ast::Shape;
use fsa1_ast::a1::format_cell;
use fsa1_ast::{ErrKind, Value};
use fsa1_model::format::lex_formatted_number;
use fsa1_model::{Format, display_value, encode_field, lex_literal, spell_block};

use crate::decompose::Block;
use crate::resolve::Resolution;
use crate::scope_block;
use crate::source::{MergedRegion, SheetSource, SourceValue};
use crate::translate::{strip_lead, translate_formula_ctx};
use crate::warnings::UnpackWarning;

/// One file per block, in the canonical file order `r0, c0, r1, c1` — which is also the tie-break
/// deciding which of two files sizes a shared axis. A sheet with no occupancy has no block and so
/// writes nothing at all.
pub fn sheet_files(
    sheet: &SheetSource,
    blocks: &[Block],
    res: &Resolution,
    warnings: &mut Vec<UnpackWarning>,
) -> Vec<(String, String)> {
    for merge in &sheet.merges {
        warnings.push(UnpackWarning::MergedRegionFlattened {
            sheet: sheet.name.clone(),
            region: merged_region(merge),
        });
    }
    scope_block::name_normal_font_losses(sheet, warnings);
    scope_block::name_losses(sheet, warnings);
    let mut blocks = blocks.to_vec();
    blocks.sort_by_key(scope_block::key);
    let geometry = scope_block::assign_geometry(sheet, &blocks, warnings);
    let mut files = Vec::with_capacity(blocks.len());
    for (block, geometry) in blocks.iter().zip(&geometry) {
        let grid = block_grid(sheet, *block, res, warnings);
        let presentation = scope_block::encode(sheet, *block, geometry);
        let shape = Shape {
            rows: block.rows,
            cols: block.cols,
        };
        let content = match presentation {
            Some(p) => format!("{grid}\n{}", spell_block(&p, shape)),
            None => grid,
        };
        files.push((block_name(*block), content));
    }
    files
}

/// The closed A1 range the block fills. A 1x1 block takes its BARE address: `A1:A1` is refused as a
/// degenerate range, so the two spellings are never both legal.
fn block_name(block: Block) -> String {
    let anchor = format_cell(block.col - 1, block.row - 1);
    if block.cols == 1 && block.rows == 1 {
        return anchor;
    }
    let sep = fsa1_model::RANGE_SEP;
    format!(
        "{anchor}{sep}{}",
        format_cell(block.col + block.cols - 2, block.row + block.rows - 2)
    )
}

fn merged_region(region: &MergedRegion) -> String {
    let sep = fsa1_model::RANGE_SEP;
    format!(
        "{}{sep}{}",
        format_cell(region.col, region.row),
        format_cell(region.col + region.cols - 1, region.row + region.rows - 1)
    )
}

/// The block's rectangle filled exactly, one field per coordinate and a blank one where the block
/// spans a cell the sheet never states. A trailing empty FIELD would otherwise be read back as a
/// missing row, the deserializer taking one trailing newline as the file's own; so a grid ending in
/// one keeps a second.
fn block_grid(
    sheet: &SheetSource,
    block: Block,
    res: &Resolution,
    warnings: &mut Vec<UnpackWarning>,
) -> String {
    let mut grid = String::new();
    for row in block.row - 1..block.row - 1 + block.rows {
        if row > block.row - 1 {
            grid.push('\n');
        }
        for col in block.col - 1..block.col - 1 + block.cols {
            if col > block.col - 1 {
                grid.push('\t');
            }
            let cell = sheet
                .cell(col, row)
                .expect("a block is cut from coordinates the sheet states, so it stays inside it");
            let (field, verbatim_reason) = cell_field(&cell.value, res, &sheet.name, row);
            // Lead-stripped, i.e. exactly the `=<body>` written to disk, so the report cannot desync.
            if let (SourceValue::Formula { raw, .. }, Some(reason)) = (&cell.value, verbatim_reason)
            {
                warnings.push(UnpackWarning::FormulaKeptVerbatim {
                    sheet: sheet.name.clone(),
                    cell: format_cell(col, row),
                    source: strip_lead(raw.trim()).to_string(),
                    reason,
                });
            }
            grid.push_str(&encode_field(&field));
        }
    }
    if grid.ends_with('\n') {
        grid.push('\n');
    }
    grid
}

/// The LOGICAL field, before the caller applies [`encode_field`]'s escaping, plus the
/// untranslatability reason when a formula was kept verbatim. Infallible: every cell is representable.
fn cell_field(
    cell: &SourceValue,
    res: &Resolution,
    sheet: &str,
    row: u32,
) -> (String, Option<String>) {
    match cell {
        SourceValue::Blank => (String::new(), None),
        SourceValue::Number(n) | SourceValue::DateSerial(n) => (num_field(*n), None),
        SourceValue::Formatted { value, format } => (format_literal(*value, *format), None),
        SourceValue::Bool(b) => (if *b { "TRUE" } else { "FALSE" }.to_string(), None),
        SourceValue::Error(k) => (error_literal(*k), None),
        SourceValue::Text(s) => (text_field(s), None),
        SourceValue::Formula { raw, format } => {
            let (body, reason) = translate_formula_ctx(raw, res, sheet, row);
            let field = match format {
                Some(f) => format!("{body}~{}", f.code()),
                None => body,
            };
            (field, reason)
        }
    }
}

/// Rust's shortest round-trip `Display`, NOT the General display format, so the literal re-lexes to
/// the same `f64` bit-for-bit.
fn num_field(n: f64) -> String {
    n.to_string()
}

/// The date family is carried as an ISO value plus a `~<code>` marker, since a displayed date's field
/// order is ambiguous; the number family is carried in its self-describing displayed form.
fn format_literal(value: f64, format: Format) -> String {
    match format {
        Format::Date(_) | Format::Time(_) | Format::DateTime(_) => {
            let iso_code = match format {
                Format::Date(_) => "yyyy-mm-dd",
                Format::Time(_) => "hh:mm:ss",
                _ => "yyyy-mm-dd hh:mm:ss",
            };
            format!("{}~{}", render_iso(value, iso_code), format.code())
        }
        _ => display_number_literal(value, format).unwrap_or_else(|| num_field(value)),
    }
}

/// Goes through fsa1-ast's ONE numFmt renderer — the engine `parse_iso_serial` inverts — so the
/// written ISO value re-lexes to the exact serial.
fn render_iso(serial: f64, iso_code: &str) -> String {
    match fsa1_ast::format_value(&Value::Number(serial), iso_code) {
        Value::Text(s) => s,
        other => display_value(&other),
    }
}

/// The exact inverse of fsa1-model's `lex_formatted_number`. `Accounting` and the date family have
/// no number-literal spelling here.
fn display_number_literal(value: f64, format: Format) -> Option<String> {
    let (sign, mag) = if value.is_sign_negative() && value != 0.0 {
        ("-", -value)
    } else {
        ("", value)
    };
    match format {
        Format::Fixed { decimals } => Some(format!("{sign}{}", fixed(mag, decimals))),
        Format::Grouped { decimals } => Some(format!("{sign}{}", grouped(mag, decimals))),
        Format::Percent { decimals } => Some(format!("{sign}{}%", fixed(mag * 100.0, decimals))),
        Format::Currency {
            symbol,
            grouping,
            decimals,
        } => {
            let body = if grouping {
                grouped(mag, decimals)
            } else {
                fixed(mag, decimals)
            };
            Some(format!("{sign}{}{body}", symbol.glyph()))
        }
        Format::Accounting { .. } | Format::Date(_) | Format::Time(_) | Format::DateTime(_) => None,
    }
}

fn fixed(mag: f64, decimals: u8) -> String {
    format!("{:.*}", decimals as usize, mag)
}

fn grouped(mag: f64, decimals: u8) -> String {
    let s = fixed(mag, decimals);
    let (int, frac) = match s.split_once('.') {
        Some((i, f)) => (i, Some(f)),
        None => (s.as_str(), None),
    };
    let int = group_thousands(int);
    match frac {
        Some(f) => format!("{int}.{f}"),
        None => int,
    }
}

fn group_thousands(int: &str) -> String {
    let len = int.len();
    let mut out = String::with_capacity(len + len / 3);
    for (i, ch) in int.chars().enumerate() {
        if i > 0 && (len - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(ch);
    }
    out
}

/// A negative `Accounting` value is `None`: its parenthesized display cannot be recovered from a
/// displayed-form literal. The reader and the strict pre-flight share this ONE remap so they agree on
/// which value literals are carriable.
pub(crate) fn effective_literal_format(format: Format, value: f64) -> Option<Format> {
    match format {
        Format::Accounting { symbol, decimals } => (value >= 0.0).then_some(Format::Currency {
            symbol,
            grouping: true,
            decimals,
        }),
        other => Some(other),
    }
}

/// Whether spelling the value to `format`'s displayed precision and re-lexing recovers the identical
/// `(value, format)`. The caller passes the EFFECTIVE format, never a raw `Accounting`.
pub(crate) fn is_display_exact(value: f64, format: Format) -> bool {
    match format {
        Format::Date(_) | Format::Time(_) | Format::DateTime(_) => true,
        Format::Fixed { decimals: 0 } => value.fract() == 0.0,
        _ => match display_number_literal(value, format) {
            Some(s) => lex_formatted_number(&s) == Some((value, format)),
            None => false,
        },
    }
}

fn error_literal(k: ErrKind) -> String {
    display_value(&Value::Error(k))
}

/// Disambiguates a bare spelling from another value type. `lex_literal` grades a TOKEN and so misses
/// the FIELD-level rule that a leading `=` is a formula; that case needs the apostrophe too.
fn text_field(s: &str) -> String {
    let lexes_bare = lex_literal(s) == (Value::Text(s.to_string()), None) && !s.starts_with('=');
    if lexes_bare {
        s.to_string()
    } else {
        format!("'{s}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    use crate::partition::Decomposition;
    use crate::source::SourceCell;
    use crate::xlsx_style::{
        BorderStyle, FillPattern, HorizontalAlign, StyleTable, XlsxBorder, XlsxFill, XlsxFont,
        XlsxStyle,
    };
    use fsa1_model::{
        Border, BorderLine, Cell, CellStyle, Chars, FontStyle, FontWeight, Points, Rgb, TextAlign,
        WhiteSpace, Workbook, deserialize_tsv,
    };

    /// The production path: the sheet's own occupancy, partitioned by the policy seam.
    fn files(sheet: &SheetSource, warnings: &mut Vec<UnpackWarning>) -> Vec<(String, String)> {
        let blocks = Decomposition::Occupancy.blocks(&crate::occupancy(sheet));
        sheet_files(sheet, &blocks, &Resolution::empty(), warnings)
    }

    /// Every generated file through the REAL loader, which is what `check` runs: a refusal here is a
    /// workbook the tool would not accept back, and a non-canonical spelling is one of them.
    fn accepted(files: &[(String, String)]) {
        let borrowed: Vec<(&str, &str)> = files
            .iter()
            .map(|(n, c)| (n.as_str(), c.as_str()))
            .collect();
        match Workbook::from_tabs(&[("Sheet1", &borrowed)]) {
            Ok(wb) => {
                let lint = wb.lint();
                assert!(lint.is_empty(), "{files:?} lints: {lint:?}");
            }
            Err(d) => panic!("{files:?} must load: {d:?}"),
        }
    }

    fn roundtrip_text(s: &str) {
        let field = encode_field(&text_field(s));
        let grid = deserialize_tsv("A1", &field).expect("a single field deserializes");
        assert_eq!(
            grid.cells[0],
            Cell::Value {
                value: Value::Text(s.to_string()),
                format: None
            },
            "{s:?} spelled as {field:?} did not re-deserialize to the same text",
        );
    }

    #[test]
    fn plain_text_is_bare() {
        assert_eq!(text_field("hello"), "hello".to_string());
        roundtrip_text("hello");
    }

    #[test]
    fn ambiguous_text_is_force_texted() {
        for s in [
            "123", "-4.5", "TRUE", "FALSE", "#REF!", "=A1", "'already", "",
        ] {
            let f = text_field(s);
            assert!(f.starts_with('\''), "{s:?} -> {f:?} should force-text");
            roundtrip_text(s);
        }
    }

    #[test]
    fn tab_quote_and_backslash_text_round_trip_via_field_escaping() {
        for s in [
            "a\tb",
            "\"quoted\"",
            "trailing\t",
            "a\\b",
            "C:\\path",
            "end\\",
        ] {
            roundtrip_text(s);
        }
    }

    #[test]
    fn empty_text_round_trips() {
        roundtrip_text("");
    }

    #[test]
    fn newline_text_round_trips_as_a_multi_line_cell() {
        roundtrip_text("line1\nline2");
        assert_eq!(text_field("line1\nline2"), "line1\nline2".to_string());
        assert_eq!(encode_field(&text_field("line1\nline2")), "line1\\nline2");
    }

    #[test]
    fn numbers_are_lossless() {
        for n in [0.0, 30.0, -3.5, 45306.0, 1e20, 1e-9, 0.1 + 0.2] {
            let f = num_field(n);
            assert_eq!(
                lex_literal(&f),
                (Value::Number(n), None),
                "{n} spelled {f:?}"
            );
        }
    }

    /// Every test cell is unstyled: what a style becomes on disk is the write leg's own contract.
    fn unstyled(values: Vec<SourceValue>) -> Vec<SourceCell> {
        values.into_iter().map(SourceCell::unstyled).collect()
    }

    fn roundtrip_cell(cell: &SourceValue) -> Cell {
        let field = encode_field(&cell_field(cell, &Resolution::empty(), "S", 0).0);
        deserialize_tsv("A1", &field)
            .expect("a single typed field deserializes")
            .cells
            .remove(0)
    }

    #[test]
    fn a_formatted_number_literal_round_trips_value_and_format() {
        use fsa1_model::CurrencySymbol;
        let cases: Vec<(SourceValue, f64, Format)> = vec![
            (
                SourceValue::Formatted {
                    value: 12.5,
                    format: Format::Fixed { decimals: 2 },
                },
                12.5,
                Format::Fixed { decimals: 2 },
            ),
            (
                SourceValue::Formatted {
                    value: 1234.0,
                    format: Format::Grouped { decimals: 2 },
                },
                1234.0,
                Format::Grouped { decimals: 2 },
            ),
            (
                SourceValue::Formatted {
                    value: 0.125,
                    format: Format::Percent { decimals: 2 },
                },
                0.125,
                Format::Percent { decimals: 2 },
            ),
            (
                SourceValue::Formatted {
                    value: 1234.0,
                    format: Format::Currency {
                        symbol: CurrencySymbol::Dollar,
                        grouping: true,
                        decimals: 2,
                    },
                },
                1234.0,
                Format::Currency {
                    symbol: CurrencySymbol::Dollar,
                    grouping: true,
                    decimals: 2,
                },
            ),
            (
                SourceValue::Formatted {
                    value: -1234.0,
                    format: Format::Currency {
                        symbol: CurrencySymbol::Dollar,
                        grouping: true,
                        decimals: 2,
                    },
                },
                -1234.0,
                Format::Currency {
                    symbol: CurrencySymbol::Dollar,
                    grouping: true,
                    decimals: 2,
                },
            ),
        ];
        for (cell, value, format) in cases {
            assert_eq!(
                roundtrip_cell(&cell),
                Cell::Value {
                    value: Value::Number(value),
                    format: Some(format),
                },
                "{cell:?} did not round-trip"
            );
        }
    }

    #[test]
    fn a_formatted_date_literal_round_trips_via_the_iso_marker() {
        use fsa1_model::{DatePattern, DateTimePattern, TimePattern};
        // 44331 = 2021-05-15; 0.5625 = 13:30:00; 44331.5625 = that datetime.
        for (value, format) in [
            (44331.0, Format::Date(DatePattern::Mdy)),
            (0.5625, Format::Time(TimePattern::Hms)),
            (44331.5625, Format::DateTime(DateTimePattern::MdyHm)),
        ] {
            assert_eq!(
                roundtrip_cell(&SourceValue::Formatted { value, format }),
                Cell::Value {
                    value: Value::Number(value),
                    format: Some(format),
                },
                "{format:?} did not round-trip"
            );
        }
    }

    #[test]
    fn a_formatted_formula_round_trips_body_and_marker() {
        use fsa1_model::CurrencySymbol;
        let cell = SourceValue::Formula {
            raw: "B1*2".to_string(),
            format: Some(Format::Percent { decimals: 2 }),
        };
        assert!(
            matches!(
                roundtrip_cell(&cell),
                Cell::Formula { src, format: Some(Format::Percent { decimals: 2 }), .. } if src == "=B1*2"
            ),
            "the deserializer peels the `~<code>` marker back off the body"
        );

        let cur = SourceValue::Formula {
            raw: "SUM(B1:B5)".to_string(),
            format: Some(Format::Currency {
                symbol: CurrencySymbol::Dollar,
                grouping: true,
                decimals: 2,
            }),
        };
        assert!(matches!(
            roundtrip_cell(&cur),
            Cell::Formula { src, format: Some(Format::Currency { symbol: CurrencySymbol::Dollar, grouping: true, decimals: 2 }), .. }
                if src == "=SUM(B1:B5)"
        ));

        let plain = SourceValue::Formula {
            raw: "B1*2".to_string(),
            format: None,
        };
        assert!(
            matches!(
                roundtrip_cell(&plain),
                Cell::Formula { src, format: None, .. } if src == "=B1*2"
            ),
            "a format-less formula carries no marker"
        );
    }

    #[test]
    fn display_exact_and_effective_format_pin_the_literal_boundary() {
        use fsa1_model::CurrencySymbol;
        assert!(is_display_exact(12.5, Format::Fixed { decimals: 2 }));
        assert!(!is_display_exact(1234.5678, Format::Fixed { decimals: 2 }));
        assert!(is_display_exact(0.125, Format::Percent { decimals: 2 }));
        assert!(!is_display_exact(0.12345, Format::Percent { decimals: 2 }));
        assert!(is_display_exact(5.0, Format::Fixed { decimals: 0 }));
        assert!(!is_display_exact(5.5, Format::Fixed { decimals: 0 }));

        let acct = Format::Accounting {
            symbol: CurrencySymbol::Dollar,
            decimals: 2,
        };
        assert_eq!(
            effective_literal_format(acct, 1234.0),
            Some(Format::Currency {
                symbol: CurrencySymbol::Dollar,
                grouping: true,
                decimals: 2,
            })
        );
        assert_eq!(
            effective_literal_format(acct, -1234.0),
            None,
            "a negative accounting value has no displayed-form spelling"
        );
    }

    #[test]
    fn errors_and_bools_spell_their_literals() {
        assert_eq!(error_literal(ErrKind::Div0), "#DIV/0!");
        assert_eq!(error_literal(ErrKind::Na), "#N/A");
        assert_eq!(
            cell_field(&SourceValue::Bool(true), &Resolution::empty(), "S", 0).0,
            "TRUE"
        );
    }

    /// A 1x1 block takes its bare address: `A1:A1` is refused as a degenerate range.
    #[test]
    fn a_single_cell_sheet_is_one_a1_file_and_an_empty_sheet_is_none() {
        let sheet = SheetSource {
            name: "S".to_string(),
            rows: 1,
            cols: 1,
            cells: unstyled(vec![SourceValue::Number(7.0)]),
            ..Default::default()
        };
        let written = files(&sheet, &mut Vec::new());
        assert_eq!(written, vec![("A1".to_string(), "7".to_string())]);
        accepted(&written);

        let empty = SheetSource {
            name: "S".to_string(),
            rows: 3,
            cols: 3,
            cells: unstyled(vec![SourceValue::Blank; 9]),
            ..Default::default()
        };
        assert!(
            files(&empty, &mut Vec::new()).is_empty(),
            "a sheet with no occupancy writes no file at all"
        );
    }

    /// The block model, not the cell model: one file over the occupancy's rectangle, its interior
    /// blank carried as an empty FIELD rather than as an absent file.
    #[test]
    fn a_sheets_occupancy_is_one_file_per_block_with_blanks_as_empty_fields() {
        let sheet = SheetSource {
            name: "S".to_string(),
            rows: 2,
            cols: 2,
            cells: unstyled(vec![
                SourceValue::Number(1.0),
                SourceValue::Blank,
                SourceValue::Formula {
                    raw: "of:=[.A1]+1".to_string(),
                    format: None,
                },
                SourceValue::Text("x".to_string()),
            ]),
            ..Default::default()
        };
        let written = files(&sheet, &mut Vec::new());
        assert_eq!(
            written,
            vec![("A1:B2".to_string(), "1\t\n=A1+1\tx".to_string())],
            "one file filling the rectangle exactly; blank B1 is an empty field"
        );
        accepted(&written);
    }

    /// A stray cell costs its own file, never a grid stretched to reach it.
    #[test]
    fn a_far_stray_cell_earns_its_own_file_rather_than_a_50000_row_grid() {
        let mut cells = vec![SourceValue::Blank; 50_000 * 26];
        cells[0] = SourceValue::Number(1.0);
        cells[1] = SourceValue::Number(2.0);
        cells[49_999 * 26 + 25] = SourceValue::Number(3.0);
        let sheet = SheetSource {
            name: "S".to_string(),
            rows: 50_000,
            cols: 26,
            cells: unstyled(cells),
            ..Default::default()
        };
        let written = files(&sheet, &mut Vec::new());
        assert_eq!(
            written,
            vec![
                ("A1:B1".to_string(), "1\t2".to_string()),
                ("Z50000".to_string(), "3".to_string()),
            ],
        );
        let fields: usize = written
            .iter()
            .map(|(_, content)| content.split(['\t', '\n']).count())
            .sum();
        assert_eq!(fields, 3, "three TSV fields in total");
        accepted(&written);
    }

    #[test]
    fn an_untranslatable_formula_is_preserved_verbatim_in_the_block_that_holds_it() {
        let sheet = SheetSource {
            name: "S".to_string(),
            rows: 1,
            cols: 1,
            cells: unstyled(vec![SourceValue::Formula {
                raw: "of:=[Sheet1.A1:Sheet2.B2]".to_string(),
                format: None,
            }]),
            ..Default::default()
        };
        let mut warnings = Vec::new();
        let written = files(&sheet, &mut warnings);
        assert_eq!(
            written,
            vec![("A1".to_string(), "=[Sheet1.A1:Sheet2.B2]".to_string())],
            "the import succeeds; the loader flags the cell at load"
        );
        assert_eq!(warnings.len(), 1);
        let UnpackWarning::FormulaKeptVerbatim {
            sheet,
            cell,
            source,
            reason,
        } = &warnings[0]
        else {
            panic!("expected a FormulaKeptVerbatim: {:?}", warnings[0]);
        };
        assert_eq!(sheet, "S");
        assert_eq!(cell, "A1");
        assert!(reason.contains("3-D range"), "{reason}");
        assert_eq!(
            source, "[Sheet1.A1:Sheet2.B2]",
            "the source is lead-stripped"
        );
        assert_eq!(
            format!("={source}"),
            written[0].1,
            "report source must match disk"
        );
    }

    #[test]
    fn a_resolvable_formula_pushes_no_verbatim_warning() {
        let sheet = SheetSource {
            name: "S".to_string(),
            rows: 1,
            cols: 1,
            cells: unstyled(vec![SourceValue::Formula {
                raw: "of:=[.A1]+1".to_string(),
                format: None,
            }]),
            ..Default::default()
        };
        let mut warnings = Vec::new();
        files(&sheet, &mut warnings);
        assert!(warnings.is_empty(), "a resolvable formula loses nothing");
    }

    #[test]
    fn a_cell_with_a_newline_tab_or_backslash_is_written_escaped_and_round_trips() {
        let sheet = SheetSource {
            name: "S".to_string(),
            rows: 1,
            cols: 3,
            cells: unstyled(vec![
                SourceValue::Text("line1\nline2".to_string()),
                SourceValue::Text("a\tb".to_string()),
                SourceValue::Text("C:\\dir".to_string()),
            ]),
            ..Default::default()
        };
        let written = files(&sheet, &mut Vec::new());
        assert_eq!(
            written,
            vec![(
                "A1:C1".to_string(),
                "line1\\nline2\ta\\tb\tC:\\\\dir".to_string()
            )]
        );
        let grid = deserialize_tsv("A1:C1", &written[0].1).expect("loads");
        for (want, cell) in ["line1\nline2", "a\tb", "C:\\dir"]
            .iter()
            .zip(grid.cells.iter())
        {
            assert_eq!(
                cell,
                &Cell::Value {
                    value: Value::Text(want.to_string()),
                    format: None
                }
            );
        }
        accepted(&written);
    }

    /// The workbook's Normal style: what a cell wearing no style of its own already is.
    fn normal() -> XlsxFont {
        XlsxFont {
            name: Some("Calibri".to_string()),
            size: Some(11.0),
            ..Default::default()
        }
    }

    fn face(name: &str, size: f64) -> XlsxStyle {
        XlsxStyle {
            font: XlsxFont {
                name: Some(name.to_string()),
                size: Some(size),
                ..Default::default()
            },
            ..Default::default()
        }
    }

    /// Row-major `(value, style index)` pairs over a `rows` x `cols` sheet, against `looks` as the
    /// workbook's whole style table.
    fn styled(
        rows: u32,
        cols: u32,
        cells: Vec<(SourceValue, Option<u32>)>,
        looks: Vec<XlsxStyle>,
    ) -> SheetSource {
        assert_eq!(cells.len(), (rows * cols) as usize, "the grid must be full");
        SheetSource {
            name: "S".to_string(),
            rows,
            cols,
            cells: cells
                .into_iter()
                .map(|(value, style)| SourceCell { value, style })
                .collect(),
            styles: StyleTable::of(looks, normal()),
            ..Default::default()
        }
    }

    fn text(s: &str) -> SourceValue {
        SourceValue::Text(s.to_string())
    }

    /// The whole sheet as ONE block, and one block per occupied cell: the two extreme partitions the
    /// seam admits.
    fn whole_sheet(sheet: &SheetSource) -> Vec<Block> {
        vec![Block {
            col: 1,
            row: 1,
            cols: sheet.cols,
            rows: sheet.rows,
        }]
    }

    fn per_cell(sheet: &SheetSource) -> Vec<Block> {
        crate::occupancy(sheet)
            .into_iter()
            .map(|(col, row, _)| Block {
                col,
                row,
                cols: 1,
                rows: 1,
            })
            .collect()
    }

    /// What the WRITTEN files say, read back through the real loader: the style in force at each
    /// sheet coordinate, and the size each sheet axis is declared at. Sizes come out separately
    /// because which file states one is the partition's business, and the cell it lands on with it.
    #[allow(clippy::type_complexity)]
    fn readback(
        files: &[(String, String)],
    ) -> (
        BTreeMap<(u32, u32), CellStyle>,
        BTreeMap<u32, Chars>,
        BTreeMap<u32, Points>,
    ) {
        let mut styles: BTreeMap<(u32, u32), CellStyle> = BTreeMap::new();
        let mut widths: BTreeMap<u32, Chars> = BTreeMap::new();
        let mut heights: BTreeMap<u32, Points> = BTreeMap::new();
        for (name, content) in files {
            let parsed = fsa1_model::parse_file(name, content)
                .unwrap_or_else(|d| panic!("{name} must load: {d:?}"));
            let region = parsed.region;
            for row in region.min_row..=region.max_row {
                for col in region.min_col..=region.max_col {
                    // A file with no block wears the same appearance as one whose block declares nothing here.
                    let mut style = match &parsed.presentation {
                        Some(p) => fsa1_model::style::resolve(
                            p,
                            row - region.min_row + 1,
                            col - region.min_col + 1,
                        ),
                        None => CellStyle::default(),
                    };
                    // A size resolves at every cell of its axis, so it is compared as the axis's own fact.
                    (style.width, style.height) = (None, None);
                    styles.insert((col, row), style);
                }
            }
            let Some(presentation) = &parsed.presentation else {
                continue;
            };
            for run in fsa1_model::declared_widths(region, presentation) {
                for axis in run.start..=run.end {
                    widths.insert(axis, run.size);
                }
            }
            for run in fsa1_model::declared_heights(region, presentation) {
                for axis in run.start..=run.end {
                    heights.insert(axis, run.size);
                }
            }
        }
        (styles, widths, heights)
    }

    /// The seam the whole design turns on: everything downstream of a `Decomposition` must produce
    /// the same appearance under ANY disjoint covering partition, so the policy can change without
    /// the encoder moving.
    #[test]
    fn the_same_sheet_reads_back_alike_under_a_whole_sheet_and_a_per_cell_partition() {
        let mut sheet = styled(
            3,
            3,
            vec![
                (text("Region"), Some(1)),
                (text("Q1"), Some(1)),
                (text("Q2"), Some(1)),
                (text("North"), None),
                (SourceValue::Number(10.0), Some(2)),
                (SourceValue::Number(20.0), Some(2)),
                (text("South"), None),
                (SourceValue::Number(30.0), Some(2)),
                (SourceValue::Number(40.0), Some(2)),
            ],
            vec![
                XlsxStyle::default(),
                XlsxStyle {
                    font: XlsxFont {
                        bold: true,
                        ..normal()
                    },
                    horizontal: Some(HorizontalAlign::Center),
                    ..Default::default()
                },
                XlsxStyle {
                    horizontal: Some(HorizontalAlign::Right),
                    ..face("Times New Roman", 14.0)
                },
            ],
        );
        sheet.col_widths.insert(0, 14.5);
        sheet.row_heights.insert(0, 22.5);

        let mut seen = Vec::new();
        let mut named = vec![
            ("whole-sheet", whole_sheet(&sheet)),
            ("per-cell", per_cell(&sheet)),
        ];
        named.extend(Decomposition::ALL.map(|d| (d.name(), d.blocks(&crate::occupancy(&sheet)))));
        for (_, blocks) in &named {
            let mut warnings = Vec::new();
            let written = sheet_files(&sheet, blocks, &Resolution::empty(), &mut warnings);
            assert!(
                warnings.is_empty(),
                "{blocks:?} lost something: {warnings:?}"
            );
            accepted(&written);
            seen.push(readback(&written));
        }
        assert_eq!(seen[0], seen[1], "whole-sheet vs per-cell");
        for (i, (name, _)) in named.iter().enumerate().skip(2) {
            assert_eq!(seen[0], seen[i], "whole-sheet vs the {name} partition");
        }
        let (styles, widths, heights) = &seen[0];
        assert_eq!(widths[&0], Chars(14.5), "column A, verbatim");
        assert_eq!(heights[&0], Points(22.5), "row 1, verbatim");
        assert_eq!(styles[&(1, 0)].font_weight, Some(FontWeight::Bold));
        assert_eq!(styles[&(1, 0)].text_align, Some(TextAlign::Center));
        assert_eq!(
            styles[&(1, 1)].font_family.as_deref(),
            Some("Times New Roman")
        );
        assert_eq!(styles[&(1, 1)].font_size, Some(Points(14.0)));
        assert_eq!(styles[&(0, 1)].font_family.as_deref(), None, "A2 is Normal");
    }

    /// The whole point of the modal rule: one shared look costs one declaration, not one per cell.
    #[test]
    fn a_sheet_whose_every_cell_shares_one_font_spells_it_once() {
        let cells = (0..9)
            .map(|i| (SourceValue::Number(f64::from(i)), Some(0)))
            .collect();
        let sheet = styled(3, 3, cells, vec![face("Times New Roman", 14.0)]);
        let written = files(&sheet, &mut Vec::new());
        assert_eq!(
            written,
            vec![(
                "A1:C3".to_string(),
                "0\t1\t2\n3\t4\t5\n6\t7\t8\n@scope {\n  \
                 td { font-family: Times New Roman; font-size: 14pt }\n}"
                    .to_string()
            )],
            "one td rule, one declaration per property"
        );
        accepted(&written);
    }

    /// A title over a table: one block spanning the gap, and column A's width stated by the file
    /// whose own range contains column A.
    #[test]
    fn a_title_over_a_table_is_one_file_carrying_its_columns_width() {
        let mut cells = vec![(SourceValue::Blank, None); 4 * 20];
        cells[0] = (text("Quarterly report"), None);
        cells[4] = (text("prepared by finance"), None);
        for row in 3..20 {
            for col in 0..4 {
                cells[row * 4 + col] = (SourceValue::Number((row * 4 + col) as f64), None);
            }
        }
        let mut sheet = styled(20, 4, cells, Vec::new());
        sheet.col_widths.insert(0, 14.5);

        let written = files(&sheet, &mut Vec::new());
        assert_eq!(written.len(), 1, "{written:?}");
        assert_eq!(written[0].0, "A1:D20");
        assert!(
            written[0]
                .1
                .ends_with("@scope {\n  td:first-child { width: 14.5ch }\n}"),
            "{}",
            written[0].1
        );
        accepted(&written);
    }

    /// A one-column file can spell no column selector at all, so `td` is the only way it sizes its
    /// own column — and the same for a one-row file's height.
    #[test]
    fn a_single_axis_block_sizes_itself_through_a_bare_td() {
        let mut sheet = styled(
            2,
            1,
            vec![(SourceValue::Number(1.0), None), (text("x"), None)],
            Vec::new(),
        );
        sheet.col_widths.insert(0, 9.0);
        sheet.row_heights.insert(1, 20.0);
        let written = files(&sheet, &mut Vec::new());
        assert_eq!(
            written,
            vec![(
                "A1:A2".to_string(),
                "1\nx\n@scope {\n  td { width: 9ch }\n  tr:last-child td { height: 20pt }\n}"
                    .to_string()
            )],
        );
        accepted(&written);
    }

    /// Reachable only for an axis lying entirely outside the sheet's occupancy: no file's own range
    /// contains it, and every eligible file is the empty set.
    #[test]
    fn an_axis_no_file_contains_is_dropped_and_named() {
        let mut sheet = styled(1, 1, vec![(SourceValue::Number(1.0), None)], Vec::new());
        sheet.col_widths.insert(4, 20.0);
        sheet.row_heights.insert(6, 30.0);
        let mut warnings = Vec::new();
        let written = files(&sheet, &mut warnings);
        assert_eq!(written, vec![("A1".to_string(), "1".to_string())]);
        assert_eq!(
            warnings.iter().map(ToString::to_string).collect::<Vec<_>>(),
            vec![
                "column width for E on sheet S dropped: no range file covers column E".to_string(),
                "row height for 7 on sheet S dropped: no range file covers row 7".to_string(),
            ],
        );
    }

    /// The cascade over one block, property by property: a header row, a column of its own and a
    /// total row, each surviving with every property it was written with.
    #[test]
    fn a_header_row_a_formatted_column_and_a_total_row_all_survive() {
        let header = XlsxStyle {
            font: XlsxFont {
                bold: true,
                color: Some(Rgb {
                    r: 0xff,
                    g: 0xff,
                    b: 0xff,
                }),
                ..normal()
            },
            fill: XlsxFill {
                pattern: FillPattern::Solid,
                fg: Some(Rgb {
                    r: 0x3f,
                    g: 0x04,
                    b: 0x21,
                }),
                bg: None,
            },
            horizontal: Some(HorizontalAlign::Center),
            wrap_text: true,
            ..Default::default()
        };
        let money = XlsxStyle {
            font: XlsxFont {
                italic: true,
                ..normal()
            },
            horizontal: Some(HorizontalAlign::Right),
            ..Default::default()
        };
        let total = XlsxStyle {
            font: XlsxFont {
                bold: true,
                ..normal()
            },
            border_top: Some(XlsxBorder {
                style: BorderStyle::Double,
                color: Some(Rgb {
                    r: 0x3f,
                    g: 0x04,
                    b: 0x21,
                }),
            }),
            ..Default::default()
        };
        let mut cells = Vec::new();
        for row in 0..4u32 {
            for col in 0..3u32 {
                let look = match (row, col) {
                    (0, _) => Some(0),
                    (3, _) => Some(2),
                    (_, 2) => Some(1),
                    _ => None,
                };
                cells.push((SourceValue::Number(f64::from(row * 3 + col)), look));
            }
        }
        let sheet = styled(4, 3, cells, vec![header, money, total]);
        let written = files(&sheet, &mut Vec::new());
        accepted(&written);
        let (styles, _, _) = readback(&written);

        let plum = Rgb {
            r: 0x3f,
            g: 0x04,
            b: 0x21,
        };
        for col in 0..3 {
            let head = &styles[&(col, 0)];
            assert_eq!(head.font_weight, Some(FontWeight::Bold), "header {col}");
            assert_eq!(head.background_color, Some(plum), "header {col}");
            assert_eq!(head.text_align, Some(TextAlign::Center), "header {col}");
            assert_eq!(head.white_space, Some(WhiteSpace::Normal), "header {col}");
            assert_eq!(
                head.color,
                Some(Rgb {
                    r: 0xff,
                    g: 0xff,
                    b: 0xff
                }),
                "header {col}"
            );
            let foot = &styles[&(col, 3)];
            assert_eq!(foot.font_weight, Some(FontWeight::Bold), "total {col}");
            assert_eq!(
                foot.border_top,
                Some(Border {
                    line: BorderLine::ThickDouble,
                    color: plum
                }),
                "total {col}"
            );
        }
        for row in 1..3 {
            let money = &styles[&(2, row)];
            assert_eq!(money.font_style, Some(FontStyle::Italic), "money {row}");
            assert_eq!(money.text_align, Some(TextAlign::Right), "money {row}");
            // Undeclared, never `font-style: normal`: what an author never asked for is not written.
            let plain = &styles[&(0, row)];
            assert_eq!(plain.font_style, None, "plain {row}");
            assert_eq!(plain.text_align, None, "plain {row} states no alignment");
            assert_eq!(plain.background_color, None, "plain {row}");
        }
    }

    /// The document's own default text colour, which the corpus restates on 1,233,720 of its
    /// 1,234,236 theme-coloured cells; a workbook wearing it throughout says nothing about colour.
    #[test]
    fn the_default_text_colour_earns_no_declaration() {
        let black = Rgb { r: 0, g: 0, b: 0 };
        let plain = XlsxStyle {
            font: XlsxFont {
                color: Some(black),
                ..normal()
            },
            ..Default::default()
        };
        let cells = (0..4)
            .map(|i| (SourceValue::Number(f64::from(i)), Some(0)))
            .collect();
        let sheet = styled(2, 2, cells, vec![plain]);
        let written = files(&sheet, &mut Vec::new());
        assert_eq!(
            written,
            vec![("A1:B2".to_string(), "0\t1\n2\t3".to_string())],
            "a theme=1 tint=0 workbook writes no block at all"
        );
    }

    #[test]
    fn a_fully_unstyled_workbook_loses_nothing() {
        let cells = (0..6)
            .map(|i| (SourceValue::Number(f64::from(i)), None))
            .collect();
        let sheet = styled(2, 3, cells, Vec::new());
        let mut warnings = Vec::new();
        let written = files(&sheet, &mut warnings);
        assert!(warnings.is_empty(), "{warnings:?}");
        assert_eq!(
            written,
            vec![("A1:C2".to_string(), "0\t1\t2\n3\t4\t5".to_string())],
            "no style, no block"
        );
    }
}
