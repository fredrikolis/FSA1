// Concern: which sheet axes a tab's files size, and where two disagree | Non-concern: reading a declaration, applying one | IO: (region, &Presentation) -> AxisRuns; (tab, files) -> Vec<Diagnostic>

use std::collections::HashSet;

use fsa1_ast::a1::format_column;

use crate::declaration::{Chars, Declaration, Points};
use crate::diagnostic::{Code, Diagnostic, Loc};
use crate::overlap::Rect;
use crate::presentation::{Presentation, Target};

/// A run of consecutive sheet columns, or of rows, inclusive on both ends and zero-based, carrying
/// one declared size.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AxisRun<T> {
    pub start: u32,
    pub end: u32,
    pub size: T,
}

/// One file's claim on the sheet's axes: the region it fills, and the block styling that region.
/// Crate-private with [`detect_geometry_conflicts`]: the first-wins in [`crate::Workbook`]'s axis
/// runs rests on the conflict census having run over EVERY file of the tab, at load.
#[derive(Clone, Copy, Debug)]
pub(crate) struct FileGeometry<'a> {
    pub name: &'a str,
    pub region: Rect,
    pub presentation: Option<&'a Presentation>,
}

/// The sheet columns the block sizes, ascending and disjoint: a file's column `k` is sheet column
/// `region.min_col + k - 1`, sized by the All -> Col half of the cascade [`crate::style::resolve`]
/// runs per cell — a `td` width sizes every column the region spans, a `td:nth-child(k)` one of
/// them. The row and cell halves carry no width, the parser refusing one there.
pub fn declared_widths(region: Rect, presentation: &Presentation) -> Vec<AxisRun<Chars>> {
    let mut base = None;
    let mut overrides = Vec::new();
    for rule in &presentation.rules {
        for declaration in &rule.declarations {
            match (rule.target, declaration) {
                (Target::All, Declaration::Width(w)) => base = Some(*w),
                (Target::Col(k), Declaration::Width(w)) => {
                    overrides.push((region.min_col + k - 1, *w));
                }
                _ => {}
            }
        }
    }
    runs(region.min_col, region.max_col, base, &overrides)
}

/// [`declared_widths`] on the other axis: a file's row `r` is sheet row `region.min_row + r - 1`.
pub fn declared_heights(region: Rect, presentation: &Presentation) -> Vec<AxisRun<Points>> {
    let mut base = None;
    let mut overrides = Vec::new();
    for rule in &presentation.rules {
        for declaration in &rule.declarations {
            match (rule.target, declaration) {
                (Target::All, Declaration::Height(h)) => base = Some(*h),
                (Target::Row(r), Declaration::Height(h)) => {
                    overrides.push((region.min_row + r - 1, *h));
                }
                _ => {}
            }
        }
    }
    runs(region.min_row, region.max_row, base, &overrides)
}

/// `overrides` are ascending and within `lo..=hi`: a parsed presentation's rules ascend by target,
/// and every index is one the parser bounded by the region's own extent.
fn runs<T: Copy>(lo: u32, hi: u32, base: Option<T>, overrides: &[(u32, T)]) -> Vec<AxisRun<T>> {
    let mut out = Vec::new();
    let mut at = lo;
    for (axis, size) in overrides {
        if let Some(base) = base
            && at < *axis
        {
            out.push(AxisRun {
                start: at,
                end: axis - 1,
                size: base,
            });
        }
        out.push(AxisRun {
            start: *axis,
            end: *axis,
            size: *size,
        });
        at = axis + 1;
    }
    if let Some(base) = base
        && at <= hi
    {
        out.push(AxisRun {
            start: at,
            end: hi,
            size: base,
        });
    }
    out
}

/// A file may size any axis its OWN range contains — not merely one it spans the used extent of — and
/// every file sizing a given sheet axis must size it alike. Two that agree pass silently; two that
/// disagree earn ONE refusal per pair, naming the first axis they part on.
pub(crate) fn detect_geometry_conflicts(tab: &str, files: &[FileGeometry<'_>]) -> Vec<Diagnostic> {
    let mut widths = Vec::new();
    let mut heights = Vec::new();
    for file in files {
        let Some(p) = file.presentation else { continue };
        widths.extend(
            declared_widths(file.region, p)
                .into_iter()
                .map(|run| (file.name, run)),
        );
        heights.extend(
            declared_heights(file.region, p)
                .into_iter()
                .map(|run| (file.name, run)),
        );
    }
    let mut out = disagreements(tab, "column", format_column, widths, Chars::spell);
    out.extend(disagreements(
        tab,
        "row",
        |row| (u64::from(row) + 1).to_string(),
        heights,
        Points::spell,
    ));
    out
}

/// A sweep in ascending start, so every run still in `active` covers the run being read and the two
/// need no interval test of their own. `active` holds one entry per (file, size) reaching the axis —
/// EVERY file, not one per distinct size, under which a file vanishes as soon as a second one agrees
/// with it and one of the pairs three disagreeing files form goes unnamed. `reported` caps a pair at one.
fn disagreements<T: Copy + PartialEq>(
    tab: &str,
    axis: &str,
    label: fn(u32) -> String,
    mut runs: Vec<(&str, AxisRun<T>)>,
    spell: fn(T) -> String,
) -> Vec<Diagnostic> {
    runs.sort_by_key(|(_, run)| run.start);
    let mut active: Vec<(&str, u32, T)> = Vec::new();
    let mut reported: HashSet<(&str, &str)> = HashSet::new();
    let mut out = Vec::new();
    for (name, run) in runs {
        active.retain(|(_, end, _)| *end >= run.start);
        for (other, _, size) in &active {
            let pair = if *other <= name {
                (*other, name)
            } else {
                (name, *other)
            };
            if *size != run.size && reported.insert(pair) {
                out.push(Diagnostic::new(
                    Code::GeometryConflict,
                    Loc::tab(tab),
                    format!(
                        "two files size sheet {axis} {} differently in tab {tab:?}\n    {other}: {}\n    {name}: {}\n    precedence: none -- reject. Make the two agree, or drop one declaration.",
                        label(run.start),
                        spell(*size),
                        spell(run.size),
                    ),
                ));
            }
        }
        match active
            .iter_mut()
            .find(|(other, _, size)| *other == name && *size == run.size)
        {
            Some(entry) if run.end > entry.1 => entry.1 = run.end,
            Some(_) => {}
            None => active.push((name, run.end, run.size)),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Workbook;
    use crate::parse_file;

    fn refusal(files: &[(&str, &str)]) -> Diagnostic {
        Workbook::from_tabs(&[("Sheet1", files)])
            .err()
            .unwrap_or_else(|| panic!("{files:?} should be refused"))
            .remove(0)
    }

    fn accepted(files: &[(&str, &str)]) {
        if let Err(d) = Workbook::from_tabs(&[("Sheet1", files)]) {
            panic!("{files:?} should load: {:?}", d[0]);
        }
    }

    fn widths(name: &str, content: &str) -> Vec<AxisRun<Chars>> {
        let f = parse_file(name, content).expect("loads");
        declared_widths(f.region, &f.presentation.expect("a presentation"))
    }

    /// Two files, neither spanning the other, sizing one sheet column between them.
    #[test]
    fn two_files_sizing_one_sheet_column_differently_are_refused() {
        let d = refusal(&[
            ("A1:A2", "1\n2\n@scope {\n  td { width: 10ch }\n}"),
            ("A3:A4", "3\n4\n@scope {\n  td { width: 12ch }\n}"),
        ]);
        assert_eq!(d.code, Code::GeometryConflict);
        assert_eq!(d.loc, Loc::tab("Sheet1"));
        assert!(d.message.contains("sheet column A"), "{}", d.message);
        assert!(d.message.contains("10ch"), "{}", d.message);
        assert!(d.message.contains("12ch"), "{}", d.message);
    }

    /// The verdict is reject whichever pairs are named, so this is about not making the author fix one
    /// file, re-run, and meet the next: two files agreeing at 10ch and a third at 12ch is TWO
    /// disagreements, and a census that remembered one file per distinct size could only ever show one.
    #[test]
    fn every_pair_disagreeing_on_one_sheet_column_is_named_in_one_pass() {
        let refusals = Workbook::from_tabs(&[(
            "Sheet1",
            &[
                ("A1:A2", "1\n2\n@scope {\n  td { width: 10ch }\n}"),
                ("A3:A4", "3\n4\n@scope {\n  td { width: 10ch }\n}"),
                ("A5:A6", "5\n6\n@scope {\n  td { width: 12ch }\n}"),
            ],
        )])
        .expect_err("three files, two disagreements");
        let named: Vec<&Diagnostic> = refusals
            .iter()
            .filter(|d| d.code == Code::GeometryConflict)
            .collect();
        assert_eq!(named.len(), 2, "{named:#?}");
        for other in ["A1:A2", "A3:A4"] {
            assert!(
                named
                    .iter()
                    .any(|d| d.message.contains(other) && d.message.contains("A5:A6")),
                "{other} vs A5:A6 was never named: {named:#?}"
            );
        }
    }

    #[test]
    fn two_files_agreeing_on_a_sheet_column_are_accepted() {
        accepted(&[
            ("A1:A2", "1\n2\n@scope {\n  td { width: 10ch }\n}"),
            ("A3:A4", "3\n4\n@scope {\n  td { width: 10ch }\n}"),
        ]);
    }

    #[test]
    fn a_file_sizes_only_the_axes_its_own_range_covers() {
        accepted(&[
            ("A1:A2", "1\n2\n@scope {\n  td { width: 10ch }\n}"),
            ("B1:B2", "1\n2\n@scope {\n  td { width: 12ch }\n}"),
        ]);
    }

    /// The offset: `C5:D9`'s last column is sheet column D, which `D1:D3` also holds. `td:last-child`
    /// is `td:nth-child(2)`'s canonical spelling in a region two columns wide.
    #[test]
    fn a_column_is_resolved_through_the_files_own_offset() {
        const OFFSET: (&str, &str) = (
            "C5:D9",
            "1\t2\n1\t2\n1\t2\n1\t2\n1\t2\n@scope {\n  td:last-child { width: 9ch }\n}",
        );
        let d = refusal(&[
            OFFSET,
            ("D1:D3", "1\n2\n3\n@scope {\n  td { width: 10ch }\n}"),
        ]);
        assert_eq!(d.code, Code::GeometryConflict);
        assert!(d.message.contains("sheet column D"), "{}", d.message);
        accepted(&[
            OFFSET,
            ("D1:D3", "1\n2\n3\n@scope {\n  td { width: 9ch }\n}"),
        ]);
        // C is C5:D9's own, sized by nothing, so a file over C disagrees with no one.
        accepted(&[
            OFFSET,
            ("C1:C3", "1\n2\n3\n@scope {\n  td { width: 4ch }\n}"),
        ]);
    }

    #[test]
    fn a_row_is_resolved_through_the_files_own_offset() {
        let d = refusal(&[
            (
                "A1:B2",
                "1\t2\n3\t4\n@scope {\n  tr:last-child td { height: 20pt }\n}",
            ),
            ("C2", "5\n@scope {\n  td { height: 15pt }\n}"),
        ]);
        assert_eq!(d.code, Code::GeometryConflict);
        assert!(d.message.contains("sheet row 2"), "{}", d.message);
        assert!(d.message.contains("20pt"), "{}", d.message);
    }

    /// A bare `td` sizes every column the region spans, and a column selector overrides that one.
    #[test]
    fn a_bare_td_covers_the_region_and_a_column_rule_carves_it() {
        assert_eq!(
            widths(
                "B2:D4",
                "1\t2\t3\n4\t5\t6\n7\t8\t9\n@scope {\n  td { width: 10ch }\n  td:nth-child(2) { width: 4ch }\n}",
            ),
            vec![
                AxisRun {
                    start: 1,
                    end: 1,
                    size: Chars(10.0)
                },
                AxisRun {
                    start: 2,
                    end: 2,
                    size: Chars(4.0)
                },
                AxisRun {
                    start: 3,
                    end: 3,
                    size: Chars(10.0)
                },
            ],
        );
        assert_eq!(
            widths(
                "B2:D4",
                "1\t2\t3\n4\t5\t6\n7\t8\t9\n@scope {\n  td:first-child { width: 4ch }\n}"
            ),
            vec![AxisRun {
                start: 1,
                end: 1,
                size: Chars(4.0)
            }],
        );
        assert!(
            widths(
                "B2:D4",
                "1\t2\t3\n4\t5\t6\n7\t8\t9\n@scope {\n  td { color: #3f0421 }\n}"
            )
            .is_empty()
        );
    }
}
