// Concern: settles which figures pack as native charts | Non-concern: a chart's bytes (fsa1-xlsx), spelling a figure (fsa1-ingest) | IO: (Workbook, Figures) -> charts, and a loss for each with none

use std::fmt;

use fsa1_model::{Figures, Workbook};
use fsa1_xlsx::Chart;

/// One figure that packs to no chart, and why — the write half's own named loss. A dropped figure is
/// reported rather than approximated: a picture of a chart would look like one while not updating when
/// a cell changes and not being editable, and an agent given a sentence can simplify its spec instead.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FigureNotDrawn {
    /// As it locates — `<tab>/<name>.vl.json` — so the loss anchors on the file an author edits.
    pub figure: String,
    pub why: String,
}

impl fmt::Display for FigureNotDrawn {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} is drawn by no Excel chart: {}",
            self.figure, self.why
        )
    }
}

/// Every figure's chart, in tab order, and one named loss per figure that has none. Representability
/// is decided by WRITING the chart and reading it back: survives → the chart ships, differs → the
/// figure is dropped and named. There is no second opinion about what is too fancy to draw.
pub(crate) fn charts(wb: &Workbook, figures: &Figures) -> (Vec<Chart>, Vec<FigureNotDrawn>) {
    let mut drawn = Vec::new();
    let mut losses = Vec::new();
    for (sheet, tab) in wb.sheet_names().iter().enumerate() {
        let sheet = sheet as u32;
        for figure in figures.in_tab(tab) {
            match chart_of(wb, sheet, figure) {
                Ok(chart) => drawn.push(chart),
                Err(why) => losses.push(FigureNotDrawn {
                    figure: figure.name.clone(),
                    why,
                }),
            }
        }
    }
    (drawn, losses)
}

fn chart_of(wb: &Workbook, sheet: u32, figure: &fsa1_model::Figure) -> Result<Chart, String> {
    let chart = fsa1_xlsx::chart_for(wb, sheet, figure)?;
    fsa1_ingest::chart_restates_figure(&fsa1_xlsx::chart_xml(&chart), wb, sheet, figure)?;
    Ok(chart)
}
