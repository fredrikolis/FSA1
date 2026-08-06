// Concern: the @scope block — finding it, its selectors and rule order | Non-concern: what a declaration means, applying a style | IO: (content) -> (grid, Block); (Block) -> Presentation -> @scope text

use crate::declaration::{Declaration, parse_declaration, syntax};
use crate::diagnostic::{Code, Diagnostic, Loc};
use fsa1_ast::Shape;

const OPEN: &str = "@scope {";
const CLOSE: &str = "}";

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

/// One located presentation block: `body` holds the rules BETWEEN `@scope {` and its closing `}`, so
/// only [`split`] can build one and [`parse_block`] never re-finds delimiters it would have to trust.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Block<'a> {
    body: &'a str,
    line: u32,
}

impl Block<'_> {
    /// The 1-based file line the block opens on: where the grid stopped, for a caller diagnosing it.
    pub(crate) fn line(&self) -> u32 {
        self.line
    }
}

/// A range file is its grid, optionally followed by a presentation block. The block is found from the
/// END — the last non-empty line must be `}`, brace-matched backwards to a line that is exactly
/// `@scope {` — so a CELL whose text is `@scope {` can never truncate the grid.
pub(crate) fn split(content: &str) -> (&str, Option<Block<'_>>) {
    let mut lines: Vec<(usize, &str)> = Vec::new();
    let mut at = 0usize;
    for line in content.split('\n') {
        lines.push((at, line));
        at += line.len() + 1;
    }
    let Some(close) = lines.iter().rposition(|(_, l)| !l.is_empty()) else {
        return (content, None);
    };
    if lines[close].1 != CLOSE {
        return (content, None);
    }
    let Some(open) = match_open(&lines, close) else {
        return (content, None);
    };
    let (start, _) = lines[open];
    let grid = if start == 0 {
        ""
    } else {
        &content[..start - 1]
    };
    let block = Block {
        body: &content[start + OPEN.len() + 1..lines[close].0],
        line: (open + 1) as u32,
    };
    (grid, Some(block))
}

/// Returns the line whose `{` closes the outermost brace, and ONLY when that line is exactly
/// `@scope {` — a grid holding stray braces therefore matches nothing and stays whole.
fn match_open(lines: &[(usize, &str)], close: usize) -> Option<usize> {
    let mut depth = 0i32;
    for (idx, (_, line)) in lines[..=close].iter().enumerate().rev() {
        for ch in line.chars().rev() {
            match ch {
                '}' => depth += 1,
                '{' => {
                    depth -= 1;
                    if depth == 0 {
                        return (*line == OPEN).then_some(idx);
                    }
                }
                _ => {}
            }
        }
    }
    None
}

/// `shape` is the region the block styles: every selector is region-relative, so which index is
/// `:last-child` — and whether an axis carries a selector of its own at all — depends on it.
pub(crate) fn parse_block(
    file: &str,
    block: &Block<'_>,
    shape: Shape,
) -> Result<Presentation, Vec<Diagnostic>> {
    let mut cur = Cursor::new(block.body, block.line + 1);
    let mut placed: Vec<(u32, u32, Rule)> = Vec::new();
    let mut diags: Vec<Diagnostic> = Vec::new();

    loop {
        cur.skip_ws();
        if cur.peek().is_none() {
            break;
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
            break;
        }
        cur.bump();
        let target = resolve_target(file, selector, line, col, shape, &mut diags);
        let faults_before = diags.len();
        let declarations = parse_declarations(file, &mut cur, target, shape, &mut diags);
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

    check_rule_order(file, &placed, shape, &mut diags);
    if placed.is_empty() && diags.is_empty() {
        diags.push(located(
            file,
            block.line,
            1,
            Code::PresentationSyntax,
            "an empty presentation block is not written; delete it".to_string(),
        ));
    }
    if diags.is_empty() {
        Ok(Presentation {
            rules: placed.into_iter().map(|(_, _, r)| r).collect(),
        })
    } else {
        Err(diags)
    }
}

/// [`parse_block`] read backward: the whole `@scope` block a presentation is written as, over the
/// region `shape` — which decides both which pseudo-class an index takes and which axis carries a
/// selector at all. The rules must already ascend by [`Target`] with each rule's declarations
/// alphabetical, the one order this parses back from; a writer holding no rule writes NO block.
pub fn spell_block(presentation: &Presentation, shape: Shape) -> String {
    let rules: Vec<String> = presentation
        .rules
        .iter()
        .map(|rule| {
            debug_assert!(
                !rule.declarations.is_empty()
                    && rule
                        .declarations
                        .windows(2)
                        .all(|w| w[0].property() < w[1].property()),
                "a written rule declares something, alphabetically, once each",
            );
            let spelled: Vec<String> = rule.declarations.iter().map(Declaration::spell).collect();
            format!(
                "  {} {{ {} }}",
                spell(rule.target, shape),
                spelled.join("; ")
            )
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
    format!("{OPEN}\n{}\n{CLOSE}", rules.join("\n"))
}

/// `None` once the selector has earned a refusal of its own, which is also what keeps a rule from
/// being reported both for its selector and for the emptiness that follows from it.
fn resolve_target(
    file: &str,
    selector: &str,
    line: u32,
    col: u32,
    shape: Shape,
    diags: &mut Vec<Diagnostic>,
) -> Option<Target> {
    match parse_selector(selector, shape) {
        Ok(target) => {
            let target = canonicalize(target, shape);
            let canonical = spell(target, shape);
            // Compared VERBATIM, never whitespace-folded: a tab or a line break between two compounds is a second spelling of one appearance exactly as `#FFF` is.
            if selector == canonical {
                return Some(target);
            }
            diags.push(located(
                file,
                line,
                col,
                Code::NonCanonicalPresentation,
                format!("non-canonical selector {selector:?}: write `{canonical}`"),
            ));
            None
        }
        Err((code, message)) => {
            diags.push(located(file, line, col, code, message));
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
        if a == b {
            diags.push(located(
                file,
                *line,
                *col,
                Code::PresentationSyntax,
                format!("`{b}` is declared twice in one rule; give it one declaration"),
            ));
        } else if b < a {
            diags.push(located(
                file,
                *line,
                *col,
                Code::NonCanonicalPresentation,
                format!("declarations are alphabetical: write `{b}` before `{a}`"),
            ));
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
        Target::All => "td".to_string(),
        Target::RowEvery { a, b } => format!("tr{} td", periodic(a, b)),
        Target::ColEvery { a, b } => format!("td{}", periodic(a, b)),
        Target::Row(r) => format!("tr{} td", pseudo(r, shape.rows)),
        Target::Col(c) => format!("td{}", pseudo(c, shape.cols)),
        Target::Cell { row, col } => format!(
            "tr{} td{}",
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
            "an at-rule has no place inside a presentation block: {text:?}"
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

/// `Ok(None)` is the bare `td`, which selects whatever its row part already narrowed to.
fn column_of(part: &str, whole: &str, cols: u32) -> Result<Option<Idx>, (Code, String)> {
    let Some(rest) = part.strip_prefix("td") else {
        return Err(unknown_selector(whole));
    };
    if rest.is_empty() {
        return Ok(None);
    }
    index_of(rest, whole, cols, "column").map(Some)
}

fn row_of(part: &str, whole: &str, rows: u32) -> Result<Idx, (Code, String)> {
    let Some(rest) = part.strip_prefix("tr") else {
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

/// `A` of 1 selects every line, which is `td`, and `A` of 0 selects one, which is a literal index —
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
/// being sized. `td` names the whole region and stays legal for both, which is what leaves a file one
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
            "`td` or `td:nth-child(k)`",
        ),
        Declaration::Height(_) => (
            matches!(target, Target::All | Target::Row(_)),
            "row",
            "`td` or `tr:nth-child(k) td`",
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
    use super::*;
    use crate::declaration::{
        BORDER_LINES, Border, BorderLine, Chars, Edge, FontStyle, FontWeight, Points, Rgb,
        TextAlign, TextDecoration, VerticalAlign, WhiteSpace,
    };

    const REGION: Shape = Shape { rows: 4, cols: 3 };

    fn block_of(content: &str) -> Block<'_> {
        let (_, block) = split(content);
        block.unwrap_or_else(|| panic!("{content:?} should carry a block"))
    }

    fn parse(content: &str, shape: Shape) -> Result<Presentation, Vec<Diagnostic>> {
        parse_block("A1:C4", &block_of(content), shape)
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
    fn a_file_with_no_trailing_brace_is_all_grid() {
        let content = "Rent\t1500\n@scope {\tx\ty\nSalaries\t1600";
        let (grid, block) = split(content);
        assert_eq!(grid, content);
        assert_eq!(block, None);
    }

    #[test]
    fn a_text_cell_spelling_the_open_line_never_truncates_the_grid() {
        // Anchored to the file's LAST line, so an interior `@scope {` cell is inert.
        let content = "@scope {\n1\n2";
        let (grid, block) = split(content);
        assert_eq!(grid, content);
        assert_eq!(block, None);
    }

    #[test]
    fn stray_braces_in_grid_text_match_nothing() {
        for content in ["a\nx { y\n}", "a\n}", "{\n}", "a\n}\n}"] {
            let (grid, block) = split(content);
            assert_eq!(grid, content, "{content:?} must stay whole");
            assert_eq!(block, None, "{content:?} must find no block");
        }
    }

    #[test]
    fn the_block_is_split_off_and_the_grid_keeps_its_line_numbers() {
        let content = "1\t2\n3\t4\n@scope {\n  td { color: #3f0421 }\n}";
        let (grid, block) = split(content);
        assert_eq!(grid, "1\t2\n3\t4");
        let block = block.expect("a block");
        assert_eq!(block.line, 3);
        assert_eq!(block.body, "  td { color: #3f0421 }\n");
    }

    #[test]
    fn blank_lines_after_the_closing_brace_do_not_hide_the_block() {
        let content = "1\n@scope {\n  td { color: #3f0421 }\n}\n\n";
        let (grid, block) = split(content);
        assert_eq!(grid, "1");
        assert_eq!(block.expect("a block").line, 2);
    }

    #[test]
    fn a_multi_line_rule_body_is_still_brace_matched_to_the_open() {
        let content = "1\n@scope {\n  td {\n    color: #3f0421\n  }\n}";
        let (grid, block) = split(content);
        assert_eq!(grid, "1");
        assert_eq!(block.expect("a block").line, 2);
        assert_eq!(rules(content)[0].target, Target::All);
    }

    #[test]
    fn every_selector_form_reads_to_its_region_relative_target() {
        assert_eq!(one_rule("td"), Target::All);
        assert_eq!(one_rule("tr:first-child td"), Target::Row(1));
        assert_eq!(one_rule("tr:last-child td"), Target::Row(4));
        assert_eq!(one_rule("tr:nth-child(2) td"), Target::Row(2));
        assert_eq!(one_rule("td:first-child"), Target::Col(1));
        assert_eq!(one_rule("td:last-child"), Target::Col(3));
        assert_eq!(one_rule("td:nth-child(2)"), Target::Col(2));
        assert_eq!(
            one_rule("tr:nth-child(2) td:nth-child(2)"),
            Target::Cell { row: 2, col: 2 }
        );
        assert_eq!(
            one_rule("tr:first-child td:last-child"),
            Target::Cell { row: 1, col: 3 }
        );
        assert_eq!(
            one_rule("tr:nth-child(2n) td"),
            Target::RowEvery { a: 2, b: 0 }
        );
        assert_eq!(
            one_rule("tr:nth-child(2n+1) td"),
            Target::RowEvery { a: 2, b: 1 }
        );
        assert_eq!(
            one_rule("td:nth-child(2n+1)"),
            Target::ColEvery { a: 2, b: 1 }
        );
    }

    /// The lines are `b, b+a, b+2a, …`, so at offset 0 the FIRST is `a` and not 0. A period wider
    /// than the region is therefore admissible whenever it still reaches TWO lines of it.
    #[test]
    fn a_periodic_index_is_bounded_by_its_first_line_not_its_period() {
        assert_eq!(
            one_rule("tr:nth-child(3n+1) td"),
            Target::RowEvery { a: 3, b: 1 }
        );
        assert!(selector_refusal("tr:nth-child(7n) td").contains("first selects 7"));
    }

    /// A period reaching ONE line of the region picks out exactly what a literal index does, so the
    /// author is sent to the spelling that set already has rather than being given a second.
    #[test]
    fn a_periodic_index_reaching_one_line_is_that_literal_index() {
        let diag = refusal("@scope {\n  tr:nth-child(7n+3) td { color: #3f0421 }\n}");
        assert_eq!(diag.code, Code::NonCanonicalPresentation);
        assert!(
            diag.message.contains("write `tr:nth-child(3) td`"),
            "{}",
            diag.message
        );
    }

    /// One appearance, one spelling: each refusal below names a set some EXISTING form already
    /// spells, so admitting it would give that set a second way to be written.
    #[test]
    fn a_periodic_index_admits_no_synonym_of_a_form_that_exists() {
        assert!(selector_refusal("tr:nth-child(1n) td").contains("every 2 or more"));
        assert!(selector_refusal("tr:nth-child(0n+2) td").contains("every 2 or more"));
        assert!(selector_refusal("tr:nth-child(2n+2) td").contains("offset runs 0 to 1"));
        assert!(selector_refusal("tr:nth-child(odd) td").contains("ten region-relative selectors"));
        assert!(
            selector_refusal("tr:nth-child(even) td").contains("ten region-relative selectors")
        );
    }

    /// A size belongs to an AXIS, and the check that says so is an ALLOWLIST of the forms that name
    /// one — so a periodic form is refused by construction, not by a branch anyone must remember.
    #[test]
    fn a_periodic_selector_carries_no_size() {
        let height = "@scope {\n  tr:nth-child(2n) td { height: 22.5pt }\n}";
        assert_eq!(refusal(height).code, Code::PresentationProperty);
        let width = "@scope {\n  td:nth-child(2n+1) { width: 14.5ch }\n}";
        assert_eq!(refusal(width).code, Code::PresentationProperty);
        rules("@scope {\n  tr:nth-child(2) td { height: 22.5pt }\n}");
    }

    /// There is no periodic CELL target, so the two halves cannot both narrow.
    #[test]
    fn a_periodic_part_composes_with_nothing() {
        assert!(
            selector_refusal("tr:nth-child(2n) td:nth-child(3)")
                .contains("ten region-relative selectors")
        );
        assert!(
            selector_refusal("tr:nth-child(3) td:nth-child(2n)")
                .contains("ten region-relative selectors")
        );
    }

    /// An axis of extent 1 has nothing to alternate over, so a periodic index there selects exactly
    /// what `td` does — and the author is told which spelling that set already has.
    #[test]
    fn a_periodic_index_on_a_single_line_axis_is_refused_for_the_form_that_says_it() {
        let one_row = Shape { rows: 1, cols: 3 };
        let content = "@scope {\n  tr:nth-child(2n+1) td { color: #3f0421 }\n}";
        let diag = parse(content, one_row)
            .expect_err("a single-row region spells this `td`")
            .remove(0);
        assert_eq!(diag.code, Code::NonCanonicalPresentation);
        assert!(diag.message.contains("write `td`"), "{}", diag.message);
    }

    #[test]
    fn every_property_reads_to_its_typed_declaration() {
        let block = "@scope {\n  td { background-color: #ffffff; border-bottom: 1px solid #3f0421; \
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
        let content = "=Total!I3\t=Total!J3\t=Total!K3\n@scope {\n  td { border-bottom: 1px solid \
                       #3f0421; color: #3f0421; font-weight: bold; text-align: center }\n}";
        let parsed = parse(content, Shape { rows: 1, cols: 3 }).expect("parses");
        assert_eq!(parsed.rules.len(), 1);
        assert_eq!(parsed.rules[0].target, Target::All);
        assert_eq!(parsed.rules[0].declarations.len(), 4);
    }

    #[test]
    fn the_seven_border_lines_are_the_whole_set() {
        for (line, width, style) in BORDER_LINES {
            let block = format!("@scope {{\n  td {{ border-top: {width} {style} #3f0421 }}\n}}");
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
            refusal("@scope {\n  td { border-top: 2px dotted #3f0421 }\n}").code,
            Code::PresentationValue,
        );
    }

    #[test]
    fn a_width_only_border_is_refused_because_it_renders_nothing() {
        let d = refusal("@scope {\n  td { border-bottom: thin }\n}");
        assert_eq!(d.code, Code::PresentationValue);
        assert!(d.message.contains("all three"), "{}", d.message);
    }

    #[test]
    fn every_refused_construct_is_a_located_refusal() {
        for (block, code) in [
            (
                "@scope {\n  td { color: #3f0421 !important }\n}",
                Code::PresentationSyntax,
            ),
            (
                "@scope {\n  @media print { td { color: #3f0421 } }\n}",
                Code::PresentationSyntax,
            ),
            (
                "@scope {\n  @import url(x.css);\n}",
                Code::PresentationSyntax,
            ),
            (
                "@scope {\n  @layer base { td { color: #3f0421 } }\n}",
                Code::PresentationSyntax,
            ),
            (
                "@scope {\n  td { font-size: calc(11pt + 1pt) }\n}",
                Code::PresentationValue,
            ),
            (
                "@scope {\n  td { font-size: 11px }\n}",
                Code::PresentationValue,
            ),
            (
                "@scope {\n  td { font-size: 1em }\n}",
                Code::PresentationValue,
            ),
            (
                "@scope {\n  td { font-size: 1rem }\n}",
                Code::PresentationValue,
            ),
            (
                "@scope {\n  td { font-size: 120% }\n}",
                Code::PresentationValue,
            ),
            (
                "@scope {\n  td { color: currentcolor }\n}",
                Code::PresentationValue,
            ),
            (
                "@scope {\n  td { background-color: linear-gradient(red, blue) }\n}",
                Code::PresentationValue,
            ),
            (
                "@scope {\n  td { box-shadow: 0 0 2px #3f0421 }\n}",
                Code::PresentationProperty,
            ),
            (
                "@scope {\n  td { text-shadow: 0 0 2px #3f0421 }\n}",
                Code::PresentationProperty,
            ),
            (
                "@scope {\n  td { transition: color 1s }\n}",
                Code::PresentationProperty,
            ),
            (
                "@scope {\n  td::before { color: #3f0421 }\n}",
                Code::PresentationSelector,
            ),
            (
                "@scope {\n  td::after { color: #3f0421 }\n}",
                Code::PresentationSelector,
            ),
            (
                "@scope {\n  td:nth-col(2) { color: #3f0421 }\n}",
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
                "@scope {\n  td:nth-child(9) { color: #3f0421 }\n}",
                Code::PresentationSelector,
            ),
            (
                "@scope {\n  td { color: #3f0421 font-weight: bold }\n}",
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
    /// `td` names the whole region and stays legal for both — the only way a one-column file, which
    /// can spell no column selector at all, sizes its column.
    #[test]
    fn a_size_is_refused_on_a_selector_that_names_no_such_axis() {
        for (block, want) in [
            (
                "@scope {\n  tr:nth-child(2) td:nth-child(2) { width: 14.5ch }\n}",
                "no column",
            ),
            (
                "@scope {\n  tr:nth-child(2) td:nth-child(2) { height: 22.5pt }\n}",
                "no row",
            ),
            (
                "@scope {\n  tr:nth-child(2) td { width: 14.5ch }\n}",
                "no column",
            ),
            (
                "@scope {\n  td:nth-child(2) { height: 22.5pt }\n}",
                "no row",
            ),
        ] {
            let d = refusal(block);
            assert_eq!(d.code, Code::PresentationProperty, "{block:?}");
            assert!(matches!(d.loc, Loc::Body { .. }), "{block:?}: {:?}", d.loc);
            assert!(d.message.contains(want), "{block:?} -> {}", d.message);
        }
        for block in [
            "@scope {\n  td { height: 22.5pt; width: 14.5ch }\n}",
            "@scope {\n  tr:nth-child(2) td { height: 22.5pt }\n}",
            "@scope {\n  td:nth-child(2) { width: 14.5ch }\n}",
        ] {
            assert!(parse(block, REGION).is_ok(), "{block:?}");
        }
        let one_col = Shape { rows: 4, cols: 1 };
        assert!(parse("@scope {\n  td { width: 14.5ch }\n}", one_col).is_ok());
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
            let block = format!("@scope {{\n  td {{ {value} }}\n}}");
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
            rules("@scope {\n  td { width: 0ch }\n}")[0].declarations[0],
            Declaration::Width(Chars(0.0)),
        );
    }

    #[test]
    fn a_missing_separator_names_the_separator_rather_than_the_value() {
        let d = refusal("@scope {\n  td { color: #3f0421 font-weight: bold }\n}");
        assert!(d.message.contains("separated by `;`"), "{}", d.message);
    }

    /// The third column is the block with the named rewrite APPLIED, supplied rather than scraped out
    /// of the message: a rewrite targets a whole declaration, a value alone, a selector, or an
    /// ordering, so there is no one span to substitute into and no mechanical way to derive it today.
    #[test]
    fn every_non_canonical_spelling_carries_a_rewrite_that_retires_it() {
        for (block, want, rewritten) in [
            (
                "@scope {\n  td:nth-child(1) { color: #3f0421 }\n}",
                "td:first-child",
                "@scope {\n  td:first-child { color: #3f0421 }\n}",
            ),
            (
                "@scope {\n  td:nth-child(3) { color: #3f0421 }\n}",
                "td:last-child",
                "@scope {\n  td:last-child { color: #3f0421 }\n}",
            ),
            (
                "@scope {\n  tr:nth-child(1) td { color: #3f0421 }\n}",
                "tr:first-child td",
                "@scope {\n  tr:first-child td { color: #3f0421 }\n}",
            ),
            (
                "@scope {\n  tr:nth-child(4) td { color: #3f0421 }\n}",
                "tr:last-child td",
                "@scope {\n  tr:last-child td { color: #3f0421 }\n}",
            ),
            (
                "@scope {\n  tr:nth-child(1) td:nth-child(1) { color: #3f0421 }\n}",
                "tr:first-child td:first-child",
                "@scope {\n  tr:first-child td:first-child { color: #3f0421 }\n}",
            ),
            (
                "@scope {\n  tr:first-child\ttd { color: #3f0421 }\n}",
                "tr:first-child td",
                "@scope {\n  tr:first-child td { color: #3f0421 }\n}",
            ),
            (
                "@scope {\n  tr:first-child  td { color: #3f0421 }\n}",
                "tr:first-child td",
                "@scope {\n  tr:first-child td { color: #3f0421 }\n}",
            ),
            (
                "@scope {\n  tr:first-child\n  td { color: #3f0421 }\n}",
                "tr:first-child td",
                "@scope {\n  tr:first-child td { color: #3f0421 }\n}",
            ),
            (
                "@scope {\n  td { color: #3F0421 }\n}",
                "#3f0421",
                "@scope {\n  td { color: #3f0421 }\n}",
            ),
            (
                "@scope {\n  td { color: #fff }\n}",
                "#ffffff",
                "@scope {\n  td { color: #ffffff }\n}",
            ),
            (
                "@scope {\n  td { font-weight: 700 }\n}",
                "bold",
                "@scope {\n  td { font-weight: bold }\n}",
            ),
            (
                "@scope {\n  td { font-weight: 400 }\n}",
                "normal",
                "@scope {\n  td { font-weight: normal }\n}",
            ),
            (
                "@scope {\n  td { color : #3f0421 }\n}",
                "write `color: #3f0421`",
                "@scope {\n  td { color: #3f0421 }\n}",
            ),
            (
                "@scope {\n  td { color:#3f0421 }\n}",
                "write `color: #3f0421`",
                "@scope {\n  td { color: #3f0421 }\n}",
            ),
            (
                "@scope {\n  td { color:\t#3f0421 }\n}",
                "write `color: #3f0421`",
                "@scope {\n  td { color: #3f0421 }\n}",
            ),
            (
                "@scope {\n  td { color:   #3f0421 }\n}",
                "write `color: #3f0421`",
                "@scope {\n  td { color: #3f0421 }\n}",
            ),
            (
                "@scope {\n  td { border-top: 1px   solid   #3f0421 }\n}",
                "write `1px solid #3f0421`",
                "@scope {\n  td { border-top: 1px solid #3f0421 }\n}",
            ),
            (
                "@scope {\n  td { border-top: 1px\tsolid\t#3f0421 }\n}",
                "write `1px solid #3f0421`",
                "@scope {\n  td { border-top: 1px solid #3f0421 }\n}",
            ),
            (
                "@scope {\n  td { font-size: 11.0pt }\n}",
                "write `11pt`",
                "@scope {\n  td { font-size: 11pt }\n}",
            ),
            (
                "@scope {\n  td { font-size: +11pt }\n}",
                "write `11pt`",
                "@scope {\n  td { font-size: 11pt }\n}",
            ),
            (
                "@scope {\n  td { font-size: 011pt }\n}",
                "write `11pt`",
                "@scope {\n  td { font-size: 11pt }\n}",
            ),
            (
                "@scope {\n  td { font-size: 1.1e1pt }\n}",
                "write `11pt`",
                "@scope {\n  td { font-size: 11pt }\n}",
            ),
            (
                "@scope {\n  td { font-size: 11.50pt }\n}",
                "write `11.5pt`",
                "@scope {\n  td { font-size: 11.5pt }\n}",
            ),
            (
                "@scope {\n  td { width: 14.50ch }\n}",
                "write `14.5ch`",
                "@scope {\n  td { width: 14.5ch }\n}",
            ),
            // An axis may measure zero, so `-0ch` is in range and spells back to itself; left canonical it would be a second zero that `geometry-conflict` reads as disagreeing with the first.
            (
                "@scope {\n  td { width: -0ch }\n}",
                "write `0ch`",
                "@scope {\n  td { width: 0ch }\n}",
            ),
            (
                "@scope {\n  td { height: -0pt }\n}",
                "write `0pt`",
                "@scope {\n  td { height: 0pt }\n}",
            ),
            (
                "@scope {\n  td { height: 022.5pt }\n}",
                "write `22.5pt`",
                "@scope {\n  td { height: 22.5pt }\n}",
            ),
            (
                "@scope {\n  td { background: #ffffff }\n}",
                "background-color",
                "@scope {\n  td { background-color: #ffffff }\n}",
            ),
            (
                "@scope {\n  td { font-weight: bold; color: #3f0421 }\n}",
                "write `color` before `font-weight`",
                "@scope {\n  td { color: #3f0421; font-weight: bold }\n}",
            ),
            (
                "@scope {\n  td:first-child { color: #3f0421 }\n  td { color: #3f0421 }\n}",
                "write `td` before `td:first-child`",
                "@scope {\n  td { color: #3f0421 }\n  td:first-child { color: #3f0421 }\n}",
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

    /// The two directions over one text: what [`spell_block`] writes is what [`parse_block`] reads,
    /// including which axis of extent 1 carries no selector.
    #[test]
    fn every_canonical_block_spells_back_to_the_text_it_was_read_from() {
        for (shape, content) in [
            (REGION, "1\n@scope {\n  td { font-size: 11pt }\n}"),
            (
                REGION,
                "1\n@scope {\n  td { color: #3f0421; font-weight: bold }\n  \
                 tr:first-child td { font-size: 14pt }\n  tr:nth-child(2) td { height: 22.5pt }\n  \
                 tr:last-child td { font-style: italic }\n  td:first-child { width: 14.5ch }\n  \
                 td:last-child { text-align: right }\n  \
                 tr:nth-child(2) td:nth-child(2) { background-color: #ffe0b2 }\n}",
            ),
            (
                Shape { rows: 1, cols: 3 },
                "1\n@scope {\n  td { white-space: nowrap }\n  td:nth-child(2) { width: 4ch }\n}",
            ),
            (
                Shape { rows: 1, cols: 1 },
                "1\n@scope {\n  td { border-bottom: 1px solid #3f0421; height: 15pt; width: 9ch }\n}",
            ),
        ] {
            let parsed =
                parse(content, shape).unwrap_or_else(|d| panic!("{content:?}: {:?}", d[0]));
            let (grid, _) = split(content);
            assert_eq!(
                format!("{grid}\n{}", spell_block(&parsed, shape)),
                content,
                "{content:?} did not spell back to itself",
            );
        }
    }

    #[test]
    fn a_rewrite_chain_terminates() {
        let chain = [
            "@scope {\n  td { background: #fff }\n}",
            "@scope {\n  td { background: #ffffff }\n}",
            "@scope {\n  td { background-color: #ffffff }\n}",
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
            let block = format!("@scope {{\n  td {{ {property}: }}\n}}");
            let d = refusal(&block);
            assert_eq!(d.code, Code::PresentationSyntax, "{block:?}");
            assert!(matches!(d.loc, Loc::Body { .. }), "{:?}", d.loc);
            assert!(d.message.contains(property), "{block:?} -> {}", d.message);
        }
        for block in [
            "@scope {\n  td { color:   }\n}",
            "@scope {\n  td { color:\t}\n}",
            "@scope {\n  td { color: ; }\n}",
            "@scope {\n  td { : }\n}",
            "@scope {\n  td { : #fff }\n}",
            "@scope {\n  td { :#fff }\n}",
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
            refusal("@scope {\n  td { box-shadow: }\n}").code,
            Code::PresentationSyntax,
        );
        assert_eq!(
            refusal("@scope {\n  td { box-shadow: none }\n}").code,
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
            "@scope {\n  tr:first-child td { color: #3f0421 }\n}",
            one_row,
        )
        .unwrap_err()
        .remove(0);
        assert_eq!(d.code, Code::NonCanonicalPresentation);
        assert!(d.message.contains("write `td`"), "{}", d.message);

        let d = parse(
            "@scope {\n  tr:first-child td:nth-child(2) { color: #3f0421 }\n}",
            one_row,
        )
        .unwrap_err()
        .remove(0);
        assert!(
            d.message.contains("write `td:nth-child(2)`"),
            "{}",
            d.message
        );

        let one_col = Shape { rows: 4, cols: 1 };
        let d = parse("@scope {\n  td:first-child { color: #3f0421 }\n}", one_col)
            .unwrap_err()
            .remove(0);
        assert!(d.message.contains("write `td`"), "{}", d.message);
    }

    #[test]
    fn a_repeated_selector_or_property_is_refused() {
        let d = refusal("@scope {\n  td { color: #3f0421 }\n  td { font-size: 11pt }\n}");
        assert_eq!(d.code, Code::PresentationSyntax);
        assert!(d.message.contains("twice"), "{}", d.message);

        let d = refusal("@scope {\n  td { color: #3f0421; color: #ffffff }\n}");
        assert_eq!(d.code, Code::PresentationSyntax);
        assert!(d.message.contains("twice"), "{}", d.message);
    }

    #[test]
    fn an_empty_rule_or_block_is_refused() {
        for block in [
            "@scope {\n}",
            "@scope {\n  td { }\n}",
            "@scope {\n  td { ; color: #3f0421 }\n}",
            "@scope {\n  td { color: #3f0421; }\n}",
        ] {
            assert_eq!(refusal(block).code, Code::PresentationSyntax, "{block:?}");
        }
    }

    #[test]
    fn a_malformed_block_is_refused_rather_than_partly_accepted() {
        for block in [
            "@scope {\n  td { color #3f0421 }\n}",
            "@scope {\n  color: #3f0421;\n}",
            "@scope {\n  td { color: #3f0421 } td\n}",
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
    fn a_tail_whose_braces_do_not_balance_is_no_block_at_all() {
        // The two facts must AGREE: an unbalanced tail matches nothing, so the file is judged as a grid and its refusal comes from there.
        for content in [
            "@scope {\n  td color: #3f0421 }\n}",
            "@scope {\n  td { color: #3f0421\n}",
        ] {
            assert_eq!(split(content).1, None, "{content:?}");
        }
    }

    #[test]
    fn every_fault_in_a_rule_is_reported_at_once() {
        let d = parse(
            "@scope {\n  td { color: red; font-size: 9px; box-shadow: none }\n}",
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
            let block = format!("@scope {{\n  td {{ font-size: {value} }}\n}}");
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
            rules("@scope {\n  td { font-size: 409pt }\n}")[0].declarations[0],
            Declaration::FontSize(Points(409.0))
        );
    }

    #[test]
    fn the_frame_around_a_declaration_is_not_the_declaration() {
        // The format's own canonical example column-aligns its selectors, so padding around `{`, `}` and `;` is frame rather than spelling; what is INSIDE a declaration is spelling.
        for block in [
            "@scope {\n  td{ color: #3f0421 }\n}",
            "@scope {\n  td   { color: #3f0421 }\n}",
            "@scope {\n  td {color: #3f0421}\n}",
            "@scope {\n  td { color: #3f0421;font-weight: bold }\n}",
            "@scope {\n  td {\n    color: #3f0421;\n    font-weight: bold\n  }\n}",
        ] {
            assert!(parse(block, REGION).is_ok(), "{block:?}");
        }
    }

    #[test]
    fn a_canonical_spelling_survives_its_own_round_trip() {
        for (value, want) in [("11pt", 11.0), ("11.5pt", 11.5), ("8pt", 8.0)] {
            let block = format!("@scope {{\n  td {{ font-size: {value} }}\n}}");
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
            rules("@scope {\n  td { font-family: Calibri }\n}")[0].declarations[0],
            Declaration::FontFamily("Calibri".to_string())
        );
        for value in ["\"Times New Roman\"", "Calibri, sans-serif", "Times  New"] {
            let block = format!("@scope {{\n  td {{ font-family: {value} }}\n}}");
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
        let d = refusal("@scope {\n  td { color: #3f0421 }\n  th { color: #3f0421 }\n}");
        assert!(
            matches!(
                d.loc,
                Loc::Body {
                    line: 3,
                    col: 3,
                    ..
                }
            ),
            "{:?}",
            d.loc
        );
    }

    #[test]
    fn hostile_block_bodies_never_panic() {
        for body in [
            "",
            "{{{{\n",
            "}}}}\n",
            "  td { color: #\n",
            "  td { color: #3f0421\n",
            "  \u{1F600} { color: #3f0421 }\n",
            "  td:nth-child(99999999999) { color: #3f0421 }\n",
            "  td:nth-child(-1) { color: #3f0421 }\n",
            "  td { font-family: \u{4e2d}\u{6587} }\n",
            &format!("  td {{ font-family: {} }}\n", "x".repeat(500)),
            &"  td { color: #3f0421 }\n".repeat(200),
        ] {
            let block = Block { body, line: 1 };
            let _ = parse_block("A1:C4", &block, REGION);
        }
    }
}
