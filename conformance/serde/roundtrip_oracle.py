# Concern: the SER2 round-trip harness — unpack then pack each fixture, grading three gates | Non-concern: SER3, roundtrip.rs's reopen leg, authoring accept/ | IO: (accept/*.xlsx) -> a parity table
"""SER2 round-trip oracle: prove each accept/ fixture EXPORTS to a file that opens identically.

Run via ``run.sh`` (which builds fsa1-cli, provisions the venv, and puts the xl-oracle sibling dir
on PYTHONPATH). The grader retargets the whole-workbook oracle's unpack->eval->diff pattern to
pack->reopen->diff: it reuses ``oracle.compare`` / ``oracle.classify_ref`` and
``workbook_oracle.reference_solution`` verbatim so the value comparison + reference computation are
single-sourced with the ENG6 oracle, never re-implemented.

Because ``conformance/serde`` is a SIBLING of ``conformance/xl-oracle``, this inserts the xl-oracle
dir onto ``sys.path`` before importing those modules (a bare ``import oracle`` from here would raise
ModuleNotFoundError); ``run.sh`` also exports PYTHONPATH, so either path resolves them.
"""

import datetime
import json
import os
import numbers
import subprocess
import sys
import tempfile
from pathlib import Path

HERE = Path(__file__).resolve().parent
ACCEPT_DIR = HERE / "accept"
LIB_GAPS_FILE = HERE / "lib_gaps.json"

# The independent numFmt renderer (this dir), anchored to golden_numfmt.json — the FORMAT gate's engine.
from numfmt_render import numfmt_render  # noqa: E402

# Single-source the comparison + reference computation from the sibling xl-oracle harness.
sys.path.insert(0, str(HERE.parent / "xl-oracle"))
import oracle  # noqa: E402  sibling module: compare / classify_ref / _unwrap / DEFAULT_CLI
import workbook_oracle  # noqa: E402  sibling module: reference_solution (formulas whole-workbook compute)

# The Excel 1900 serial origin (matches numfmt_render._EPOCH) — to coerce an openpyxl datetime back to
# the serial the FORMAT gate renders.
_EPOCH = datetime.datetime(1899, 12, 30)

# Real-file formula chains accumulate IEEE rounding; allow the same small absolute floor the
# whole-workbook oracle uses on top of oracle.REL_TOL's relative floor.
DEFAULT_TOL = 1e-6


def load_lib_gaps():
    """Declared reference-library defects to EXCLUDE from grading (printed, not graded) — keyed
    ``(fixture_stem, SHEET_UPPER, CELL_UPPER) -> reason``. The IDENTITY gate compares `formulas`
    against ITSELF (source vs export), so a lib gap yields the SAME value on both sides and matches
    naturally; this hook exists to mirror the xl-oracle exclusion mechanism and to defuse any cell
    whose reference computation is non-deterministic. Empty/absent file => no exclusions."""
    if not LIB_GAPS_FILE.exists():
        return {}
    with open(LIB_GAPS_FILE, encoding="utf-8") as handle:
        entries = json.load(handle)
    return {
        (e["fixture"], e["sheet"].upper(), e["cell"].upper()): e.get("reason", "")
        for e in entries
    }


def _run(cli, *args, cwd=None):
    """Run fsa1-cli, returning (returncode, stdout+stderr).

    ``cwd`` matters for `pack`, which derives its output name and writes it into the CURRENT
    directory (plan 10) — running it from the fixture workdir is what keeps the export beside the
    workbook instead of polluting the repo root.
    """
    proc = subprocess.run(
        [str(cli), *args], capture_output=True, text=True, cwd=cwd
    )
    return proc.returncode, (proc.stdout + proc.stderr)


def _is_blank(value):
    """A formulas cell value that carries no content (an empty cell the lib still keys)."""
    if value is None:
        return True
    if isinstance(value, str) and value == "":
        return True
    return False


def _export_value_str(value):
    """Render a `formulas` export value into the string form ``oracle.compare`` expects as FSA1's
    output: an error/bool/text stays its canonical string; a number becomes a numeric string so
    ``compare``'s number branch (``float(fsa1_str)``) reparses it."""
    kind, canonical = oracle.classify_ref(value)
    if kind == "num":
        return repr(float(canonical))
    return canonical


def _to_serial(value):
    """Coerce a cell value to the numeric Excel serial the FORMAT gate renders, or ``None`` if it is not
    a numeric-format value (a bool/str/formula-string — never a formatted-number cell in this corpus).
    A `formulas`/numpy number passes through; an openpyxl ``datetime``/``date`` becomes its serial."""
    value = oracle._unwrap(value)
    if isinstance(value, bool):
        return None
    if isinstance(value, datetime.datetime):
        return (value - _EPOCH).total_seconds() / 86400.0
    if isinstance(value, datetime.date):
        return float((datetime.datetime(value.year, value.month, value.day) - _EPOCH).days)
    if isinstance(value, numbers.Number):
        return float(value)
    return None


def _numfmt_map(xlsx_path):
    """Every non-empty cell's ``(number_format, stored_value)`` keyed ``(SHEET_UPPER, COORD_UPPER)`` via
    openpyxl. ``stored_value`` is the fallback display value for a LITERAL cell that the `formulas`
    solution does not surface (a standalone literal not referenced by any formula); a FORMULA cell's
    ``stored_value`` is its ``=…`` text (non-numeric), so its display value must come from `formulas`."""
    wb = oracle.openpyxl.load_workbook(str(xlsx_path))
    fmts, vals = {}, {}
    for ws in wb.worksheets:
        for row in ws.iter_rows():
            for cell in row:
                if cell.value is None:
                    continue
                key = (ws.title.upper(), cell.coordinate.upper())
                fmts[key] = cell.number_format
                vals[key] = cell.value
    return fmts, vals


def _is_general(code):
    """Whether a numFmt code is the default General (openpyxl's unformatted sentinel) — not FORMAT-graded."""
    return code is None or code.strip() == "" or code.strip().lower() == "general"


def grade_formats(fixture, export_path, ref_src):
    """The GRID7 FORMAT gate (§9.2): for each cell the SOURCE formats non-General, render(source code,
    value) vs render(export code, value) through the independent ``numfmt_render`` and assert EQUAL —
    rendered display-equivalence, NOT code-string equality. The VALUE is the `formulas` reference
    solution (so a formula cell — no cached <v> on export — grades exactly like a literal), falling back
    to the openpyxl stored value for a literal the solution does not surface. Returns
    ``(rows, matched, graded, defects)`` with rows ``(sheet, cell, verdict, src_render, exp_render, detail)``."""
    src_fmts, src_vals = _numfmt_map(fixture)
    exp_fmts, exp_vals = _numfmt_map(export_path)
    rows, matched, graded, defects = [], 0, 0, 0
    for key in sorted(src_fmts):
        scode = src_fmts[key]
        if _is_general(scode):
            continue
        sheet, cell = key
        ecode = exp_fmts.get(key)
        if ecode is None:
            defects += 1
            graded += 1
            rows.append((sheet, cell, "FMT-DIV", scode, "<absent>",
                         "formatted source cell is absent from the export"))
            continue
        # The display value: the `formulas` reference (works for a formula cell), else the openpyxl
        # stored literal value (a standalone literal not in the solution).
        value = _to_serial(ref_src.get(key))
        if value is None:
            value = _to_serial(src_vals.get(key))
        if value is None:
            # Not a numeric-format value (would be a non-number under a numFmt — none in this corpus).
            continue
        graded += 1
        src_disp = numfmt_render(scode, value)
        exp_disp = numfmt_render(ecode, value)
        if src_disp == exp_disp:
            matched += 1
            rows.append((sheet, cell, "FMT-OK", src_disp, exp_disp, f"{scode!r} vs {ecode!r}"))
        else:
            defects += 1
            rows.append((sheet, cell, "FMT-DIV", src_disp, exp_disp,
                         f"render diverges: {scode!r} -> {src_disp!r}  vs  {ecode!r} -> {exp_disp!r}"))
    return rows, matched, graded, defects


def grade_fixture(cli, fixture, workdir, lib_gaps):
    """unpack --strict -> export -> reopen (structural) -> diff vs source (identity). Returns
    ``(rows, matched, graded, defects, excluded, fatal)``. ``fatal`` is a non-cell failure
    (import/export/reader error) that fails the fixture outright."""
    stem = fixture.stem
    wb_dir = workdir / "wb"
    export_path = workdir / f"{wb_dir.name}.xlsx"  # the name `pack` derives (plan 10)

    # `unpack`/`pack`, not `import`/`export` — plan 10 renamed both verbs with no aliases kept.
    rc, out = _run(cli, "unpack", "--strict", str(fixture), str(wb_dir))
    if rc != 0:
        return [], 0, 0, 0, 0, f"unpack --strict failed (exit {rc}): {out.strip()[:100]}"
    # `pack` takes ONE positional and DERIVES `./<folder-basename>.xlsx` in the CWD (plan 10), so run
    # it from the workdir and pick the derived name up there.
    rc, out = _run(cli, "pack", wb_dir.name, cwd=str(workdir))
    if rc != 0:
        return [], 0, 0, 0, 0, f"pack failed (exit {rc}): {out.strip()[:100]}"
    if not export_path.exists():
        return [], 0, 0, 0, 0, "export reported success but wrote no file"

    # STRUCTURAL gate — the two Python readers must BOTH reopen the export without raising.
    # (The calamine leg of the three-reader triangulation runs in fsa1-cli/tests/roundtrip.rs.)
    try:
        oracle.openpyxl.load_workbook(str(export_path))
    except Exception as exc:  # noqa: BLE001
        return [], 0, 0, 0, 0, f"openpyxl could not reopen the export: {type(exc).__name__}: {exc}"
    try:
        ref_exp = workbook_oracle.reference_solution(export_path)
    except Exception as exc:  # noqa: BLE001
        return [], 0, 0, 0, 0, f"formulas could not compute the export: {type(exc).__name__}: {exc}"

    # IDENTITY gate — the source's known values (formulas' compute of the SOURCE) must match the
    # export's recompute cell-for-cell. Cached values are omitted on export (§4.3), so `formulas`
    # recomputes on load — a genuine external "opens-identically" check, not a byte echo.
    try:
        ref_src = workbook_oracle.reference_solution(fixture)
    except Exception as exc:  # noqa: BLE001
        return [], 0, 0, 0, 0, f"formulas could not compute the source: {type(exc).__name__}: {exc}"

    rows, matched, graded, defects, excluded = [], 0, 0, 0, 0
    for key in sorted(set(ref_src) | set(ref_exp)):
        sheet, cell = key
        src_val = ref_src.get(key)
        exp_val = ref_exp.get(key)
        if _is_blank(src_val) and _is_blank(exp_val):
            continue
        gap = lib_gaps.get((stem, sheet, cell))
        if gap is not None:
            excluded += 1
            rows.append((sheet, cell, "EXCLUDED", str(src_val), str(exp_val), gap))
            continue
        graded += 1
        if _is_blank(src_val) != _is_blank(exp_val):
            defects += 1
            which = "source" if _is_blank(src_val) else "export"
            rows.append((sheet, cell, "DIVERGE", str(src_val), str(exp_val),
                         f"cell present in {'export' if which == 'source' else 'source'} only"))
            continue
        case = {"tol": DEFAULT_TOL, "name": f"{sheet}!{cell}", "category": stem}
        exp_str = _export_value_str(exp_val)
        is_match, ref_disp, kind, detail = oracle.compare(case, src_val, exp_str)
        if is_match:
            matched += 1
            rows.append((sheet, cell, "MATCH", ref_disp, exp_str, kind))
        else:
            defects += 1
            rows.append((sheet, cell, "DIVERGE", ref_disp, exp_str, detail))

    # FORMAT gate (GRID7, §9.2): rendered display-equivalence over every formatted cell. Its rows +
    # match/graded/defect counts fold into the fixture's totals so a format divergence fails SER2.
    try:
        fmt_rows, fmt_matched, fmt_graded, fmt_defects = grade_formats(fixture, export_path, ref_src)
    except Exception as exc:  # noqa: BLE001
        return rows, matched, graded, defects, excluded, (
            f"FORMAT gate raised: {type(exc).__name__}: {exc}"
        )
    rows.extend(fmt_rows)
    matched += fmt_matched
    graded += fmt_graded
    defects += fmt_defects
    return rows, matched, graded, defects, excluded, None


def _print_table(title, rows):
    print(f"\n=== {title} ===")
    headers = ("sheet", "cell", "verdict", "source", "export", "detail")
    widths = (10, 6, 8, 18, 18, 40)
    print("  ".join(h.ljust(w) for h, w in zip(headers, widths)))
    print("  ".join("-" * w for w in widths))
    for sheet, cell, verdict, src_disp, exp_disp, detail in rows:
        print("  ".join((
            oracle._clip(sheet, widths[0]).ljust(widths[0]),
            oracle._clip(cell, widths[1]).ljust(widths[1]),
            verdict.ljust(widths[2]),
            oracle._clip(src_disp, widths[3]).ljust(widths[3]),
            oracle._clip(exp_disp, widths[4]).ljust(widths[4]),
            oracle._clip(detail, widths[5]),
        )))


def run():
    cli = Path(os.environ.get("FSA1_CLI", oracle.DEFAULT_CLI))
    if not cli.exists():
        print(f"fsa1-cli not found at {cli} (build it: cargo build -p fsa1-cli)", file=sys.stderr)
        return 2
    fixtures = sorted(ACCEPT_DIR.glob("*.xlsx"))
    if not fixtures:
        print(f"no accept fixtures under {ACCEPT_DIR}", file=sys.stderr)
        return 2

    lib_gaps = load_lib_gaps()
    artifacts = HERE / ".artifacts"
    artifacts.mkdir(exist_ok=True)
    total_matched = total_graded = total_defects = total_excluded = 0
    failed_fixtures = []
    with tempfile.TemporaryDirectory(prefix="serde-roundtrip-", dir=str(artifacts)) as work_root:
        for fixture in fixtures:
            workdir = Path(work_root) / fixture.stem
            workdir.mkdir(parents=True, exist_ok=True)
            rows, matched, graded, defects, excluded, fatal = grade_fixture(cli, fixture, workdir, lib_gaps)
            if fatal is not None:
                print(f"\n=== {fixture.name} ===\n  SER2 FAILURE: {fatal}")
                failed_fixtures.append(fixture.name)
                continue
            _print_table(fixture.name, rows)
            print(f"  -> {matched}/{graded} cells opens-identically "
                  f"(excluded lib-gaps: {excluded}; divergences: {defects})")
            total_matched += matched
            total_graded += graded
            total_defects += defects
            total_excluded += excluded
            if defects:
                failed_fixtures.append(fixture.name)

    print()
    print(f"SER2 round-trip: {total_matched}/{total_graded} graded cells open identically "
          f"across {len(fixtures)} accept fixture(s) (excluded lib-gaps: {total_excluded})")
    if failed_fixtures:
        print(f"SER2 FAILURES: {', '.join(sorted(set(failed_fixtures)))}")
        return 1
    print("no SER2 divergences — every accept fixture opens identically")
    return 0


if __name__ == "__main__":
    sys.exit(run())
