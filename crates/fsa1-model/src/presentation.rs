// Concern: a sidecar's rules — selectors, declarations, order | Non-concern: a declaration's meaning, applying a style, the filename | IO: (root, text) <-> Rules; text -> a selector + its declarations

use crate::declaration::{Declaration, parse_declaration, syntax};
use crate::diagnostic::{Code, Diagnostic, Loc};
use crate::overlap::Rect;
use fsa1_ast::Shape;

/// The set of cells a rule selects, region-relative. SIX variants, not the ten selector forms: a
/// LITERAL index's pseudo-class follows from the region's extent, so it is a spelling ([`spell`]),
/// where a PERIODIC one has only the one. The derived `Ord` IS the canonical WRITING order and NOT
/// CSS specificity, which ranks Col under Row and ties each periodic with the literal beside it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Target {
    All,
    RowEvery { a: u32, b: u32 },
    ColEvery { a: u32, b: u32 },
    Row(u32),
    Col(u32),
    Cell { row: u32, col: u32 },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Rule {
    pub target: Target,
    pub declarations: Vec<Declaration>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Presentation {
    pub rules: Vec<Rule>,
}

/// A sidecar holds rules and nothing else: its FILENAME is the root, and that root supplies the
/// `Shape` the selectors are region-relative to, so which index is `:last-child` — and whether an
/// axis carries a selector of its own at all — follows from the name.
pub fn parse_rules(file: &str, root: Rect, content: &str) -> Result<Presentation, Vec<Diagnostic>> {
    parse_rules_located(file, root, content).map(rules_of)
}

/// One rule and where in the sidecar it was written, in ONE value: a caller cannot pair a rule with
/// another rule's position, and a filtered sequence stays located rather than silently sliding.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LocatedRule {
    pub rule: Rule,
    pub line: u32,
    pub col: u32,
}

/// A sidecar's rules in written order, each carrying its own line and column.
pub type LocatedRules = Vec<LocatedRule>;

/// The rules alone, in written order.
pub fn rules_of(located: LocatedRules) -> Presentation {
    Presentation {
        rules: located.into_iter().map(|l| l.rule).collect(),
    }
}

/// [`parse_rules`] keeping each rule's position. A caller refusing a rule for something outside the
/// sidecar — what its ROOT is, or what the tab around it holds — locates that refusal from here
/// rather than reading the text again.
pub fn parse_rules_located(
    file: &str,
    root: Rect,
    content: &str,
) -> Result<LocatedRules, Vec<Diagnostic>> {
    let shape = shape_of(root);
    let mut cur = Cursor::new(content, 1);
    let mut diags: Vec<Diagnostic> = Vec::new();
    let placed = read_rules(file, &mut cur, shape, &mut diags);
    check_rule_order(file, &placed, shape, &mut diags);
    if placed.is_empty() && diags.is_empty() {
        diags.push(located(
            file,
            1,
            1,
            Code::PresentationSyntax,
            "a sidecar declaring nothing is not written; delete it".to_string(),
        ));
    }
    if diags.is_empty() {
        Ok(placed
            .into_iter()
            .map(|(line, col, rule)| LocatedRule { rule, line, col })
            .collect())
    } else {
        Err(diags)
    }
}

/// Every rule the sidecar holds. Recovery is per rule; one that is not `<selector> { … }` ends the
/// read, since nothing after it can be located against a text whose framing is no longer known.
fn read_rules(
    file: &str,
    cur: &mut Cursor<'_>,
    shape: Shape,
    diags: &mut Vec<Diagnostic>,
) -> Vec<(u32, u32, Rule)> {
    let mut placed: Vec<(u32, u32, Rule)> = Vec::new();
    loop {
        cur.skip_ws();
        if cur.peek().is_none() {
            return placed;
        }
        let (line, col) = (cur.line, cur.col);
        let selector = cur.take_until(&['{', '}', ';']).trim_end();
        if cur.peek() != Some('{') {
            diags.push(located(
                file,
                line,
                col,
                Code::PresentationSyntax,
                format!("a rule is `<selector> {{ <declarations> }}`; found {selector:?}"),
            ));
            return placed;
        }
        cur.bump();
        let target = resolve_target(file, selector, line, col, shape, diags);
        let faults_before = diags.len();
        let declarations = parse_declarations(file, cur, target, shape, diags);
        let Some(target) = target else { continue };
        if !declarations.is_empty() {
            placed.push((
                line,
                col,
                Rule {
                    target,
                    declarations,
                },
            ));
        } else if diags.len() == faults_before {
            diags.push(located(
                file,
                line,
                col,
                Code::PresentationSyntax,
                format!(
                    "the rule `{}` declares nothing; drop it",
                    spell(target, shape)
                ),
            ));
        }
    }
}

/// [`parse_rules`] read backward. The rules must already ascend by [`Target`] with each rule's
/// declarations alphabetical, the one order this parses back from; a root holding no rule writes NO
/// sidecar. `root` supplies the extent an index is spelled against, exactly as the filename does.
pub fn spell_rules(root: Rect, presentation: &Presentation) -> String {
    let shape = shape_of(root);
    let selectors: Vec<String> = presentation
        .rules
        .iter()
        .map(|rule| spell(rule.target, shape))
        .collect();
    let rules: Vec<(&str, Vec<(&str, String)>)> = presentation
        .rules
        .iter()
        .zip(&selectors)
        .map(|(rule, selector)| {
            debug_assert!(
                !rule.declarations.is_empty()
                    && rule
                        .declarations
                        .windows(2)
                        .all(|w| w[0].property() < w[1].property()),
                "a written rule declares something, alphabetically, once each",
            );
            let declared = rule
                .declarations
                .iter()
                .map(|d| (d.property(), d.value_text()))
                .collect();
            (selector.as_str(), declared)
        })
        .collect();
    debug_assert!(
        !rules.is_empty()
            && presentation
                .rules
                .windows(2)
                .all(|w| w[0].target < w[1].target),
        "a written presentation holds rules, ascending by target",
    );
    spell_sidecar(&rules)
}

/// The rule text a sidecar holds: two-space indent, a space inside each brace, `; ` between
/// declarations, no trailing `;`, one closing newline. [`split_rule`] is this read backward.
pub fn spell_sidecar(rules: &[(&str, Vec<(&str, String)>)]) -> String {
    let spelled: Vec<String> = rules
        .iter()
        .map(|(selector, declarations)| {
            let declared: Vec<String> = declarations
                .iter()
                .map(|(property, value)| format!("{property}: {value}"))
                .collect();
            format!("  {selector} {{ {} }}", declared.join("; "))
        })
        .collect();
    format!("{}\n", spelled.join("\n"))
}

/// What one rule's text broke, as DATA rather than a [`Diagnostic`]: a caller states it in its own
/// vocabulary, over its own code and location.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RuleFault {
    /// The text found, which is no one rule.
    Malformed(String),
    /// A `;` that no `; ` separates, so the rule ends on one.
    TrailingSeparator,
    /// The segment holding no `: `.
    NotADeclaration(String),
    /// The segment spelled with the wrong space.
    NonCanonical(String),
    /// The property no `allowed` admits.
    UnknownProperty(String),
    /// The property declared twice.
    Duplicate(String),
    /// The pair out of alphabetical order.
    OutOfOrder { before: String, after: String },
}

/// [`spell_sidecar`] read backward over a file holding ONE rule, as far as its FRAME: the selector
/// between the indent and ` { `, and the declaration text inside the braces, unread. Anything else
/// is a second spelling of one appearance, which is the very thing a sidecar has none of. The
/// selector is mandatory and judged no further HERE, so a caller grades it before its declarations.
pub fn split_rule(text: &str) -> Result<(&str, &str), RuleFault> {
    let body = text
        .strip_suffix('\n')
        .filter(|body| !body.contains('\n'))
        .ok_or_else(|| RuleFault::Malformed(text.to_string()))?;
    let malformed = || RuleFault::Malformed(body.to_string());
    let (selector, inner) = body
        .strip_prefix("  ")
        .and_then(|rest| rest.split_once(" { "))
        .and_then(|(selector, rest)| Some((selector, rest.strip_suffix(" }")?)))
        .ok_or_else(malformed)?;
    if selector.is_empty() || inner.is_empty() || inner.contains(['{', '}']) {
        return Err(malformed());
    }
    Ok((selector, inner))
}

/// The declaration text [`split_rule`] left unread, one pair per declaration in the order written.
/// `allowed` is the caller's property vocabulary, applied per declaration before the duplicate and
/// order verdict.
pub fn read_declarations<'a>(
    inner: &'a str,
    allowed: &[&str],
) -> Result<Vec<(&'a str, &'a str)>, RuleFault> {
    let mut declarations: Vec<(&str, &str)> = Vec::new();
    for segment in inner.split("; ") {
        if segment.contains(';') {
            return Err(RuleFault::TrailingSeparator);
        }
        let (property, value) = segment
            .split_once(": ")
            .ok_or_else(|| RuleFault::NotADeclaration(segment.to_string()))?;
        if property.trim() != property || value.trim() != value || value.is_empty() {
            return Err(RuleFault::NonCanonical(segment.to_string()));
        }
        if !allowed.contains(&property) {
            return Err(RuleFault::UnknownProperty(property.to_string()));
        }
        if let Some((before, _)) = declarations.last() {
            match adjacent(before, property) {
                Adjacent::Duplicate => return Err(RuleFault::Duplicate(property.to_string())),
                Adjacent::OutOfOrder => {
                    return Err(RuleFault::OutOfOrder {
                        before: (*before).to_string(),
                        after: property.to_string(),
                    });
                }
                Adjacent::Ascending => {}
            }
        }
        declarations.push((property, value));
    }
    Ok(declarations)
}

/// The one order every sidecar's declarations are held to, read by both legs that judge it.
enum Adjacent {
    Ascending,
    Duplicate,
    OutOfOrder,
}

fn adjacent(before: &str, after: &str) -> Adjacent {
    if after == before {
        Adjacent::Duplicate
    } else if after < before {
        Adjacent::OutOfOrder
    } else {
        Adjacent::Ascending
    }
}

/// The extent a root's selectors count in: its own, the filename no longer supplying one.
fn shape_of(root: Rect) -> Shape {
    Shape {
        rows: root.max_row - root.min_row + 1,
        cols: root.max_col - root.min_col + 1,
    }
}

/// `None` once the selector has earned a refusal of its own, which is also what keeps a rule from
/// being reported both for its selector and for the emptiness that follows from it.
fn resolve_target(
    file: &str,
    selector: &str,
    line: u32,
    col_at: u32,
    shape: Shape,
    diags: &mut Vec<Diagnostic>,
) -> Option<Target> {
    match parse_selector(selector, shape) {
        Ok(target) => {
            let target = canonicalize(target, shape);
            // PRES1, after canonicalisation: on a one-row or one-column root a cell selector folds to `Col`/`Row` and stays legal, so what is left truly addresses a coordinate.
            if let Target::Cell { row, col } = target {
                let at = crate::Rect::cell(col - 1, row - 1).label();
                diags.push(located(
                    file,
                    line,
                    col_at,
                    Code::PresentationSelector,
                    format!(
                        "{selector:?} addresses a coordinate; a selector states a region's SHAPE. \
                         State that cell in its own sidecar instead: the root's {at} as <cell>.css"
                    ),
                ));
                return None;
            }
            let canonical = spell(target, shape);
            // Compared VERBATIM, never whitespace-folded: a tab or a line break between two compounds is a second spelling of one appearance exactly as `#FFF` is.
            if selector == canonical {
                return Some(target);
            }
            diags.push(located(
                file,
                line,
                col_at,
                Code::NonCanonicalPresentation,
                format!("non-canonical selector {selector:?}: write `{canonical}`"),
            ));
            None
        }
        Err((code, message)) => {
            diags.push(located(file, line, col_at, code, message));
            None
        }
    }
}

/// Consumes the declaration list through its closing `}`. Recovery is per declaration, so an author
/// sees every fault in a rule rather than only its first. `target` is `None` where the selector has
/// already earned a refusal, and the declarations it governs are then read for their own faults only.
fn parse_declarations(
    file: &str,
    cur: &mut Cursor<'_>,
    target: Option<Target>,
    shape: Shape,
    diags: &mut Vec<Diagnostic>,
) -> Vec<Declaration> {
    let mut parsed: Vec<(u32, u32, Declaration)> = Vec::new();
    let mut after_separator = false;
    loop {
        cur.skip_ws();
        let (line, col) = (cur.line, cur.col);
        match cur.peek() {
            None => {
                diags.push(located(
                    file,
                    line,
                    col,
                    Code::PresentationSyntax,
                    "the rule is never closed; a rule ends with `}`".to_string(),
                ));
                break;
            }
            Some('}') => {
                cur.bump();
                if after_separator {
                    diags.push(located(
                        file,
                        line,
                        col,
                        Code::PresentationSyntax,
                        "a declaration list has no empty segment; drop the trailing `;`"
                            .to_string(),
                    ));
                }
                break;
            }
            _ => {}
        }
        let text = cur.take_until(&[';', '}', '{']).trim_end();
        if text.is_empty() {
            diags.push(located(
                file,
                line,
                col,
                Code::PresentationSyntax,
                "a declaration list has no empty segment; drop the extra `;`".to_string(),
            ));
        } else {
            let read = parse_declaration(text).and_then(|d| match axis_fault(&d, target, shape) {
                Some(fault) => Err(fault),
                None => Ok(d),
            });
            match read {
                Ok(d) => parsed.push((line, col, d)),
                Err((code, message)) => diags.push(located(file, line, col, code, message)),
            }
        }
        after_separator = cur.peek() == Some(';');
        if after_separator {
            cur.bump();
        } else if cur.peek() == Some('{') {
            diags.push(located(
                file,
                cur.line,
                cur.col,
                Code::PresentationSyntax,
                "a declaration holds no `{`; a rule ends with `}`".to_string(),
            ));
            break;
        }
    }
    check_declaration_order(file, &parsed, diags);
    parsed.into_iter().map(|(_, _, d)| d).collect()
}

fn check_declaration_order(
    file: &str,
    parsed: &[(u32, u32, Declaration)],
    diags: &mut Vec<Diagnostic>,
) {
    for window in parsed.windows(2) {
        let ((_, _, before), (line, col, after)) = (&window[0], &window[1]);
        let (a, b) = (before.property(), after.property());
        match adjacent(a, b) {
            Adjacent::Duplicate => diags.push(located(
                file,
                *line,
                *col,
                Code::PresentationSyntax,
                format!("`{b}` is declared twice in one rule; give it one declaration"),
            )),
            Adjacent::OutOfOrder => diags.push(located(
                file,
                *line,
                *col,
                Code::NonCanonicalPresentation,
                format!("declarations are alphabetical: write `{b}` before `{a}`"),
            )),
            Adjacent::Ascending => {}
        }
    }
}

fn check_rule_order(
    file: &str,
    placed: &[(u32, u32, Rule)],
    shape: Shape,
    diags: &mut Vec<Diagnostic>,
) {
    for window in placed.windows(2) {
        let ((_, _, before), (line, col, after)) = (&window[0], &window[1]);
        if after.target == before.target {
            diags.push(located(
                file,
                *line,
                *col,
                Code::PresentationSyntax,
                format!(
                    "the selector `{}` is declared twice; give it one rule",
                    spell(after.target, shape)
                ),
            ));
        } else if after.target < before.target {
            diags.push(located(
                file,
                *line,
                *col,
                Code::NonCanonicalPresentation,
                format!(
                    "rules run all, periodic rows, periodic columns, rows, columns then cells, each ascending: write `{}` before `{}`",
                    spell(after.target, shape),
                    spell(before.target, shape),
                ),
            ));
        }
    }
}

/// An axis of extent 1 carries no selector of its own: with H = 1 a row selector and a cell selector
/// pick out the whole region and a column respectively, and folding them here is what leaves one
/// spelling per appearance.
fn canonicalize(target: Target, shape: Shape) -> Target {
    let target = match target {
        Target::RowEvery { a, b } => match lone(a, b, shape.rows) {
            Some(row) => Target::Row(row),
            None => target,
        },
        Target::ColEvery { a, b } => match lone(a, b, shape.cols) {
            Some(col) => Target::Col(col),
            None => target,
        },
        other => other,
    };
    let target = match target {
        Target::Row(_) | Target::RowEvery { .. } if shape.rows == 1 => Target::All,
        Target::Cell { col, .. } if shape.rows == 1 => Target::Col(col),
        other => other,
    };
    match target {
        Target::Col(_) | Target::ColEvery { .. } if shape.cols == 1 => Target::All,
        Target::Cell { row, .. } if shape.cols == 1 => Target::Row(row),
        other => other,
    }
}

/// The one line `An+B` reaches inside `extent`, where it reaches only one — which a LITERAL index
/// already spells, so leaving it periodic would give one appearance two spellings.
fn lone(a: u32, b: u32, extent: u32) -> Option<u32> {
    let first = if b == 0 { a } else { b };
    (first <= extent && first + a > extent).then_some(first)
}

fn spell(target: Target, shape: Shape) -> String {
    match target {
        Target::All => "fsa1-cell".to_string(),
        Target::RowEvery { a, b } => format!("fsa1-row{} fsa1-cell", periodic(a, b)),
        Target::ColEvery { a, b } => format!("fsa1-cell{}", periodic(a, b)),
        Target::Row(r) => format!("fsa1-row{} fsa1-cell", pseudo(r, shape.rows)),
        Target::Col(c) => format!("fsa1-cell{}", pseudo(c, shape.cols)),
        Target::Cell { row, col } => format!(
            "fsa1-row{} fsa1-cell{}",
            pseudo(row, shape.rows),
            pseudo(col, shape.cols)
        ),
    }
}

/// A periodic index carries `B` only when it is non-zero, because `An+0` and `An` select the same
/// lines and the format spells one appearance one way.
fn periodic(a: u32, b: u32) -> String {
    if b == 0 {
        format!(":nth-child({a}n)")
    } else {
        format!(":nth-child({a}n+{b})")
    }
}

/// `:nth-child(1)` ties with `:first-child` on specificity and matches the same line, so index 1 and
/// index `extent` have one spelling each.
fn pseudo(index: u32, extent: u32) -> String {
    if index == 1 {
        ":first-child".to_string()
    } else if index == extent {
        ":last-child".to_string()
    } else {
        format!(":nth-child({index})")
    }
}

fn parse_selector(text: &str, shape: Shape) -> Result<Target, (Code, String)> {
    if text.starts_with('@') {
        return Err(syntax(&format!(
            "an at-rule has no place inside a sidecar: {text:?}"
        )));
    }
    let parts: Vec<&str> = text.split_whitespace().collect();
    match parts.as_slice() {
        [cell] => Ok(match column_of(cell, text, shape.cols)? {
            None => Target::All,
            Some(Idx::At(col)) => Target::Col(col),
            Some(Idx::Every { a, b }) => Target::ColEvery { a, b },
        }),
        [row, cell] => match (
            row_of(row, text, shape.rows)?,
            column_of(cell, text, shape.cols)?,
        ) {
            (Idx::At(row), None) => Ok(Target::Row(row)),
            (Idx::At(row), Some(Idx::At(col))) => Ok(Target::Cell { row, col }),
            (Idx::Every { a, b }, None) => Ok(Target::RowEvery { a, b }),
            // No periodic CELL target exists, so a periodic part composes with nothing.
            _ => Err(unknown_selector(text)),
        },
        _ => Err(unknown_selector(text)),
    }
}

/// One axis index: a literal line, or every `a`th line offset by `b`.
#[derive(Clone, Copy)]
enum Idx {
    At(u32),
    Every { a: u32, b: u32 },
}

/// `Ok(None)` is the bare `fsa1-cell`, which selects whatever its row part already narrowed to.
fn column_of(part: &str, whole: &str, cols: u32) -> Result<Option<Idx>, (Code, String)> {
    let Some(rest) = part.strip_prefix("fsa1-cell") else {
        return Err(unknown_selector(whole));
    };
    if rest.is_empty() {
        return Ok(None);
    }
    index_of(rest, whole, cols, "column").map(Some)
}

fn row_of(part: &str, whole: &str, rows: u32) -> Result<Idx, (Code, String)> {
    let Some(rest) = part.strip_prefix("fsa1-row") else {
        return Err(unknown_selector(whole));
    };
    index_of(rest, whole, rows, "row")
}

fn index_of(pseudo: &str, whole: &str, extent: u32, axis: &str) -> Result<Idx, (Code, String)> {
    let index = if pseudo == ":first-child" {
        1
    } else if pseudo == ":last-child" {
        extent
    } else if let Some(k) = pseudo
        .strip_prefix(":nth-child(")
        .and_then(|s| s.strip_suffix(')'))
    {
        // Before the integer parse: `2n` is not a number and would die as a malformed one.
        if let Some((a, b)) = split_periodic(k) {
            return periodic_of(a, b, whole, extent, axis);
        }
        k.parse::<u32>().map_err(|_| unknown_selector(whole))?
    } else {
        return Err(unknown_selector(whole));
    };
    if index == 0 || index > extent {
        return Err((
            Code::PresentationSelector,
            format!("{axis} {index} is outside the region's {extent}: {whole:?}"),
        ));
    }
    Ok(Idx::At(index))
}

/// Splits `An` or `An+B` into its two numbers. `odd` and `even` reach here as the keywords they are
/// and split to nothing, which is what refuses them: they are `2n+1` and `2n` under another name,
/// and one appearance is spelled one way.
fn split_periodic(k: &str) -> Option<(&str, &str)> {
    let (a, rest) = k.split_once('n')?;
    match rest.strip_prefix('+') {
        Some(b) => Some((a, b)),
        None if rest.is_empty() => Some((a, "0")),
        None => None,
    }
}

/// `A` of 1 selects every line, which is `fsa1-cell`, and `A` of 0 selects one, which is a literal index —
/// each already has a spelling, so admitting a second here would give one appearance two.
fn periodic_of(
    a: &str,
    b: &str,
    whole: &str,
    extent: u32,
    axis: &str,
) -> Result<Idx, (Code, String)> {
    let a: u32 = a.parse().map_err(|_| unknown_selector(whole))?;
    let b: u32 = b.parse().map_err(|_| unknown_selector(whole))?;
    if a < 2 {
        return Err((
            Code::PresentationSelector,
            format!("a periodic {axis} repeats every 2 or more, not every {a}: {whole:?}"),
        ));
    }
    if b >= a {
        return Err((
            Code::PresentationSelector,
            format!(
                "a periodic {axis} offset runs 0 to {}, not {b}: {whole:?}",
                a - 1
            ),
        ));
    }
    // Lines b, b+a, b+2a, … — but at offset 0 line 0 does not exist, so the first is a.
    let first = if b == 0 { a } else { b };
    if first > extent {
        return Err((
            Code::PresentationSelector,
            format!(
                "a periodic {axis} first selects {first}, outside the region's {extent}: {whole:?}"
            ),
        ));
    }
    Ok(Idx::Every { a, b })
}

fn unknown_selector(text: &str) -> (Code, String) {
    (
        Code::PresentationSelector,
        format!("{text:?} is not one of the ten region-relative selectors"),
    )
}

/// A size belongs to an AXIS, and only a selector naming one may carry it: a cell selector names two
/// and so is neither, and the cross axis — a row for a width, a column for a height — is not the one
/// being sized. `fsa1-cell` names the whole region and stays legal for both, which is what leaves a file one
/// column wide, where no column selector can be spelled, a way to size its column.
fn axis_fault(
    declaration: &Declaration,
    target: Option<Target>,
    shape: Shape,
) -> Option<(Code, String)> {
    let target = target?;
    let (sizes_axis, axis, forms) = match declaration {
        Declaration::Width(_) => (
            matches!(target, Target::All | Target::Col(_)),
            "column",
            "`fsa1-cell` or `fsa1-cell:nth-child(k)`",
        ),
        Declaration::Height(_) => (
            matches!(target, Target::All | Target::Row(_)),
            "row",
            "`fsa1-cell` or `fsa1-row:nth-child(k) fsa1-cell`",
        ),
        _ => return None,
    };
    (!sizes_axis).then(|| {
        (
            Code::PresentationProperty,
            format!(
                "`{}` sizes a {axis}, and `{}` names no {axis}: write it on {forms}",
                declaration.property(),
                spell(target, shape),
            ),
        )
    })
}

fn located(file: &str, line: u32, col: u32, code: Code, message: String) -> Diagnostic {
    Diagnostic::new(code, Loc::body(file, line, col), message)
}

/// Tracks the file line and byte column of every character it consumes, so a fault inside the block
/// is located exactly as a fault inside the grid is.
struct Cursor<'a> {
    rest: &'a str,
    line: u32,
    col: u32,
}

impl<'a> Cursor<'a> {
    fn new(src: &'a str, line: u32) -> Cursor<'a> {
        Cursor {
            rest: src,
            line,
            col: 1,
        }
    }

    fn peek(&self) -> Option<char> {
        self.rest.chars().next()
    }

    fn bump(&mut self) -> Option<char> {
        let c = self.peek()?;
        self.rest = &self.rest[c.len_utf8()..];
        if c == '\n' {
            self.line += 1;
            self.col = 1;
        } else {
            self.col += c.len_utf8() as u32;
        }
        Some(c)
    }

    fn skip_ws(&mut self) {
        while self.peek().is_some_and(char::is_whitespace) {
            self.bump();
        }
    }

    fn take_until(&mut self, stop: &[char]) -> &'a str {
        let start = self.rest;
        let mut taken = 0usize;
        while let Some(c) = self.peek() {
            if stop.contains(&c) {
                break;
            }
            self.bump();
            taken += c.len_utf8();
        }
        &start[..taken]
    }
}

#[cfg(test)]
mod tests {
    /// The retired form's open line, which these helpers rewrite into a prelude.
    const OPEN: &str = "@scope {";

    use super::*;
    use crate::declaration::{
        BORDER_LINES, Border, BorderLine, Chars, Edge, FontStyle, FontWeight, Points, Rgb,
        TextAlign, TextDecoration, VerticalAlign, WhiteSpace,
    };

    const REGION: Shape = Shape { rows: 4, cols: 3 };

    /// The root a `shape` spells when it is anchored at A1 — the one a test's rules are read under.
    fn root_of(shape: Shape) -> Rect {
        Rect {
            min_col: 0,
            min_row: 0,
            max_col: shape.cols - 1,
            max_row: shape.rows - 1,
        }
    }

    /// The sidecar path a `shape`'s rules are located against.
    fn sidecar(shape: Shape) -> String {
        format!("Sheet1/{}.css", root_of(shape).label())
    }

    /// The rules a test's case states. A case frames them in the open and close the format retired,
    /// plus the shape they are read at; the frame is stripped here rather than dropped from every
    /// literal. Anything before the open goes with it: a sidecar holds rules and no grid.
    fn body(content: &str) -> String {
        let inner = content
            .split_once(OPEN)
            .expect("a case frames its rules with `@scope {`")
            .1;
        let inner = inner
            .rsplit_once('}')
            .expect("a case closes its frame with `}`")
            .0;
        format!("{}\n", inner.trim_matches('\n'))
    }

    fn parse(content: &str, shape: Shape) -> Result<Presentation, Vec<Diagnostic>> {
        parse_rules(&sidecar(shape), root_of(shape), &body(content))
    }

    fn rules(content: &str) -> Vec<Rule> {
        parse(content, REGION)
            .unwrap_or_else(|d| panic!("{content:?} should parse: {:?}", d[0]))
            .rules
    }

    fn refusal(content: &str) -> Diagnostic {
        parse(content, REGION)
            .err()
            .unwrap_or_else(|| panic!("{content:?} should be refused"))
            .remove(0)
    }

    fn one_rule(selector: &str) -> Target {
        rules(&format!("@scope {{\n  {selector} {{ color: #3f0421 }}\n}}"))[0].target
    }

    /// [`one_rule`] for the selectors that do not survive: the message, so a test names the reason
    /// it was refused and not merely that something was.
    fn selector_refusal(selector: &str) -> String {
        refusal(&format!("@scope {{\n  {selector} {{ color: #3f0421 }}\n}}")).message
    }

    #[test]
    fn a_multi_line_rule_body_reads_as_one_rule() {
        assert_eq!(
            rules("@scope {\n  fsa1-cell {\n    color: #3f0421\n  }\n}")[0].target,
            Target::All,
        );
    }

    #[test]
    fn every_selector_form_reads_to_its_region_relative_target() {
        assert_eq!(one_rule("fsa1-cell"), Target::All);
        assert_eq!(one_rule("fsa1-row:first-child fsa1-cell"), Target::Row(1));
        assert_eq!(one_rule("fsa1-row:last-child fsa1-cell"), Target::Row(4));
        assert_eq!(one_rule("fsa1-row:nth-child(2) fsa1-cell"), Target::Row(2));
        assert_eq!(one_rule("fsa1-cell:first-child"), Target::Col(1));
        assert_eq!(one_rule("fsa1-cell:last-child"), Target::Col(3));
        assert_eq!(one_rule("fsa1-cell:nth-child(2)"), Target::Col(2));
        // A cell selector reads to a coordinate and PRES1 refuses one; here only the shapes are read.
        assert_eq!(
            one_rule("fsa1-row:nth-child(2n) fsa1-cell"),
            Target::RowEvery { a: 2, b: 0 }
        );
        assert_eq!(
            one_rule("fsa1-row:nth-child(2n+1) fsa1-cell"),
            Target::RowEvery { a: 2, b: 1 }
        );
        assert_eq!(
            one_rule("fsa1-cell:nth-child(2n+1)"),
            Target::ColEvery { a: 2, b: 1 }
        );
    }

    /// The lines are `b, b+a, b+2a, …`, so at offset 0 the FIRST is `a` and not 0. A period wider
    /// than the region is therefore admissible whenever it still reaches TWO lines of it.
    #[test]
    fn a_periodic_index_is_bounded_by_its_first_line_not_its_period() {
        assert_eq!(
            one_rule("fsa1-row:nth-child(3n+1) fsa1-cell"),
            Target::RowEvery { a: 3, b: 1 }
        );
        assert!(selector_refusal("fsa1-row:nth-child(7n) fsa1-cell").contains("first selects 7"));
    }

    /// A period reaching ONE line of the region picks out exactly what a literal index does, so the
    /// author is sent to the spelling that set already has rather than being given a second.
    #[test]
    fn a_periodic_index_reaching_one_line_is_that_literal_index() {
        let diag = refusal("@scope {\n  fsa1-row:nth-child(7n+3) fsa1-cell { color: #3f0421 }\n}");
        assert_eq!(diag.code, Code::NonCanonicalPresentation);
        assert!(
            diag.message
                .contains("write `fsa1-row:nth-child(3) fsa1-cell`"),
            "{}",
            diag.message
        );
    }

    /// One appearance, one spelling: each refusal below names a set some EXISTING form already
    /// spells, so admitting it would give that set a second way to be written.
    #[test]
    fn a_periodic_index_admits_no_synonym_of_a_form_that_exists() {
        assert!(selector_refusal("fsa1-row:nth-child(1n) fsa1-cell").contains("every 2 or more"));
        assert!(selector_refusal("fsa1-row:nth-child(0n+2) fsa1-cell").contains("every 2 or more"));
        assert!(
            selector_refusal("fsa1-row:nth-child(2n+2) fsa1-cell").contains("offset runs 0 to 1")
        );
        assert!(
            selector_refusal("fsa1-row:nth-child(odd) fsa1-cell")
                .contains("ten region-relative selectors")
        );
        assert!(
            selector_refusal("fsa1-row:nth-child(even) fsa1-cell")
                .contains("ten region-relative selectors")
        );
    }

    /// A size belongs to an AXIS, and the check that says so is an ALLOWLIST of the forms that name
    /// one — so a periodic form is refused by construction, not by a branch anyone must remember.
    #[test]
    fn a_periodic_selector_carries_no_size() {
        let height = "@scope {\n  fsa1-row:nth-child(2n) fsa1-cell { height: 22.5pt }\n}";
        assert_eq!(refusal(height).code, Code::PresentationProperty);
        let width = "@scope {\n  fsa1-cell:nth-child(2n+1) { width: 14.5ch }\n}";
        assert_eq!(refusal(width).code, Code::PresentationProperty);
        rules("@scope {\n  fsa1-row:nth-child(2) fsa1-cell { height: 22.5pt }\n}");
    }

    /// There is no periodic CELL target, so the two halves cannot both narrow.
    #[test]
    fn a_periodic_part_composes_with_nothing() {
        assert!(
            selector_refusal("fsa1-row:nth-child(2n) fsa1-cell:nth-child(3)")
                .contains("ten region-relative selectors")
        );
        assert!(
            selector_refusal("fsa1-row:nth-child(3) fsa1-cell:nth-child(2n)")
                .contains("ten region-relative selectors")
        );
    }

    /// An axis of extent 1 has nothing to alternate over, so a periodic index there selects exactly
    /// what `fsa1-cell` does — and the author is told which spelling that set already has.
    #[test]
    fn a_periodic_index_on_a_single_line_axis_is_refused_for_the_form_that_says_it() {
        let one_row = Shape { rows: 1, cols: 3 };
        let content = "@scope {\n  fsa1-row:nth-child(2n+1) fsa1-cell { color: #3f0421 }\n}";
        let diag = parse(content, one_row)
            .expect_err("a single-row region spells this `fsa1-cell`")
            .remove(0);
        assert_eq!(diag.code, Code::NonCanonicalPresentation);
        assert!(
            diag.message.contains("write `fsa1-cell`"),
            "{}",
            diag.message
        );
    }

    #[test]
    fn every_property_reads_to_its_typed_declaration() {
        let block = "@scope {\n  fsa1-cell { background-color: #ffffff; border-bottom: 1px solid #3f0421; \
                     color: #3f0421; font-family: Times New Roman; font-size: 11pt; \
                     font-style: italic; font-weight: bold; height: 22.5pt; text-align: right; \
                     text-decoration: line-through; vertical-align: middle; white-space: nowrap; \
                     width: 14.5ch }\n}";
        assert_eq!(
            rules(block)[0].declarations,
            vec![
                Declaration::BackgroundColor(Rgb {
                    r: 255,
                    g: 255,
                    b: 255
                }),
                Declaration::Border {
                    edge: Edge::Bottom,
                    border: Border {
                        line: BorderLine::ThinSolid,
                        color: Rgb {
                            r: 0x3f,
                            g: 0x04,
                            b: 0x21
                        },
                    },
                },
                Declaration::Color(Rgb {
                    r: 0x3f,
                    g: 0x04,
                    b: 0x21
                }),
                Declaration::FontFamily("Times New Roman".to_string()),
                Declaration::FontSize(Points(11.0)),
                Declaration::FontStyle(FontStyle::Italic),
                Declaration::FontWeight(FontWeight::Bold),
                Declaration::Height(Points(22.5)),
                Declaration::TextAlign(TextAlign::Right),
                Declaration::TextDecoration(TextDecoration::LineThrough),
                Declaration::VerticalAlign(VerticalAlign::Middle),
                Declaration::WhiteSpace(WhiteSpace::Nowrap),
                Declaration::Width(Chars(14.5)),
            ]
        );
    }

    #[test]
    fn a_real_region_block_parses_as_written() {
        let content = "=Total!I3\t=Total!J3\t=Total!K3\n@scope {\n  fsa1-cell { border-bottom: 1px solid \
                       #3f0421; color: #3f0421; font-weight: bold; text-align: center }\n}";
        let parsed = parse(content, Shape { rows: 1, cols: 3 }).expect("parses");
        assert_eq!(parsed.rules.len(), 1);
        assert_eq!(parsed.rules[0].target, Target::All);
        assert_eq!(parsed.rules[0].declarations.len(), 4);
    }

    #[test]
    fn the_seven_border_lines_are_the_whole_set() {
        for (line, width, style) in BORDER_LINES {
            let block =
                format!("@scope {{\n  fsa1-cell {{ border-top: {width} {style} #3f0421 }}\n}}");
            assert_eq!(
                rules(&block)[0].declarations[0],
                Declaration::Border {
                    edge: Edge::Top,
                    border: Border {
                        line: *line,
                        color: Rgb {
                            r: 0x3f,
                            g: 0x04,
                            b: 0x21
                        },
                    },
                },
            );
        }
        assert_eq!(
            refusal("@scope {\n  fsa1-cell { border-top: 2px dotted #3f0421 }\n}").code,
            Code::PresentationValue,
        );
    }

    #[test]
    fn a_width_only_border_is_refused_because_it_renders_nothing() {
        let d = refusal("@scope {\n  fsa1-cell { border-bottom: thin }\n}");
        assert_eq!(d.code, Code::PresentationValue);
        assert!(d.message.contains("all three"), "{}", d.message);
    }

    #[test]
    fn every_refused_construct_is_a_located_refusal() {
        for (block, code) in [
            (
                "@scope {\n  fsa1-cell { color: #3f0421 !important }\n}",
                Code::PresentationSyntax,
            ),
            (
                "@scope {\n  @media print { fsa1-cell { color: #3f0421 } }\n}",
                Code::PresentationSyntax,
            ),
            (
                "@scope {\n  @import url(x.css);\n}",
                Code::PresentationSyntax,
            ),
            (
                "@scope {\n  @layer base { fsa1-cell { color: #3f0421 } }\n}",
                Code::PresentationSyntax,
            ),
            (
                "@scope {\n  fsa1-cell { font-size: calc(11pt + 1pt) }\n}",
                Code::PresentationValue,
            ),
            (
                "@scope {\n  fsa1-cell { font-size: 11px }\n}",
                Code::PresentationValue,
            ),
            (
                "@scope {\n  fsa1-cell { font-size: 1em }\n}",
                Code::PresentationValue,
            ),
            (
                "@scope {\n  fsa1-cell { font-size: 1rem }\n}",
                Code::PresentationValue,
            ),
            (
                "@scope {\n  fsa1-cell { font-size: 120% }\n}",
                Code::PresentationValue,
            ),
            (
                "@scope {\n  fsa1-cell { color: currentcolor }\n}",
                Code::PresentationValue,
            ),
            (
                "@scope {\n  fsa1-cell { background-color: linear-gradient(red, blue) }\n}",
                Code::PresentationValue,
            ),
            (
                "@scope {\n  fsa1-cell { box-shadow: 0 0 2px #3f0421 }\n}",
                Code::PresentationProperty,
            ),
            (
                "@scope {\n  fsa1-cell { text-shadow: 0 0 2px #3f0421 }\n}",
                Code::PresentationProperty,
            ),
            (
                "@scope {\n  fsa1-cell { transition: color 1s }\n}",
                Code::PresentationProperty,
            ),
            (
                "@scope {\n  fsa1-cell::before { color: #3f0421 }\n}",
                Code::PresentationSelector,
            ),
            (
                "@scope {\n  fsa1-cell::after { color: #3f0421 }\n}",
                Code::PresentationSelector,
            ),
            (
                "@scope {\n  fsa1-cell:nth-col(2) { color: #3f0421 }\n}",
                Code::PresentationSelector,
            ),
            (
                "@scope {\n  ::column { color: #3f0421 }\n}",
                Code::PresentationSelector,
            ),
            (
                "@scope {\n  th { color: #3f0421 }\n}",
                Code::PresentationSelector,
            ),
            (
                "@scope {\n  fsa1-cell:nth-child(9) { color: #3f0421 }\n}",
                Code::PresentationSelector,
            ),
            (
                "@scope {\n  fsa1-cell { color: #3f0421 font-weight: bold }\n}",
                Code::PresentationSyntax,
            ),
        ] {
            let d = refusal(block);
            assert_eq!(d.code, code, "{block:?} -> {}", d.message);
            assert!(
                matches!(d.loc, Loc::Body { .. }),
                "{block:?} must be located: {:?}",
                d.loc
            );
        }
    }

    /// A width is a column's and a height a row's, so the selector must name the axis being sized.
    /// `fsa1-cell` names the whole region and stays legal for both — the only way a one-column file, which
    /// can spell no column selector at all, sizes its column.
    #[test]
    fn a_size_is_refused_on_a_selector_that_names_no_such_axis() {
        for (block, want) in [
            (
                "@scope {\n  fsa1-cell:nth-child(2) { height: 22.5pt }\n}",
                "no row",
            ),
            (
                "@scope {\n  fsa1-row:nth-child(2) fsa1-cell { width: 14.5ch }\n}",
                "no column",
            ),
            (
                "@scope {\n  fsa1-row:nth-child(2) fsa1-cell { width: 14.5ch }\n}",
                "no column",
            ),
            (
                "@scope {\n  fsa1-cell:nth-child(2) { height: 22.5pt }\n}",
                "no row",
            ),
        ] {
            let d = refusal(block);
            assert_eq!(d.code, Code::PresentationProperty, "{block:?}");
            assert!(matches!(d.loc, Loc::Body { .. }), "{block:?}: {:?}", d.loc);
            assert!(d.message.contains(want), "{block:?} -> {}", d.message);
        }
        for block in [
            "@scope {\n  fsa1-cell { height: 22.5pt; width: 14.5ch }\n}",
            "@scope {\n  fsa1-row:nth-child(2) fsa1-cell { height: 22.5pt }\n}",
            "@scope {\n  fsa1-cell:nth-child(2) { width: 14.5ch }\n}",
        ] {
            assert!(parse(block, REGION).is_ok(), "{block:?}");
        }
        let one_col = Shape { rows: 4, cols: 1 };
        assert!(parse("@scope {\n  fsa1-cell { width: 14.5ch }\n}", one_col).is_ok());
    }

    #[test]
    fn an_axis_size_takes_its_own_unit_and_excels_own_range() {
        for value in [
            "width: 10px",
            "width: 10pt",
            "width: 256ch",
            "width: -1ch",
            "height: 15ch",
            "height: 410pt",
            "height: -1pt",
        ] {
            let block = format!("@scope {{\n  fsa1-cell {{ {value} }}\n}}");
            let d = refusal(&block);
            assert_eq!(
                d.code,
                Code::PresentationValue,
                "{value:?} -> {}",
                d.message
            );
            assert!(
                d.message.len() < 100,
                "{value:?} must earn an actionable message: {}",
                d.message
            );
        }
        assert_eq!(
            rules("@scope {\n  fsa1-cell { width: 0ch }\n}")[0].declarations[0],
            Declaration::Width(Chars(0.0)),
        );
    }

    #[test]
    fn a_missing_separator_names_the_separator_rather_than_the_value() {
        let d = refusal("@scope {\n  fsa1-cell { color: #3f0421 font-weight: bold }\n}");
        assert!(d.message.contains("separated by `;`"), "{}", d.message);
    }

    /// The third column is the block with the named rewrite APPLIED, supplied rather than scraped out
    /// of the message: a rewrite targets a whole declaration, a value alone, a selector, or an
    /// ordering, so there is no one span to substitute into and no mechanical way to derive it today.
    #[test]
    fn every_non_canonical_spelling_carries_a_rewrite_that_retires_it() {
        for (block, want, rewritten) in [
            (
                "@scope {\n  fsa1-cell:nth-child(1) { color: #3f0421 }\n}",
                "fsa1-cell:first-child",
                "@scope {\n  fsa1-cell:first-child { color: #3f0421 }\n}",
            ),
            (
                "@scope {\n  fsa1-cell:nth-child(3) { color: #3f0421 }\n}",
                "fsa1-cell:last-child",
                "@scope {\n  fsa1-cell:last-child { color: #3f0421 }\n}",
            ),
            (
                "@scope {\n  fsa1-row:nth-child(1) fsa1-cell { color: #3f0421 }\n}",
                "fsa1-row:first-child fsa1-cell",
                "@scope {\n  fsa1-row:first-child fsa1-cell { color: #3f0421 }\n}",
            ),
            (
                "@scope {\n  fsa1-row:nth-child(4) fsa1-cell { color: #3f0421 }\n}",
                "fsa1-row:last-child fsa1-cell",
                "@scope {\n  fsa1-row:last-child fsa1-cell { color: #3f0421 }\n}",
            ),
            (
                "@scope {\n  fsa1-row:first-child\tfsa1-cell { color: #3f0421 }\n}",
                "fsa1-row:first-child fsa1-cell",
                "@scope {\n  fsa1-row:first-child fsa1-cell { color: #3f0421 }\n}",
            ),
            (
                "@scope {\n  fsa1-row:first-child  fsa1-cell { color: #3f0421 }\n}",
                "fsa1-row:first-child fsa1-cell",
                "@scope {\n  fsa1-row:first-child fsa1-cell { color: #3f0421 }\n}",
            ),
            (
                "@scope {\n  fsa1-row:first-child\n  fsa1-cell { color: #3f0421 }\n}",
                "fsa1-row:first-child fsa1-cell",
                "@scope {\n  fsa1-row:first-child fsa1-cell { color: #3f0421 }\n}",
            ),
            (
                "@scope {\n  fsa1-cell { color: #3F0421 }\n}",
                "#3f0421",
                "@scope {\n  fsa1-cell { color: #3f0421 }\n}",
            ),
            (
                "@scope {\n  fsa1-cell { color: #fff }\n}",
                "#ffffff",
                "@scope {\n  fsa1-cell { color: #ffffff }\n}",
            ),
            (
                "@scope {\n  fsa1-cell { font-weight: 700 }\n}",
                "bold",
                "@scope {\n  fsa1-cell { font-weight: bold }\n}",
            ),
            (
                "@scope {\n  fsa1-cell { font-weight: 400 }\n}",
                "normal",
                "@scope {\n  fsa1-cell { font-weight: normal }\n}",
            ),
            (
                "@scope {\n  fsa1-cell { color : #3f0421 }\n}",
                "write `color: #3f0421`",
                "@scope {\n  fsa1-cell { color: #3f0421 }\n}",
            ),
            (
                "@scope {\n  fsa1-cell { color:#3f0421 }\n}",
                "write `color: #3f0421`",
                "@scope {\n  fsa1-cell { color: #3f0421 }\n}",
            ),
            (
                "@scope {\n  fsa1-cell { color:\t#3f0421 }\n}",
                "write `color: #3f0421`",
                "@scope {\n  fsa1-cell { color: #3f0421 }\n}",
            ),
            (
                "@scope {\n  fsa1-cell { color:   #3f0421 }\n}",
                "write `color: #3f0421`",
                "@scope {\n  fsa1-cell { color: #3f0421 }\n}",
            ),
            (
                "@scope {\n  fsa1-cell { border-top: 1px   solid   #3f0421 }\n}",
                "write `1px solid #3f0421`",
                "@scope {\n  fsa1-cell { border-top: 1px solid #3f0421 }\n}",
            ),
            (
                "@scope {\n  fsa1-cell { border-top: 1px\tsolid\t#3f0421 }\n}",
                "write `1px solid #3f0421`",
                "@scope {\n  fsa1-cell { border-top: 1px solid #3f0421 }\n}",
            ),
            (
                "@scope {\n  fsa1-cell { font-size: 11.0pt }\n}",
                "write `11pt`",
                "@scope {\n  fsa1-cell { font-size: 11pt }\n}",
            ),
            (
                "@scope {\n  fsa1-cell { font-size: +11pt }\n}",
                "write `11pt`",
                "@scope {\n  fsa1-cell { font-size: 11pt }\n}",
            ),
            (
                "@scope {\n  fsa1-cell { font-size: 011pt }\n}",
                "write `11pt`",
                "@scope {\n  fsa1-cell { font-size: 11pt }\n}",
            ),
            (
                "@scope {\n  fsa1-cell { font-size: 1.1e1pt }\n}",
                "write `11pt`",
                "@scope {\n  fsa1-cell { font-size: 11pt }\n}",
            ),
            (
                "@scope {\n  fsa1-cell { font-size: 11.50pt }\n}",
                "write `11.5pt`",
                "@scope {\n  fsa1-cell { font-size: 11.5pt }\n}",
            ),
            (
                "@scope {\n  fsa1-cell { width: 14.50ch }\n}",
                "write `14.5ch`",
                "@scope {\n  fsa1-cell { width: 14.5ch }\n}",
            ),
            // An axis may measure zero, so `-0ch` is in range and spells back to itself; left canonical it would be a second spelling of the one size zero.
            (
                "@scope {\n  fsa1-cell { width: -0ch }\n}",
                "write `0ch`",
                "@scope {\n  fsa1-cell { width: 0ch }\n}",
            ),
            (
                "@scope {\n  fsa1-cell { height: -0pt }\n}",
                "write `0pt`",
                "@scope {\n  fsa1-cell { height: 0pt }\n}",
            ),
            (
                "@scope {\n  fsa1-cell { height: 022.5pt }\n}",
                "write `22.5pt`",
                "@scope {\n  fsa1-cell { height: 22.5pt }\n}",
            ),
            (
                "@scope {\n  fsa1-cell { background: #ffffff }\n}",
                "background-color",
                "@scope {\n  fsa1-cell { background-color: #ffffff }\n}",
            ),
            (
                "@scope {\n  fsa1-cell { font-weight: bold; color: #3f0421 }\n}",
                "write `color` before `font-weight`",
                "@scope {\n  fsa1-cell { color: #3f0421; font-weight: bold }\n}",
            ),
            (
                "@scope {\n  fsa1-cell:first-child { color: #3f0421 }\n  fsa1-cell { color: #3f0421 }\n}",
                "write `fsa1-cell` before `fsa1-cell:first-child`",
                "@scope {\n  fsa1-cell { color: #3f0421 }\n  fsa1-cell:first-child { color: #3f0421 }\n}",
            ),
        ] {
            let d = refusal(block);
            assert_eq!(d.code, Code::NonCanonicalPresentation, "{block:?}");
            assert!(d.message.contains(want), "{block:?} -> {}", d.message);
            let refused = (d.code, d.message);
            let after = match parse(rewritten, REGION) {
                Ok(_) => Vec::new(),
                Err(diags) => diags.into_iter().map(|d| (d.code, d.message)).collect(),
            };
            assert!(
                !after.contains(&refused),
                "{block:?}: applying the rewrite returns the same refusal {refused:?}",
            );
        }
    }

    /// The two directions over one text: what [`spell_rules`] writes is what [`parse_rules`] reads,
    /// including which axis of extent 1 carries no selector.
    #[test]
    fn every_canonical_sidecar_spells_back_to_the_text_it_was_read_from() {
        for (shape, content) in [
            (REGION, "@scope {\n  fsa1-cell { font-size: 11pt }\n}"),
            (
                REGION,
                "@scope {\n  fsa1-cell { color: #3f0421; font-weight: bold }\n  \
                 fsa1-row:nth-child(2n) fsa1-cell { background-color: #ffe0b2 }\n  \
                 fsa1-row:first-child fsa1-cell { font-size: 14pt }\n  \
                 fsa1-row:nth-child(2) fsa1-cell { height: 22.5pt }\n  \
                 fsa1-row:last-child fsa1-cell { font-style: italic }\n  \
                 fsa1-cell:first-child { width: 14.5ch }\n  \
                 fsa1-cell:last-child { text-align: right }\n}",
            ),
            (
                Shape { rows: 1, cols: 3 },
                "@scope {\n  fsa1-cell { white-space: nowrap }\n  fsa1-cell:nth-child(2) { width: 4ch }\n}",
            ),
            (
                Shape { rows: 1, cols: 1 },
                "@scope {\n  fsa1-cell { border-bottom: 1px solid #3f0421; height: 15pt; width: 9ch }\n}",
            ),
        ] {
            let text = body(content);
            let root = root_of(shape);
            let parsed = parse_rules(&sidecar(shape), root, &text)
                .unwrap_or_else(|d| panic!("{text:?}: {:?}", d[0]));
            assert_eq!(
                spell_rules(root, &parsed),
                text,
                "{text:?} did not spell back to itself",
            );
        }
    }

    /// The FILENAME's root is what the selectors count in, and a sidecar states no extent of its own.
    #[test]
    fn a_selector_index_is_bounded_by_the_root_the_filename_names() {
        let root = Rect {
            min_col: 2,
            min_row: 4,
            max_col: 3,
            max_row: 8,
        };
        let ok = "  fsa1-row:last-child fsa1-cell { height: 20pt }\n  fsa1-cell:last-child { width: 9ch }\n";
        assert!(parse_rules("Sheet1/C5:D9.css", root, ok).is_ok());
        let d = parse_rules(
            "Sheet1/C5:D9.css",
            root,
            "  fsa1-cell:nth-child(3) { width: 9ch }\n",
        )
        .expect_err("column 3 is outside a two-column root")
        .remove(0);
        assert_eq!(d.code, Code::PresentationSelector);
        assert!(d.message.contains("column 3 is outside"), "{}", d.message);
    }

    /// The retired prelude is not silently re-admitted: inside a sidecar it is an at-rule like any
    /// other, refused where it stands rather than read as a second scoping root.
    #[test]
    fn a_retired_prelude_inside_a_sidecar_is_refused_as_the_at_rule_it_is() {
        let d = parse_rules(
            &sidecar(REGION),
            root_of(REGION),
            "@scope (A1:C3) {\n  fsa1-cell { color: #3f0421 }\n}\n",
        )
        .expect_err("a prelude has no place in a sidecar")
        .remove(0);
        assert_eq!(d.code, Code::PresentationSyntax);
        assert!(d.message.contains("at-rule"), "{}", d.message);
    }

    #[test]
    fn a_rewrite_chain_terminates() {
        let chain = [
            "@scope {\n  fsa1-cell { background: #fff }\n}",
            "@scope {\n  fsa1-cell { background: #ffffff }\n}",
            "@scope {\n  fsa1-cell { background-color: #ffffff }\n}",
        ];
        let mut seen: Vec<(Code, String)> = Vec::new();
        let mut accepted = false;
        for body in chain {
            match parse(body, REGION) {
                Ok(_) => {
                    accepted = true;
                    break;
                }
                Err(mut diags) => {
                    let d = diags.remove(0);
                    let refused = (d.code, d.message);
                    assert!(!seen.contains(&refused), "the chain repeats {refused:?}");
                    seen.push(refused);
                }
            }
        }
        assert!(accepted, "the chain never reached an accepted body");
        assert!(seen.len() < 4, "the chain took {} steps", seen.len());
    }

    /// The names come from [`Declaration::property`], the one place they are spelled, so a property
    /// added later cannot reach the parser without this case covering it.
    fn every_supported_property() -> Vec<&'static str> {
        let color = Rgb { r: 0, g: 0, b: 0 };
        let border = Border {
            line: BorderLine::ThinSolid,
            color,
        };
        [
            Declaration::BackgroundColor(color),
            Declaration::Border {
                edge: Edge::Top,
                border,
            },
            Declaration::Border {
                edge: Edge::Bottom,
                border,
            },
            Declaration::Border {
                edge: Edge::Left,
                border,
            },
            Declaration::Border {
                edge: Edge::Right,
                border,
            },
            Declaration::Color(color),
            Declaration::FontFamily(String::new()),
            Declaration::FontSize(Points(1.0)),
            Declaration::FontStyle(FontStyle::Normal),
            Declaration::FontWeight(FontWeight::Normal),
            Declaration::Height(Points(1.0)),
            Declaration::TextAlign(TextAlign::Left),
            Declaration::TextDecoration(TextDecoration::None),
            Declaration::VerticalAlign(VerticalAlign::Top),
            Declaration::WhiteSpace(WhiteSpace::Normal),
            Declaration::Width(Chars(1.0)),
        ]
        .iter()
        .map(Declaration::property)
        .collect()
    }

    #[test]
    fn a_declaration_missing_either_half_is_a_syntax_refusal_naming_the_half_it_has() {
        for property in every_supported_property() {
            let block = format!("@scope {{\n  fsa1-cell {{ {property}: }}\n}}");
            let d = refusal(&block);
            assert_eq!(d.code, Code::PresentationSyntax, "{block:?}");
            assert!(matches!(d.loc, Loc::Body { .. }), "{:?}", d.loc);
            assert!(d.message.contains(property), "{block:?} -> {}", d.message);
        }
        for block in [
            "@scope {\n  fsa1-cell { color:   }\n}",
            "@scope {\n  fsa1-cell { color:\t}\n}",
            "@scope {\n  fsa1-cell { color: ; }\n}",
            "@scope {\n  fsa1-cell { : }\n}",
            "@scope {\n  fsa1-cell { : #fff }\n}",
            "@scope {\n  fsa1-cell { :#fff }\n}",
        ] {
            let d = refusal(block);
            assert_eq!(
                d.code,
                Code::PresentationSyntax,
                "{block:?} -> {}",
                d.message
            );
            assert!(matches!(d.loc, Loc::Body { .. }), "{:?}", d.loc);
        }
    }

    #[test]
    fn an_empty_half_never_masks_the_property_refusal_behind_it() {
        assert_eq!(
            refusal("@scope {\n  fsa1-cell { box-shadow: }\n}").code,
            Code::PresentationSyntax,
        );
        assert_eq!(
            refusal("@scope {\n  fsa1-cell { box-shadow: none }\n}").code,
            Code::PresentationProperty,
        );
    }

    #[test]
    fn a_rule_with_no_selector_is_a_selector_refusal() {
        assert_eq!(
            refusal("@scope {\n  { color: #3f0421 }\n}").code,
            Code::PresentationSelector,
        );
    }

    #[test]
    fn an_axis_of_extent_one_carries_no_selector_of_its_own() {
        let one_row = Shape { rows: 1, cols: 3 };
        let d = parse(
            "@scope {\n  fsa1-row:first-child fsa1-cell { color: #3f0421 }\n}",
            one_row,
        )
        .unwrap_err()
        .remove(0);
        assert_eq!(d.code, Code::NonCanonicalPresentation);
        assert!(d.message.contains("write `fsa1-cell`"), "{}", d.message);

        let d = parse(
            "@scope {\n  fsa1-row:first-child fsa1-cell:nth-child(2) { color: #3f0421 }\n}",
            one_row,
        )
        .unwrap_err()
        .remove(0);
        assert!(
            d.message.contains("write `fsa1-cell:nth-child(2)`"),
            "{}",
            d.message
        );

        let one_col = Shape { rows: 4, cols: 1 };
        let d = parse(
            "@scope {\n  fsa1-cell:first-child { color: #3f0421 }\n}",
            one_col,
        )
        .unwrap_err()
        .remove(0);
        assert!(d.message.contains("write `fsa1-cell`"), "{}", d.message);
    }

    #[test]
    fn a_repeated_selector_or_property_is_refused() {
        let d =
            refusal("@scope {\n  fsa1-cell { color: #3f0421 }\n  fsa1-cell { font-size: 11pt }\n}");
        assert_eq!(d.code, Code::PresentationSyntax);
        assert!(d.message.contains("twice"), "{}", d.message);

        let d = refusal("@scope {\n  fsa1-cell { color: #3f0421; color: #ffffff }\n}");
        assert_eq!(d.code, Code::PresentationSyntax);
        assert!(d.message.contains("twice"), "{}", d.message);
    }

    #[test]
    fn an_empty_rule_or_sidecar_is_refused() {
        for block in [
            "@scope {\n}",
            "@scope {\n  fsa1-cell { }\n}",
            "@scope {\n  fsa1-cell { ; color: #3f0421 }\n}",
            "@scope {\n  fsa1-cell { color: #3f0421; }\n}",
        ] {
            assert_eq!(refusal(block).code, Code::PresentationSyntax, "{block:?}");
        }
    }

    #[test]
    fn a_malformed_rule_is_refused_rather_than_partly_accepted() {
        for block in [
            "@scope {\n  fsa1-cell { color #3f0421 }\n}",
            "@scope {\n  color: #3f0421;\n}",
            "@scope {\n  fsa1-cell { color: #3f0421 } fsa1-cell\n}",
        ] {
            let d = refusal(block);
            assert_eq!(
                d.code,
                Code::PresentationSyntax,
                "{block:?} -> {}",
                d.message
            );
        }
    }

    #[test]
    fn every_fault_in_a_rule_is_reported_at_once() {
        let d = parse(
            "@scope {\n  fsa1-cell { color: red; font-size: 9px; box-shadow: none }\n}",
            REGION,
        )
        .unwrap_err();
        assert_eq!(d.len(), 3, "{d:?}");
    }

    #[test]
    fn a_font_size_outside_excels_range_is_refused_as_out_of_range() {
        for value in [
            "0pt", "-1pt", "0.5pt", "5e-324pt", "410pt", "1e300pt", "inf",
        ] {
            let block = format!("@scope {{\n  fsa1-cell {{ font-size: {value} }}\n}}");
            let d = refusal(&block);
            assert_eq!(d.code, Code::PresentationValue, "{value:?}");
            assert!(
                d.message.len() < 100,
                "{value:?} must earn an actionable message, got {}: {}",
                d.message.len(),
                d.message
            );
        }
        assert_eq!(
            rules("@scope {\n  fsa1-cell { font-size: 409pt }\n}")[0].declarations[0],
            Declaration::FontSize(Points(409.0))
        );
    }

    #[test]
    fn the_frame_around_a_declaration_is_not_the_declaration() {
        // The format's own canonical example column-aligns its selectors, so padding around `{`, `}` and `;` is frame rather than spelling; what is INSIDE a declaration is spelling.
        for block in [
            "@scope {\n  fsa1-cell{ color: #3f0421 }\n}",
            "@scope {\n  fsa1-cell   { color: #3f0421 }\n}",
            "@scope {\n  fsa1-cell {color: #3f0421}\n}",
            "@scope {\n  fsa1-cell { color: #3f0421;font-weight: bold }\n}",
            "@scope {\n  fsa1-cell {\n    color: #3f0421;\n    font-weight: bold\n  }\n}",
        ] {
            assert!(parse(block, REGION).is_ok(), "{block:?}");
        }
    }

    #[test]
    fn a_canonical_spelling_survives_its_own_round_trip() {
        for (value, want) in [("11pt", 11.0), ("11.5pt", 11.5), ("8pt", 8.0)] {
            let block = format!("@scope {{\n  fsa1-cell {{ font-size: {value} }}\n}}");
            let points = match &rules(&block)[0].declarations[0] {
                Declaration::FontSize(p) => *p,
                other => panic!("expected a font size, got {other:?}"),
            };
            assert_eq!(points, Points(want));
            assert_eq!(points.spell(), value, "spell is the accepted spelling");
        }
    }

    #[test]
    fn a_font_family_is_one_unquoted_name() {
        assert_eq!(
            rules("@scope {\n  fsa1-cell { font-family: Calibri }\n}")[0].declarations[0],
            Declaration::FontFamily("Calibri".to_string())
        );
        for value in ["\"Times New Roman\"", "Calibri, sans-serif", "Times  New"] {
            let block = format!("@scope {{\n  fsa1-cell {{ font-family: {value} }}\n}}");
            assert_eq!(refusal(&block).code, Code::PresentationValue, "{value:?}");
        }
    }

    /// The two legs of ONE rule, swept rather than sampled. [`Declaration::font_family`] is what a
    /// WRITER asks before emitting a face; [`parse_declaration`] is what `check` then asks of the text
    /// it wrote. Both read the same list of what a value may hold, so they must answer identically for
    /// every character — the assertion that fails the moment either leg grows a list of its own.
    #[test]
    fn the_write_leg_admits_exactly_the_family_names_the_read_leg_accepts() {
        for c in (b' '..=b'~').map(char::from).chain(['\t', '\n', 'é', '中']) {
            for name in [format!("My{c}Font"), format!("{c}Font"), format!("Font{c}")] {
                assert_eq!(
                    Declaration::font_family(&name),
                    parse_declaration(&format!("font-family: {name}")).ok(),
                    "{name:?}",
                );
            }
        }
    }

    #[test]
    fn a_refusal_points_at_the_line_the_fault_is_on() {
        let d = refusal("@scope {\n  fsa1-cell { color: #3f0421 }\n  th { color: #3f0421 }\n}");
        assert!(
            matches!(
                d.loc,
                Loc::Body {
                    line: 2,
                    col: 3,
                    ..
                }
            ),
            "{:?}",
            d.loc
        );
    }

    #[test]
    fn hostile_sidecars_never_panic() {
        for text in [
            "",
            "{{{{\n",
            "}}}}\n",
            "@scope\n",
            "@scope (\n",
            "  fsa1-cell { color: #\n",
            "  \u{1F600} { color: #3f0421 }\n",
            "  fsa1-cell:nth-child(99999999999) { color: #3f0421 }\n",
            "  fsa1-cell:nth-child(-1) { color: #3f0421 }\n",
            "  fsa1-cell { font-family: \u{4e2d}\u{6587} }\n",
            &format!("  fsa1-cell {{ font-family: {} }}\n", "x".repeat(500)),
            &"  fsa1-cell { color: #3f0421 }\n".repeat(200),
        ] {
            let _ = parse_rules(&sidecar(REGION), root_of(REGION), text);
        }
    }
}
