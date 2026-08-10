// Concern: assembles what an .xlsx will not carry into one located list | Non-concern: FINDING either half — fsa1-model fills scope.uncarried | IO: (Workbook, Overlay, figure losses) -> Diagnostics

use fsa1_model::{Code, Diagnostic, Loc, Overlay, Workbook};

use crate::charts::FigureNotDrawn;

/// Both halves arrive already found — the declarations on the read that built each [`fsa1_model::SidecarScope`],
/// the figures on the pass that drew the charts — so answering costs no second parse and no second
/// chart, under one code so a caller draws it as one table. A `None` overlay is presentation that
/// would not PARSE: it silences that half only, and the figures are answered either way.
pub fn losses(
    wb: &Workbook,
    overlay: Option<&Overlay>,
    not_drawn: &[FigureNotDrawn],
) -> Vec<Diagnostic> {
    let declared: Vec<Diagnostic> = overlay
        .into_iter()
        .flat_map(|overlay| {
            (0..wb.sheet_names().len() as u32)
                .flat_map(move |sheet| overlay.scopes(wb, sheet))
                .flat_map(|scope| scope.uncarried.to_vec())
        })
        .collect();
    let figures = not_drawn.iter().map(|loss| {
        Diagnostic::new(
            Code::XlsxNotCarried,
            Loc::file(&loss.figure),
            loss.why.clone(),
        )
    });
    declared.into_iter().chain(figures).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const SHEET1: &[(&str, &str)] = &[
        ("A1:B2", "a\tb\nc\td"),
        (".css", "  fsa1-cell { transition: color 1s }\n"),
        ("A1:B2.css", "  fsa1-cell { color: crimson }\n"),
    ];
    const SHEET2: &[(&str, &str)] = &[
        ("A1:B2", "e\tf\ng\th"),
        ("A1:B2.css", "  fsa1-cell { box-shadow: none }\n"),
    ];

    /// The list is read top to bottom by whoever must fix it, so its order is the order an author
    /// walks the tree: every tab's sidecars in cascade order, tab by tab, and the figures last.
    #[test]
    fn declarations_come_in_scope_order_sheet_by_sheet_and_figures_last() {
        let tabs: &[(&str, &[(&str, &str)])] = &[("Sheet1", SHEET1), ("Sheet2", SHEET2)];
        let wb = Workbook::from_tabs(tabs).expect("the tree loads");
        let overlay = Overlay::from_tabs(tabs).expect("its sidecars load");
        let not_drawn = [FigureNotDrawn {
            figure: "Sheet1/Chart1.json".to_string(),
            why: "a boxplot has no Excel chart".to_string(),
        }];

        let found = losses(&wb, Some(&overlay), &not_drawn);
        let located: Vec<String> = found.iter().map(|d| d.loc.to_string()).collect();
        assert_eq!(
            located,
            [
                "Sheet1/.css:1:15",
                "Sheet1/A1:B2.css:1:15",
                "Sheet2/A1:B2.css:1:15",
                "Sheet1/Chart1.json",
            ],
            "{found:?}"
        );
        assert!(
            found.iter().all(|d| d.code == Code::XlsxNotCarried),
            "{found:?}"
        );
        assert_eq!(found[3].message, not_drawn[0].why);
    }

    /// A workbook with no sidecar and every figure drawn has an EMPTY list, not a missing one: the
    /// caller reports "nothing was lost" off exactly this.
    #[test]
    fn a_workbook_that_loses_nothing_yields_no_findings() {
        let tabs: &[(&str, &[(&str, &str)])] = &[("Sheet1", &[("A1:B2", "a\tb\nc\td")])];
        let wb = Workbook::from_tabs(tabs).expect("the tree loads");
        let overlay = Overlay::from_tabs(tabs).expect("its sidecars load");
        assert!(losses(&wb, Some(&overlay), &[]).is_empty());
    }

    /// Presentation that would not parse costs the DECLARATION half and nothing else. Its own syntax
    /// fault is what the caller shows for it; the figures are a separate read and still answer here.
    #[test]
    fn presentation_that_did_not_parse_still_leaves_the_figures_answered() {
        let tabs: &[(&str, &[(&str, &str)])] = &[("Sheet1", SHEET1)];
        let wb = Workbook::from_tabs(tabs).expect("the tree loads");
        let not_drawn = [FigureNotDrawn {
            figure: "Sheet1/Chart1.json".to_string(),
            why: "a boxplot has no Excel chart".to_string(),
        }];

        let found = losses(&wb, None, &not_drawn);
        let located: Vec<String> = found.iter().map(|d| d.loc.to_string()).collect();
        assert_eq!(located, ["Sheet1/Chart1.json"], "{found:?}");
    }
}
