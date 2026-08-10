// Concern: a tab's sidecars as scopes: their shape, their bytes, whether their NAMED roots may coexist | Non-concern: a coordinate's style, a rule's grammar | IO: (sidecars, roots) -> scopes, diags

use crate::declaration::Declaration;
use crate::diagnostic::{Code, Diagnostic, Loc};
use crate::overlap::Rect;
use crate::presentation::{LocatedRule, Presentation, Target};

/// One sidecar: its root, its `<tab>/<name>`, its text VERBATIM as read, the rules parsed from that
/// text, and the declarations no typed rule took. The text is retained because a carrier that
/// re-spells presentation copies the authored bytes rather than re-deriving them from the typed
/// rules; `uncarried` is retained because the read that found them is the read that parsed the rules.
#[derive(Clone, Debug)]
pub(crate) struct Sidecar {
    pub root: Rect,
    pub file: String,
    pub text: String,
    pub presentation: Presentation,
    pub uncarried: Vec<Diagnostic>,
}

/// One scoping root and the bytes stated over it. `file` is `<tab>/<name>` exactly as a diagnostic
/// spells it, so a carrier locates its own refusals without a second walk of the tree, and
/// `tab_layer` separates the layer beneath every block from a block. `uncarried` is what a carrier
/// with a CLOSED property list refuses: located already, from the read that built this scope.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SidecarScope<'a> {
    pub root: Rect,
    pub file: String,
    pub text: &'a str,
    pub tab_layer: bool,
    pub uncarried: &'a [Diagnostic],
}

/// Every sidecar a tab holds, in cascade order — the tab layer first, then the blocks widest root
/// first — each handing out the BYTES its author wrote. A figure's placement sidecar is no scope and
/// appears in neither half.
pub(crate) fn scopes<'a>(
    layer: Option<&'a Sidecar>,
    blocks: &'a [Sidecar],
) -> Vec<SidecarScope<'a>> {
    layer
        .into_iter()
        .map(|s| scope_of(s, true))
        .chain(blocks.iter().map(|s| scope_of(s, false)))
        .collect()
}

fn scope_of(sidecar: &Sidecar, tab_layer: bool) -> SidecarScope<'_> {
    SidecarScope {
        root: sidecar.root,
        file: sidecar.file.clone(),
        text: &sidecar.text,
        tab_layer,
        uncarried: &sidecar.uncarried,
    }
}

pub(crate) fn area(root: Rect) -> u64 {
    u64::from(root.max_col - root.min_col + 1) * u64::from(root.max_row - root.min_row + 1)
}

/// Decision 3: the roots of one tab are DISJOINT, or nest with the inner one a SINGLE cell. Any
/// other overlap has no one subtree of the outer to be applied over — its cells are parts of several
/// of the outer's rows — so it is refused on the LATER of the pair, which the cascade order names.
/// Every root the tab NAMES is graded, read or not; an identical pair is [`Code::DuplicateSidecarRoot`]'s.
pub(crate) fn check_scope_nesting(roots: &[(String, Rect)], diags: &mut Vec<Diagnostic>) {
    for (index, (file, root)) in roots.iter().enumerate() {
        for (_, outer) in &roots[..index] {
            let (outer, root) = (*outer, *root);
            if outer == root || nests(root, outer) || root.intersect(&outer).is_none() {
                continue;
            }
            diags.push(Diagnostic::new(
                Code::SidecarScopeCrossing,
                Loc::file(file),
                format!(
                    "{} and {} claim overlapping cells and neither is one cell inside the other; a \
                     scope is one region of the sheet -- nest a single cell, or split one",
                    outer.label(),
                    root.label()
                ),
            ));
        }
    }
}

/// One root wholly inside the other AND that inner root a single cell.
fn nests(a: Rect, b: Rect) -> bool {
    let inner = match a.intersect(&b) {
        Some(shared) if shared == a => a,
        Some(shared) if shared == b => b,
        _ => return false,
    };
    area(inner) == 1
}

/// Decision 4: a tab layer's root is the whole content where a block's is a region of it, so an
/// index in the layer reaches into every block. Exempt is a rule DECLARING only a size, reaching no
/// cell either way — the encoder's own layer. Never a vacuous one: a rule the model carries nothing
/// from reaches every block on the author's bytes, so it answers for its index like any other.
pub(crate) fn check_tab_layer(file: &str, layer: &[LocatedRule], diags: &mut Vec<Diagnostic>) {
    for located in layer {
        let declarations = &located.rule.declarations;
        let sized_only = !declarations.is_empty()
            && declarations
                .iter()
                .all(|d| matches!(d, Declaration::Width(_) | Declaration::Height(_)));
        if located.rule.target == Target::All || sized_only {
            continue;
        }
        diags.push(Diagnostic::new(
            Code::TabLayerIndex,
            Loc::body(file, located.line, located.col),
            "this tab holds a rooted sidecar, so an index counted in the tab's own content reaches \
             into every block; give the region its own <range>.css, or drop the index"
                .to_string(),
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::overlay::Overlay;
    use crate::workbook::Workbook;

    fn refusals(files: &[(&str, &str)]) -> Vec<Diagnostic> {
        Overlay::from_tabs(&[("Sheet1", files)]).expect_err("these sidecars must refuse")
    }

    /// A carrier that re-spells presentation copies what the author WROTE, so the bytes reach it
    /// indent, spacing and trailing newline intact rather than re-derived from the typed rules.
    #[test]
    fn a_scope_hands_out_its_sidecars_bytes_verbatim() {
        let text = "  fsa1-cell { background-color: #eef6ff }\n";
        let files: &[(&str, &str)] = &[("A1:B4", "1\t2\n3\t4\n5\t6\n7\t8"), ("A1:B4.css", text)];
        let wb = Workbook::from_tabs(&[("Sheet1", files)]).expect("the tree loads");
        let overlay = Overlay::from_tabs(&[("Sheet1", files)]).expect("its sidecars load");
        let scopes = overlay.scopes(&wb, 0);
        assert_eq!(scopes.len(), 1, "{scopes:?}");
        assert_eq!(scopes[0].text, text);
        assert_eq!(scopes[0].file, "Sheet1/A1:B4.css");
        assert!(!scopes[0].tab_layer);
    }

    /// Two roots of equal area sharing a cell CROSS: B1 belongs to a row of each and to no single
    /// subtree, so no cascade order settles them and neither ordering of the tree loads.
    #[test]
    fn two_overlapping_roots_of_equal_area_are_refused_as_crossing() {
        let first = ("A1:B1.css", "  fsa1-cell { color: #ff0000 }\n");
        let last = ("B1:C1.css", "  fsa1-cell { color: #0000ff }\n");
        for entries in [[first, last], [last, first]] {
            let mut files = vec![("A1:C1", "1\t2\t3")];
            files.extend(entries);
            let diags = refusals(&files);
            assert!(
                diags.iter().any(|d| d.code == Code::SidecarScopeCrossing),
                "{diags:?}"
            );
            assert!(
                diags
                    .iter()
                    .any(|d| d.loc.to_string() == "Sheet1/B1:C1.css"),
                "located on the later of the pair, whatever order the tree lists them: {diags:?}"
            );
        }
    }

    /// The tab layer's indices count in the tab's CONTENT, which no block need cover: row 1 here is
    /// row 1 of the tab and the one block starts at row 9. So the refusal is its own code, and its
    /// remediation may not name a block that does not exist.
    #[test]
    fn an_index_in_the_tab_layer_over_blocks_is_its_own_refusal() {
        let diags = refusals(&[
            ("A1:A10", "1\n2\n3\n4\n5\n6\n7\n8\n9\n10"),
            ("A9:A10.css", "  fsa1-cell { color: #3f0421 }\n"),
            (
                ".css",
                "  fsa1-row:first-child fsa1-cell { font-weight: bold }\n",
            ),
        ]);
        assert_eq!(
            diags.iter().map(|d| d.code).collect::<Vec<_>>(),
            vec![Code::TabLayerIndex],
            "{diags:?}"
        );
        assert_eq!(diags[0].loc.to_string(), "Sheet1/.css:1:3");
    }

    /// A size reaches no cell, so the encoder's own layer — an axis selector sizing an axis and
    /// declaring nothing else — still loads over a block.
    #[test]
    fn a_tab_layer_sizing_an_axis_still_loads_over_a_block() {
        let files: &[(&str, &str)] = &[
            ("A1:C2", "1\t2\t3\n4\t5\t6"),
            ("A1:C2.css", "  fsa1-cell { font-family: Arial }\n"),
            (".css", "  fsa1-cell:nth-child(2) { width: 13ch }\n"),
        ];
        Overlay::from_tabs(&[("Sheet1", files)]).expect("a size-only layer rule is no fault");
    }

    /// An index in the tab layer reaches into every block on the BYTES the author wrote, and the
    /// reach does not care what the typed model can read: the same two layers, differing only in
    /// whether the rule's declaration is carried, earn the one refusal on the one line.
    #[test]
    fn a_tab_layer_rule_the_model_carries_nothing_from_still_answers_for_its_index() {
        for layer in [
            "  fsa1-cell:first-child { color: #ffffff }\n",
            "  fsa1-cell:first-child { box-shadow: none }\n",
        ] {
            let diags = refusals(&[
                ("A1:B2", "1\t2\n3\t4"),
                ("A1:B2.css", "  fsa1-cell { color: #3f0421 }\n"),
                (".css", layer),
            ]);
            assert_eq!(
                diags.iter().map(|d| d.code).collect::<Vec<_>>(),
                vec![Code::TabLayerIndex],
                "{layer:?} -> {diags:?}"
            );
            assert_eq!(diags[0].loc.to_string(), "Sheet1/.css:1:3", "{layer:?}");
        }
    }
}
