# Concern: the whole-workbook differential harness — formula cells diffed, lib vs FSA1 | Non-concern: what FSA1 computes, per-formula grading | IO: (corpus_workbooks/*.xlsx) -> a table
"""ENG6 whole-workbook oracle: grade FSA1 against `formulas` on real multi-formula .xlsx files.

Run via ``run.sh --workbook`` (which activates the venv and builds fsa1-cli). This is the real
'unpack -> compute -> diff vs reference' fitness on genuine files, complementing the per-formula
``oracle.py``. It reuses oracle.py's ``compare``/``classify_ref``/``fsa1_value`` so the value
comparison is single-sourced.
"""

import json
import os
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

import openpyxl

import oracle  # sibling module: reuse its comparison + FSA1 eval (single source of truth)

HERE = Path(__file__).resolve().parent
WORKBOOKS_DIR = HERE / "corpus_workbooks"
LIB_GAPS_FILE = WORKBOOKS_DIR / "lib_gaps.json"
# Real-file chains accumulate IEEE rounding across periods, so allow a small absolute floor on top of
# oracle.REL_TOL's relative floor. A genuine divergence dwarfs this; FP noise stays under it.
DEFAULT_TOL = 1e-6


def load_lib_gaps():
    """Declared reference-library defects to EXCLUDE (printed, not graded) — the whole-workbook analogue
    of the per-formula ``lib_gap`` flag. Keyed ``(workbook_stem, SHEET_UPPER, CELL_UPPER) -> reason``;
    each is evidenced in KNOWN-LIB-GAPS.md. Distorting FSA1 to a wrong reference is forbidden."""
    if not LIB_GAPS_FILE.exists():
        return {}
    with open(LIB_GAPS_FILE, encoding="utf-8") as handle:
        entries = json.load(handle)
    return {
        (e["workbook"], e["sheet"].upper(), e["cell"].upper()): e.get("reason", "")
        for e in entries
    }


def formula_cells(xlsx_path):
    """Every formula cell as ``(sheet_name, coord)`` — sheet_name in the workbook's own casing (the
    FSA1 tab folder name), coord like ``A3``. A formula cell is one openpyxl marks data_type 'f'."""
    wb = openpyxl.load_workbook(xlsx_path, data_only=False)
    cells = []
    for ws in wb.worksheets:
        for row in ws.iter_rows():
            for cell in row:
                if cell.data_type == "f":
                    cells.append((ws.title, cell.coordinate))
    return cells


def reference_solution(xlsx_path):
    """Compute the whole workbook with `formulas` and return ``{(SHEET_UPPER, CELL_UPPER): value}``.

    The lib keys results ``'[file.xlsx]SHEET'!CELL``; we key by (upper sheet, upper cell) so a lookup
    from openpyxl's formula-cell list (whatever its casing) resolves.
    """
    model = oracle.formulas.ExcelModel().loads(str(xlsx_path)).finish()
    solution = model.calculate()
    out = {}
    for key, cell in solution.items():
        # key example: "'[pnl.xlsx]PNL'!B3"  — split sheet vs cell on the "'!" boundary.
        if "'!" not in key:
            continue
        left, cell_addr = key.split("'!", 1)
        sheet = left.rsplit("]", 1)[-1]  # drop the "'[file.xlsx]" prefix
        out[(sheet.upper(), cell_addr.upper())] = oracle._unwrap(cell.value)
    return out


def grade_workbook(cli, xlsx_path, workdir, lib_gaps):
    """Import one workbook, then diff every formula cell. Returns
    ``(rows, matched, graded, defects, excluded)``. ``rows`` are
    ``(sheet, cell, verdict, ref_disp, FSA1, detail)`` for the parity table.
    """
    wb_dir = workdir / "cwb"
    proc = subprocess.run(
        # `unpack`, not `import` — plan 10 renamed the verb (no alias kept).
        [str(cli), "unpack", str(xlsx_path), str(wb_dir)],
        capture_output=True,
        text=True,
    )
    if proc.returncode != 0:
        note = proc.stdout.strip()[:80] or proc.stderr.strip()[:80]
        # An import-level failure is an FSA1 defect (it forces a non-zero exit) but it is NOT a graded
        # formula cell — count it as (graded=0, defects=1) so the "/graded formula cells" summary total
        # reflects only real per-cell parity and never misrepresents a whole-workbook failure as one cell.
        return ([("*", "*", "DIVERGE", "<import failed>", None, note)], 0, 0, 1, 0)

    ref = reference_solution(xlsx_path)
    rows, matched, graded, defects, excluded = [], 0, 0, 0, 0
    for sheet, coord in formula_cells(xlsx_path):
        ref_value = ref.get((sheet.upper(), coord.upper()))
        ch_value, _exit, _note = oracle.fsa1_value(cli, wb_dir, f"={coord}", tab=sheet)
        gap = lib_gaps.get((xlsx_path.stem, sheet.upper(), coord.upper()))
        ref_disp = ref_value if ref_value is not None else "<no ref value>"
        if gap is not None:
            # A declared reference-library defect: print it (both values) but never grade it.
            excluded += 1
            rows.append((sheet, coord, "EXCLUDED", str(ref_disp), ch_value, gap))
            continue
        graded += 1
        if ref_value is None:
            defects += 1
            rows.append((sheet, coord, "DIVERGE", "<no ref value>", ch_value, "reference had no value for this formula cell"))
            continue
        case = {"tol": DEFAULT_TOL, "name": f"{sheet}!{coord}", "category": xlsx_path.stem}
        is_match, ref_disp, kind, detail = oracle.compare(case, ref_value, ch_value)
        if is_match:
            matched += 1
            rows.append((sheet, coord, "MATCH", ref_disp, ch_value, kind))
        else:
            defects += 1
            rows.append((sheet, coord, "DIVERGE", ref_disp, ch_value, detail))
    return rows, matched, graded, defects, excluded


def _print_table(title, rows):
    print(f"\n=== {title} ===")
    headers = ("sheet", "cell", "verdict", "reference", "FSA1", "detail")
    widths = (10, 6, 8, 18, 18, 44)
    print("  ".join(h.ljust(w) for h, w in zip(headers, widths)))
    print("  ".join("-" * w for w in widths))
    for sheet, cell, verdict, ref_disp, ch_value, detail in rows:
        cells = (
            oracle._clip(sheet, widths[0]).ljust(widths[0]),
            oracle._clip(cell, widths[1]).ljust(widths[1]),
            verdict.ljust(widths[2]),
            oracle._clip(ref_disp, widths[3]).ljust(widths[3]),
            oracle._clip(ch_value, widths[4]).ljust(widths[4]),
            oracle._clip(detail, widths[5]),
        )
        print("  ".join(cells))


def run():
    cli = Path(os.environ.get("FSA1_CLI", oracle.DEFAULT_CLI))
    if not cli.exists():
        print(f"fsa1-cli not found at {cli} (build it: cargo build -p fsa1-cli)", file=sys.stderr)
        return 2
    workbooks = sorted(WORKBOOKS_DIR.glob("*.xlsx"))
    if not workbooks:
        print(f"no workbooks under {WORKBOOKS_DIR}", file=sys.stderr)
        return 2

    lib_gaps = load_lib_gaps()
    artifacts = HERE / ".artifacts"
    artifacts.mkdir(exist_ok=True)
    work_root = Path(tempfile.mkdtemp(prefix="xl-wb-oracle-", dir=str(artifacts)))
    total_matched = total_graded = total_defects = total_excluded = 0
    try:
        for xlsx in workbooks:
            workdir = work_root / xlsx.stem
            workdir.mkdir(parents=True, exist_ok=True)
            rows, matched, graded, defects, excluded = grade_workbook(cli, xlsx, workdir, lib_gaps)
            _print_table(xlsx.name, rows)
            total_matched += matched
            total_graded += graded
            total_defects += defects
            total_excluded += excluded
    finally:
        shutil.rmtree(work_root, ignore_errors=True)

    print()
    print(f"whole-workbook parity: {total_matched}/{total_graded} graded formula cells match "
          f"across {len(workbooks)} workbook(s) (excluded lib-gaps: {total_excluded})")
    if total_defects:
        print(f"DIVERGENCES (FSA1 defects): {total_defects}")
    else:
        print("no FSA1-defect divergences")
    return 1 if total_defects else 0


if __name__ == "__main__":
    sys.exit(run())
