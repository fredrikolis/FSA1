// Concern: a sidecar's rules — selectors, declarations, and which no carrier takes | Non-concern: a declaration's meaning, applying a style, the filename | IO: (root, text) <-> Rules + uncarried

use crate::declaration::{DeclFault, Declaration, parse_declaration, syntax};
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
    parse_rules_located(file, root, content).map(|read| rules_of(read.rules))
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

/// A sidecar READ once: every rule the author WROTE — emptied by typing or not — and every
/// declaration the model does not carry, already stated as findings. Both come off the ONE pass over
/// the text, so the carrier that must refuse what it cannot take never re-reads the author's bytes to
/// learn what that is, and a check judging a rule's PLACE sees the rule whatever typing left in it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SidecarRead {
    pub rules: LocatedRules,
    pub uncarried: Vec<Diagnostic>,
}

/// The rules the MODEL carries, in written order: a rule typing emptied is dropped here, there being
/// nothing in it left to resolve. Every check judging a rule's FRAME runs over the located rules,
/// before this, so dropping one from the model never exempts it from what its place already earned.
pub fn rules_of(located: LocatedRules) -> Presentation {
    Presentation {
        rules: located
            .into_iter()
            .filter(|l| !l.rule.declarations.is_empty())
            .map(|l| l.rule)
            .collect(),
    }
}

/// [`parse_rules`] keeping each rule's position, and every declaration the model does not carry
/// beside them. A caller refusing a rule for something outside the sidecar — what its ROOT is, what
/// the tab around it holds, or what its own carrier cannot take — locates that refusal from here
/// rather than reading the text again.
pub fn parse_rules_located(
    file: &str,
    root: Rect,
    content: &str,
) -> Result<SidecarRead, Vec<Diagnostic>> {
    let shape = shape_of(root);
    let mut cur = Cursor::new(content, 1);
    let mut diags: Vec<Diagnostic> = Vec::new();
    let mut uncarried: Vec<Uncarried> = Vec::new();
    let placed = read_rules(file, &mut cur, shape, &mut diags, &mut uncarried);
    if placed.is_empty() && diags.is_empty() && uncarried.is_empty() {
        diags.push(located(
            file,
            1,
            1,
            Code::PresentationSyntax,
            "a sidecar declaring nothing is not written; delete it".to_string(),
        ));
    }
    if diags.is_empty() {
        // Every rule reaches the CALLER, located and in written order: what a rule's place earns is not the model's to grant, and `rules_of` is the one point the emptied ones stop.
        Ok(SidecarRead {
            rules: placed
                .into_iter()
                .map(|(line, col, rule)| LocatedRule { rule, line, col })
                .collect(),
            uncarried: uncarried.into_iter().map(|u| u.finding(file)).collect(),
        })
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
    uncarried: &mut Vec<Uncarried>,
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
        if selector.is_empty() {
            // Nothing was written where a selector goes, which is the FRAME and not a selector FSA1 cannot resolve. The body is still consumed through its `}` — the cursor must advance, or the next read is this one.
            diags.push(located(
                file,
                line,
                col,
                Code::PresentationSyntax,
                "a rule states a selector before its `{`".to_string(),
            ));
            parse_declarations(file, cur, None, shape, diags, uncarried);
            continue;
        }
        let read = resolve_target(file, selector, line, col, shape, diags);
        let named = match read {
            TargetRead::Named(target) => Some(target),
            _ => None,
        };
        let faults_before = (diags.len(), uncarried.len());
        let declarations = parse_declarations(file, cur, named, shape, diags, uncarried);
        let target = match read {
            TargetRead::Named(target) => target,
            TargetRead::Refused => continue,
            // ONE finding per RULE: the declarations under a selector that resolves to nothing were read before the target settled, and what the carrier drops is the rule, not each line of it.
            TargetRead::Unresolved(why) => {
                uncarried.truncate(faults_before.1);
                uncarried.push(Uncarried {
                    line,
                    col,
                    text: selector.to_string(),
                    why,
                });
                continue;
            }
        };
        if declarations.is_empty() && faults_before == (diags.len(), uncarried.len()) {
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
            continue;
        }
        // A rule whose declarations the model cannot read is still a rule the author WROTE: its selector and its position are the frame, and the frame is judged whether or not typing left anything behind. `rules_of` is what drops the emptied rule from the model afterwards.
        placed.push((
            line,
            col,
            Rule {
                target,
                declarations,
            },
        ));
    }
}

/// [`parse_rules`] read backward, over the order the [`Presentation`] holds — WRITTEN order, which
/// is the order the rules cascade in, so a sidecar read and spelled again is its own text. Each
/// rule's declarations must already be alphabetical, the one order those parse back from, and a
/// root holding no rule writes NO sidecar; `root` supplies the extent an index is spelled against.
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
        !rules.is_empty(),
        "a root holding no rule writes no sidecar",
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

/// What a selector's text yields where it yields no [`Target`]: a REFUSAL, the sidecar's own root or
/// frame being wrong, or a selector FSA1 simply resolves to nothing, which the whole rule is
/// uncarried for.
enum SelectorFault {
    Refused(Code, String),
    Unresolved(String),
}

/// What one rule's selector settled to. `Refused` has already stated its own diagnostic; `Unresolved`
/// carries the reason the rule is named to the carrier that drops it.
enum TargetRead {
    Named(Target),
    Unresolved(String),
    Refused,
}

/// Anything but `Named` keeps a rule from being reported both for its selector and for the emptiness
/// that follows from it.
fn resolve_target(
    file: &str,
    selector: &str,
    line: u32,
    col_at: u32,
    shape: Shape,
    diags: &mut Vec<Diagnostic>,
) -> TargetRead {
    match parse_selector(selector, shape) {
        // PRES1 over the target the selector NAMES: a cell selector addresses a coordinate on every root, a single line of one axis included, so no FSA1 carrier takes it — but a coordinate earns a better reason than the generic one.
        Ok(Target::Cell { row, col }) => {
            let at = crate::Rect::cell(col - 1, row - 1).label();
            TargetRead::Unresolved(format!(
                "{selector:?} addresses a coordinate; a selector states a region's SHAPE. \
                 State that cell in its own sidecar instead: the root's {at} as <cell>.css"
            ))
        }
        Ok(target) => TargetRead::Named(target),
        Err(SelectorFault::Refused(code, message)) => {
            diags.push(located(file, line, col_at, code, message));
            TargetRead::Refused
        }
        Err(SelectorFault::Unresolved(why)) => TargetRead::Unresolved(why),
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
    uncarried: &mut Vec<Uncarried>,
) -> Vec<Declaration> {
    let mut parsed: Vec<Declaration> = Vec::new();
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
            match parse_declaration(text) {
                Ok(d) => match axis_fault(&d, target, shape) {
                    Some((code, message)) => diags.push(located(file, line, col, code, message)),
                    None => parsed.push(d),
                },
                Err(DeclFault::Frame(code, message)) => {
                    diags.push(located(file, line, col, code, message));
                }
                Err(DeclFault::Uncarried(why)) => uncarried.push(Uncarried {
                    line,
                    col,
                    text: text.to_string(),
                    why,
                }),
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
    parsed
}

/// ONE thing the typed model cannot read, and where it was written — a declaration, or a whole rule
/// whose selector resolves to no cell or axis. It leaves the [`Rule`], the page still paints the
/// author's own bytes (PRES2), and [`Uncarried::finding`] is what names it to the one carrier that
/// cannot take it.
struct Uncarried {
    line: u32,
    col: u32,
    text: String,
    why: String,
}

impl Uncarried {
    /// The declaration named to the carrier that cannot take it. The reason is `why` ALONE — the
    /// carrier itself is [`Code::XlsxNotCarried`]'s, stated once in the registry and printed beside
    /// every one of these.
    fn finding(self, file: &str) -> Diagnostic {
        located(
            file,
            self.line,
            self.col,
            Code::XlsxNotCarried,
            format!("{}: {}", self.text, self.why),
        )
    }
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

fn parse_selector(text: &str, shape: Shape) -> Result<Target, SelectorFault> {
    if text.starts_with('@') {
        let (code, message) = syntax(&format!(
            "an at-rule has no place inside a sidecar: {text:?}"
        ));
        return Err(SelectorFault::Refused(code, message));
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
            _ => Err(unresolved()),
        },
        _ => Err(unresolved()),
    }
}

/// One axis index: a literal line, or every `a`th line offset by `b`.
#[derive(Clone, Copy)]
enum Idx {
    At(u32),
    Every { a: u32, b: u32 },
}

/// `Ok(None)` is the bare `fsa1-cell`, which selects whatever its row part already narrowed to.
fn column_of(part: &str, whole: &str, cols: u32) -> Result<Option<Idx>, SelectorFault> {
    let Some(rest) = part.strip_prefix("fsa1-cell") else {
        return Err(unresolved());
    };
    if rest.is_empty() {
        return Ok(None);
    }
    index_of(rest, whole, cols, "column").map(Some)
}

fn row_of(part: &str, whole: &str, rows: u32) -> Result<Idx, SelectorFault> {
    let Some(rest) = part.strip_prefix("fsa1-row") else {
        return Err(unresolved());
    };
    index_of(rest, whole, rows, "row")
}

fn index_of(pseudo: &str, whole: &str, extent: u32, axis: &str) -> Result<Idx, SelectorFault> {
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
        k.parse::<u32>().map_err(|_| unresolved())?
    } else {
        return Err(unresolved());
    };
    if index == 0 || index > extent {
        return Err(SelectorFault::Refused(
            Code::PresentationSelector,
            format!("{axis} {index} is outside the region's {extent}: {whole:?}"),
        ));
    }
    Ok(Idx::At(index))
}

/// Splits `An` or `An+B` into its two numbers, a bare `An` taking offset 0. Anything else — a
/// keyword like `odd`, a signed or spaced offset — splits to nothing and falls through to the
/// literal-index parse, which refuses it.
fn split_periodic(k: &str) -> Option<(&str, &str)> {
    let (a, rest) = k.split_once('n')?;
    match rest.strip_prefix('+') {
        Some(b) => Some((a, b)),
        None if rest.is_empty() => Some((a, "0")),
        None => None,
    }
}

/// `A` of 1 selects every line, which is `fsa1-cell`, and `A` of 0 selects one, which is a literal
/// index — each already has a spelling, so the model resolves neither synonym to a target, and an
/// offset at or past the period names the very lines a smaller one does. The FIRST line, though, is
/// counted in the ROOT, so a period reaching past it is that root's refusal.
fn periodic_of(
    a: &str,
    b: &str,
    whole: &str,
    extent: u32,
    axis: &str,
) -> Result<Idx, SelectorFault> {
    let a: u32 = a.parse().map_err(|_| unresolved())?;
    let b: u32 = b.parse().map_err(|_| unresolved())?;
    if a < 2 || b >= a {
        return Err(unresolved());
    }
    // Lines b, b+a, b+2a, … — but at offset 0 line 0 does not exist, so the first is a.
    let first = if b == 0 { a } else { b };
    if first > extent {
        return Err(SelectorFault::Refused(
            Code::PresentationSelector,
            format!(
                "a periodic {axis} first selects {first}, outside the region's {extent}: {whole:?}"
            ),
        ));
    }
    Ok(Idx::Every { a, b })
}

/// The ONE wording every selector FSA1 resolves to nothing is named to its carrier by: the rule is
/// uncarried whole, and which spelling missed is the author's own text beside it.
fn unresolved() -> SelectorFault {
    SelectorFault::Unresolved(
        "FSA1 resolves this selector to no cell or axis, so the whole rule is uncarried"
            .to_string(),
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

    /// `#3f0421`, the colour these cases declare, as the value a resolved style holds it as.
    const PLUM: Rgb = Rgb {
        r: 0x3f,
        g: 0x04,
        b: 0x21,
    };

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

    /// The declarations a case's rules leave uncarried, over a sidecar that LOADS: the assertion a
    /// value the model cannot read earns instead of a refusal.
    fn uncarried(content: &str) -> Vec<Diagnostic> {
        parse_rules_located(&sidecar(REGION), root_of(REGION), &body(content))
            .unwrap_or_else(|d| panic!("{content:?} should load: {:?}", d[0]))
            .uncarried
    }

    /// The one uncarried declaration a case states, as its message.
    fn one_uncarried(content: &str) -> String {
        let mut found = uncarried(content);
        assert_eq!(found.len(), 1, "{content:?} -> {found:?}");
        let d = found.remove(0);
        assert_eq!(d.code, Code::XlsxNotCarried, "{content:?}");
        assert!(
            matches!(d.loc, Loc::Body { .. }),
            "{content:?}: {:?}",
            d.loc
        );
        d.message
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
        // A cell selector reads to a coordinate, which no FSA1 carrier takes (PRES1); here only the shapes are read.
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

    /// A target is what its selector NAMES, so a period reaching one line of the region stays the
    /// period written — and reaches exactly the line a literal index would.
    #[test]
    fn a_periodic_index_reaching_one_line_reaches_that_line() {
        let p = parse(
            "@scope {\n  fsa1-row:nth-child(7n+3) fsa1-cell { color: #3f0421 }\n}",
            REGION,
        )
        .expect("a period naming one line is the period written");
        assert_eq!(p.rules[0].target, Target::RowEvery { a: 7, b: 3 });
        for row in 1..=REGION.rows {
            assert_eq!(
                crate::style::resolve(&p, row, 1).color.is_some(),
                row == 3,
                "row {row}"
            );
        }
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

    /// An axis of extent 1 has one line for a periodic index to reach, and the target is still the
    /// period the selector names — at the rank a row selector has, not `fsa1-cell`'s.
    #[test]
    fn a_periodic_index_on_a_single_line_axis_names_that_line() {
        let one_row = Shape { rows: 1, cols: 3 };
        let content = "@scope {\n  fsa1-row:nth-child(2n+1) fsa1-cell { color: #3f0421 }\n}";
        let p = parse(content, one_row).expect("a one-row root carries the period written");
        assert_eq!(p.rules[0].target, Target::RowEvery { a: 2, b: 1 });
        assert!(crate::style::resolve(&p, 1, 1).color.is_some());
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
        assert!(
            one_uncarried("@scope {\n  fsa1-cell { border-top: 2px dotted #3f0421 }\n}")
                .contains("no border edge is `2px dotted`"),
        );
    }

    #[test]
    fn a_width_only_border_is_uncarried_because_it_renders_nothing() {
        let message = one_uncarried("@scope {\n  fsa1-cell { border-bottom: thin }\n}");
        assert!(message.contains("all three"), "{message}");
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

    /// A selector FSA1 resolves to no cell or axis is not the sidecar's fault: it loads, its bytes
    /// reach the page (PRES2), and the ONE carrier that drops it — the `.xlsx` — is told once, about
    /// the RULE and not about each declaration under it.
    #[test]
    fn a_selector_the_model_resolves_to_nothing_leaves_the_whole_rule_uncarried() {
        for selector in [
            "fsa1-cell::before",
            "fsa1-cell::after",
            "fsa1-cell:nth-col(2)",
            "::column",
            "th",
            "fsa1-row:nth-child(1n) fsa1-cell",
            "fsa1-row:nth-child(0n+2) fsa1-cell",
            "fsa1-row:nth-child(2n+2) fsa1-cell",
            "fsa1-row:nth-child(odd) fsa1-cell",
            "fsa1-row:nth-child(even) fsa1-cell",
            // A periodic part composes with nothing, there being no periodic CELL target.
            "fsa1-row:nth-child(2n) fsa1-cell:nth-child(3)",
            "fsa1-row:nth-child(3) fsa1-cell:nth-child(2n)",
            "fsa1-row:first-child fsa1-cell:first-child",
        ] {
            let message =
                one_uncarried(&format!("@scope {{\n  {selector} {{ color: #3f0421 }}\n}}"));
            assert!(
                message.starts_with(&format!("{selector}: ")),
                "{selector:?} -> {message}",
            );
        }
        // A coordinate earns its own reason; every other unresolved selector shares the one wording.
        assert!(
            one_uncarried(
                "@scope {\n  fsa1-row:first-child fsa1-cell:first-child { color: #3f0421 }\n}"
            )
            .contains("addresses a coordinate"),
        );
        assert!(
            one_uncarried("@scope {\n  th { color: #3f0421 }\n}")
                .contains("resolves this selector to no cell or axis"),
        );
        // ONE finding per RULE: two declarations the model cannot read either, under a selector it cannot resolve, are still the one rule the carrier drops.
        assert_eq!(
            uncarried("@scope {\n  th { color: crimson; box-shadow: none }\n}").len(),
            1,
        );
    }

    /// The other half of the table above: a VALUE the typed model cannot read, and a PROPERTY it
    /// holds no slot for, are not the sidecar's faults at all. Each loads, each keeps its bytes on
    /// the page, and each is named once — to `pack`, the one carrier that cannot take it.
    #[test]
    fn every_declaration_the_model_cannot_read_is_uncarried_rather_than_refused() {
        for (block, want) in [
            (
                "@scope {\n  fsa1-cell { font-size: calc(11pt + 1pt) }\n}",
                "never computed",
            ),
            ("@scope {\n  fsa1-cell { font-size: 11px }\n}", "font size"),
            ("@scope {\n  fsa1-cell { font-size: 1em }\n}", "font size"),
            ("@scope {\n  fsa1-cell { font-size: 1rem }\n}", "font size"),
            ("@scope {\n  fsa1-cell { font-size: 120% }\n}", "font size"),
            (
                "@scope {\n  fsa1-cell { color: currentcolor }\n}",
                "is not a colour",
            ),
            (
                "@scope {\n  fsa1-cell { background-color: linear-gradient(red, blue) }\n}",
                "is not a colour",
            ),
            (
                "@scope {\n  fsa1-cell { box-shadow: 0 0 2px #3f0421 }\n}",
                "not a supported presentation property",
            ),
            (
                "@scope {\n  fsa1-cell { text-shadow: 0 0 2px #3f0421 }\n}",
                "not a supported presentation property",
            ),
            (
                "@scope {\n  fsa1-cell { transition: color 1s }\n}",
                "not a supported presentation property",
            ),
        ] {
            let message = one_uncarried(block);
            assert!(message.contains(want), "{block:?} -> {message}");
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
    fn an_axis_size_outside_its_own_unit_and_excels_own_range_is_uncarried() {
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
            let message = one_uncarried(&block);
            assert!(
                message.len() < 100,
                "{value:?} must earn an actionable message: {message}",
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

    #[test]
    fn every_spelling_of_one_target_reads_to_that_target() {
        for (block, want) in [
            (
                "@scope {\n  fsa1-cell:nth-child(1) { color: #3f0421 }\n}",
                Target::Col(1),
            ),
            (
                "@scope {\n  fsa1-cell:nth-child(3) { color: #3f0421 }\n}",
                Target::Col(3),
            ),
            (
                "@scope {\n  fsa1-row:nth-child(1) fsa1-cell { color: #3f0421 }\n}",
                Target::Row(1),
            ),
            (
                "@scope {\n  fsa1-row:nth-child(4) fsa1-cell { color: #3f0421 }\n}",
                Target::Row(4),
            ),
            (
                "@scope {\n  fsa1-row:first-child\tfsa1-cell { color: #3f0421 }\n}",
                Target::Row(1),
            ),
            (
                "@scope {\n  fsa1-row:first-child  fsa1-cell { color: #3f0421 }\n}",
                Target::Row(1),
            ),
            (
                "@scope {\n  fsa1-row:first-child\n  fsa1-cell { color: #3f0421 }\n}",
                Target::Row(1),
            ),
            (
                "@scope {\n  fsa1-row:nth-child(2n+0) fsa1-cell { color: #3f0421 }\n}",
                Target::RowEvery { a: 2, b: 0 },
            ),
            (
                "@scope {\n  fsa1-row:nth-child(2n) fsa1-cell { color: #3f0421 }\n}",
                Target::RowEvery { a: 2, b: 0 },
            ),
        ] {
            assert_eq!(rules(block)[0].target, want, "{block:?}");
        }
    }

    /// A rule the typed model reads nothing from leaves the MODEL, which has nothing to resolve from
    /// it, while the rule the model does carry stays.
    #[test]
    fn a_rule_typing_emptied_leaves_the_model() {
        let loaded = rules(
            "@scope {\n  fsa1-cell { color: #3f0421 }\n  fsa1-cell:first-child { box-shadow: none }\n}",
        );
        assert_eq!(loaded.len(), 1, "{loaded:?}");
        assert_eq!(loaded[0].target, Target::All);
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

    /// The order a sidecar is WRITTEN in is the order it cascades in, so the emitter writes what it
    /// was handed rather than a sorted copy of it: a sidecar out of ascending order and one
    /// repeating a selector each spell back to their own bytes, and back to the same rules again.
    #[test]
    fn a_sidecar_out_of_ascending_order_or_repeating_a_selector_spells_back_to_itself() {
        for content in [
            "@scope {\n  fsa1-cell:nth-child(2) { color: #d33333 }\n  fsa1-cell { color: #3f0421 }\n}",
            "@scope {\n  fsa1-cell { color: #d33333 }\n  fsa1-cell { color: #3f0421 }\n}",
        ] {
            let text = body(content);
            let root = root_of(REGION);
            let parsed = parse_rules(&sidecar(REGION), root, &text)
                .unwrap_or_else(|d| panic!("{text:?}: {:?}", d[0]));
            let spelled = spell_rules(root, &parsed);
            assert_eq!(spelled, text, "{text:?} did not spell back to itself");
            assert_eq!(
                parse_rules(&sidecar(REGION), root, &spelled)
                    .unwrap_or_else(|d| panic!("{spelled:?}: {:?}", d[0])),
                parsed,
                "{text:?} did not read back to the rules it was written from",
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
    fn an_empty_half_is_a_frame_refusal_even_where_the_property_is_uncarried() {
        assert_eq!(
            refusal("@scope {\n  fsa1-cell { box-shadow: }\n}").code,
            Code::PresentationSyntax,
        );
        assert!(
            one_uncarried("@scope {\n  fsa1-cell { box-shadow: none }\n}")
                .contains("not a supported presentation property"),
        );
    }

    #[test]
    fn a_rule_with_no_selector_is_a_frame_refusal() {
        assert_eq!(
            refusal("@scope {\n  { color: #3f0421 }\n}").code,
            Code::PresentationSyntax,
        );
    }

    /// An axis of extent 1 still has a line, and the selector naming it loads at the rank CSS gives
    /// it: a bare `fsa1-cell` written after a row rule is the less specific of the two and loses to
    /// it. A CELL selector addresses a coordinate on such a root as on any other (PRES1), so the
    /// sidecar loads and the whole rule is what the `.xlsx` is told it cannot carry.
    #[test]
    fn an_axis_of_extent_one_carries_the_selector_that_names_it() {
        let one_row = Shape { rows: 1, cols: 3 };
        let p = parse(
            "@scope {\n  fsa1-row:first-child fsa1-cell { color: #3f0421 }\n  fsa1-cell { color: #ffffff }\n}",
            one_row,
        )
        .expect("a one-row root carries its row selector");
        assert_eq!(p.rules[0].target, Target::Row(1));
        assert_eq!(crate::style::resolve(&p, 1, 2).color, Some(PLUM));

        let read = parse_rules_located(
            &sidecar(one_row),
            root_of(one_row),
            &body("@scope {\n  fsa1-row:first-child fsa1-cell:nth-child(2) { color: #3f0421 }\n}"),
        )
        .expect("a coordinate selector loads and is uncarried");
        assert_eq!(read.uncarried.len(), 1, "{:?}", read.uncarried);
        assert_eq!(read.uncarried[0].code, Code::XlsxNotCarried);
        assert!(
            matches!(read.uncarried[0].loc, Loc::Body { .. }),
            "{:?}",
            read.uncarried[0].loc
        );

        let one_col = Shape { rows: 4, cols: 1 };
        let p = parse(
            "@scope {\n  fsa1-cell:first-child { color: #3f0421 }\n}",
            one_col,
        )
        .expect("a one-column root carries its column selector");
        assert_eq!(p.rules[0].target, Target::Col(1));
    }

    /// Two rules of one target are two rules, applying in turn: the later wins property by property,
    /// which is the cascade the browser runs and not a merge the parser does.
    #[test]
    fn a_repeated_selector_cascades_in_source_order() {
        let p = parse(
            "@scope {\n  fsa1-cell { color: #d33333 }\n  fsa1-cell { color: #3f0421 }\n}",
            REGION,
        )
        .expect("a selector written twice layers");
        assert_eq!(p.rules.len(), 2, "{:?}", p.rules);
        assert_eq!(crate::style::resolve(&p, 1, 1).color, Some(PLUM));
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
        let d =
            uncarried("@scope {\n  fsa1-cell { color: red; font-size: 9px; box-shadow: none }\n}");
        assert_eq!(d.len(), 3, "{d:?}");
    }

    #[test]
    fn a_font_size_outside_excels_range_is_uncarried_as_out_of_range() {
        for value in [
            "0pt", "-1pt", "0.5pt", "5e-324pt", "410pt", "1e300pt", "inf",
        ] {
            let block = format!("@scope {{\n  fsa1-cell {{ font-size: {value} }}\n}}");
            let message = one_uncarried(&block);
            assert!(
                message.len() < 100,
                "{value:?} must earn an actionable message, got {}: {message}",
                message.len(),
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
            assert!(
                one_uncarried(&block).contains("a font family is one unquoted name"),
                "{value:?}",
            );
        }
    }

    /// The WRITE leg swept rather than sampled. [`Declaration::font_family`] is what an encoder asks
    /// before emitting a face, and the read leg no longer shares its list — so what it must still
    /// promise is that every name it admits parses back to the very declaration it spelled.
    #[test]
    fn the_write_leg_admits_only_the_family_names_it_can_spell_back() {
        for c in (b' '..=b'~').map(char::from).chain(['\t', '\n', 'é', '中']) {
            for name in [format!("My{c}Font"), format!("{c}Font"), format!("Font{c}")] {
                let Some(written) = Declaration::font_family(&name) else {
                    continue;
                };
                assert_eq!(
                    parse_declaration(&written.spell()).ok(),
                    Some(written.clone()),
                    "{name:?}",
                );
            }
        }
    }

    #[test]
    fn a_refusal_points_at_the_line_the_fault_is_on() {
        let d = refusal(
            "@scope {\n  fsa1-cell { color: #3f0421 }\n  fsa1-cell:nth-child(9) { color: #3f0421 }\n}",
        );
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
