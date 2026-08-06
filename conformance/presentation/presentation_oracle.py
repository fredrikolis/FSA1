# Concern: the visual-fidelity harness — unpack, diff against the frozen expectation, pack, reopen via openpyxl | Non-concern: authoring the corpus, value parity | IO: (fixtures/*.xlsx) -> a table

"""Presentation round-trip oracle: prove an .xlsx's appearance survives `unpack` -> `pack` -> reopen.

Run via ``run.sh`` (which builds fsa1-cli, provisions the venv, and puts the xl-oracle sibling dir on
PYTHONPATH). Three gates, per fixture:

  * **SCOPE** — the range files `unpack` writes, names and contents, must equal the frozen expectation
    under ``expected/``. That expectation is a reading of the openpyxl-authored fixture, and is
    corrected ONLY when the reading was wrong — never edited to chase an FSA1 regression.
  * **WARNING** — a ``warn_*`` fixture must print exactly the SER3 warning its expectation names, and
    the run must NOT print the ``nothing lost`` line. A fixture with no expected warning must print it.
  * **VISUAL** — every cell the SOURCE styles must wear the same look in the PACKED export, read back
    through openpyxl (a third-party reader, so FSA1 never vouches for itself), along with every column
    width and row height the source declares. Skipped for the ``warn_*`` fixtures, whose whole concern
    is the declared loss.

Because ``conformance/presentation`` is a SIBLING of ``conformance/xl-oracle``, this inserts the
xl-oracle dir onto ``sys.path`` before importing it (a bare ``import oracle`` from here would raise
ModuleNotFoundError); ``run.sh`` also exports PYTHONPATH, so either path resolves it.
"""

import difflib
import os
import subprocess
import sys
import tempfile
from pathlib import Path

HERE = Path(__file__).resolve().parent
FIXTURE_DIR = HERE / "fixtures"
EXPECTED_DIR = HERE / "expected"

# Single-source the CLI location, the table clipper and the openpyxl handle from the sibling harness.
sys.path.insert(0, str(HERE.parent / "xl-oracle"))
import oracle  # noqa: E402  sibling module: DEFAULT_CLI / _clip / openpyxl

NOTHING_LOST = "unpack fidelity: nothing lost"

WIDTH_TOL = 1e-9

CORRECTION_RULE = (
    "a frozen expectation is corrected ONLY when the reading of the openpyxl-authored fixture was "
    "wrong -- never edited to chase an FSA1 regression"
)


def _run(cli, *args, cwd=None):
    """Run fsa1-cli, returning ``(returncode, stdout, stderr)``. ``cwd`` matters for `pack`, which
    derives its output name and writes it into the CURRENT directory."""
    proc = subprocess.run([str(cli), *args], capture_output=True, text=True, cwd=cwd)
    return proc.returncode, proc.stdout, proc.stderr


def read_tree(root):
    """Every range file under a workbook dir, keyed ``"<tab>/<name>"`` -> its exact contents."""
    files = {}
    for tab in sorted(p for p in root.iterdir() if p.is_dir()):
        for entry in sorted(tab.iterdir()):
            if entry.is_file():
                files[f"{tab.name}/{entry.name}"] = entry.read_text(encoding="utf-8")
    return files


def render(warnings, files):
    """The corpus's canonical text form, which IS the on-disk expectation format: a `warning:` line per
    SER3 item, then a `file:` line per range file with its contents `|`-prefixed. The prefix is what
    makes an empty grid line distinguishable from nothing at all."""
    out = []
    for text in warnings:
        out.append(f"warning: {text}")
    for name in sorted(files):
        out.append(f"file: {name}")
        for line in files[name].split("\n"):
            out.append(f"|{line}")
    return "\n".join(out) + "\n"


def parse_expectation(path):
    """The inverse of [render] over a frozen file, ignoring `#` comments and blank separators."""
    warnings, files, current = [], {}, None
    for raw in path.read_text(encoding="utf-8").split("\n"):
        if raw.startswith("#") or raw == "":
            continue
        if raw.startswith("warning: "):
            warnings.append(raw[len("warning: ") :])
        elif raw.startswith("file: "):
            current = raw[len("file: ") :]
            files[current] = []
        elif raw.startswith("|"):
            files[current].append(raw[1:])
        else:
            raise ValueError(f"{path.name}: not a directive, a comment or a `|` content line: {raw!r}")
    return warnings, {name: "\n".join(lines) for name, lines in files.items()}


def _rgb(color):
    """An openpyxl colour as its `AARRGGBB` string, with the theme-1/tint-0 no-op read as NO colour —
    it is the document's own default text colour, which the encoder deliberately never declares."""
    if color is None:
        return None
    if getattr(color, "type", None) == "theme":
        if color.theme == 1 and not color.tint:
            return None
        return f"theme{color.theme}+{color.tint}"
    return color.rgb if isinstance(color.rgb, str) else None


def _edge(side):
    return None if side is None or side.style is None else (side.style, _rgb(side.color))


def effective(ws, cell):
    """What a cell WEARS, which is not always what it states. A `<col style>` or a
    `<row s customFormat>` — how Excel and openpyxl both write "format this whole column/row" — dresses
    every cell of its axis that states nothing of its own, and openpyxl resolves `cell.font` through
    `cellXfs` alone. Following the axis here is what keeps the reader from under-reading the source."""
    if cell.has_style:
        return cell
    row = ws.row_dimensions.get(cell.row)
    if row is not None and row.customFormat and row.has_style:
        return row
    column = ws.column_dimensions.get(cell.column_letter)
    if column is not None and column.has_style:
        return column
    return cell


def cell_look(cell):
    """The visual facts a reopened cell must carry, as a flat dict so a divergence names the property.
    Takes whatever [effective] resolved to — a cell, or the axis dimension standing in for it, both of
    which openpyxl gives the same four style handles."""
    font, fill, border, align = cell.font, cell.fill, cell.border, cell.alignment
    solid = _rgb(fill.fgColor) if getattr(fill, "patternType", None) == "solid" else None
    return {
        "font-family": font.name,
        "font-size": font.sz,
        "font-weight": bool(font.b),
        "font-style": bool(font.i),
        "text-decoration": font.u,
        "color": _rgb(font.color),
        "background-color": solid,
        "border-top": _edge(border.top),
        "border-bottom": _edge(border.bottom),
        "border-left": _edge(border.left),
        "border-right": _edge(border.right),
        "text-align": align.horizontal,
        "vertical-align": align.vertical,
        "white-space": bool(align.wrapText),
    }


def _styled_coords(ws):
    """Every coordinate the source states anything about — a value, or a style of its own."""
    return [
        cell.coordinate
        for row in ws.iter_rows()
        for cell in row
        if cell.value is not None or cell.has_style
    ]


def grade_visual(fixture, export_path):
    """Compare the SOURCE's look against the PACKED export's, cell by cell and axis by axis, both read
    through openpyxl. Returns ``(rows, matched, graded)`` with rows ``(sheet, where, verdict, source,
    export, detail)``."""
    src = oracle.openpyxl.load_workbook(str(fixture))
    exp = oracle.openpyxl.load_workbook(str(export_path))
    rows, matched, graded = [], 0, 0
    for ws in src.worksheets:
        if ws.title not in exp.sheetnames:
            rows.append((ws.title, "-", "SHEET-MISSING", ws.title, "<absent>", "the export has no such sheet"))
            graded += 1
            continue
        target = exp[ws.title]
        for coord in _styled_coords(ws):
            wears = cell_look(effective(target, target[coord]))
            for prop, want in cell_look(effective(ws, ws[coord])).items():
                got = wears[prop]
                graded += 1
                if want == got:
                    matched += 1
                else:
                    rows.append((ws.title, f"{coord} {prop}", "LOOK-DIV", str(want), str(got),
                                 "the packed export does not wear the source's look"))
        for letter, dim in ws.column_dimensions.items():
            if dim.width is None or not dim.customWidth:
                continue
            got = target.column_dimensions[letter].width
            graded += 1
            if got is not None and abs(got - dim.width) <= WIDTH_TOL:
                matched += 1
            else:
                rows.append((ws.title, f"col {letter} width", "AXIS-DIV", str(dim.width), str(got),
                             "the column width did not survive the round-trip"))
        for index, dim in ws.row_dimensions.items():
            if dim.height is None or not dim.customHeight:
                continue
            got = target.row_dimensions[index].height
            graded += 1
            if got is not None and abs(got - dim.height) <= WIDTH_TOL:
                matched += 1
            else:
                rows.append((ws.title, f"row {index} height", "AXIS-DIV", str(dim.height), str(got),
                             "the row height did not survive the round-trip"))
    return rows, matched, graded


def grade_fixture(cli, fixture, expectation, workdir):
    """unpack -> SCOPE + WARNING diff -> pack -> reopen (VISUAL) -> re-unpack (idempotence). Returns
    ``(rows, matched, graded, failures)``; ``failures`` are whole-fixture faults, not per-cell ones."""
    want_warnings, want_files = parse_expectation(expectation)
    wb_dir = workdir / "wb"
    export_path = workdir / f"{wb_dir.name}.xlsx"
    rows, failures = [], []

    rc, _, err = _run(cli, "unpack", str(fixture), str(wb_dir))
    if rc != 0:
        return [], 0, 0, [f"unpack failed (exit {rc}): {err.strip()[:200]}"]

    got_files = read_tree(wb_dir)
    if want_files != got_files:
        failures.append("SCOPE diff (below); " + CORRECTION_RULE)
        rows.extend(_diff_rows(render([], want_files), render([], got_files)))

    reported = [line for line in err.split("\n") if line.startswith("  ")]
    got_warnings = sorted(line.strip() for line in reported)
    if got_warnings != sorted(want_warnings):
        failures.append(f"WARNING diff: expected {sorted(want_warnings)}, got {got_warnings}")
    if want_warnings and NOTHING_LOST in err:
        failures.append(f"a lossy conversion still printed {NOTHING_LOST!r}")
    if not want_warnings and NOTHING_LOST not in err:
        failures.append(f"a faithful conversion did not print {NOTHING_LOST!r}")

    if not got_files:
        return rows, 0, 0, failures

    rc, _, err = _run(cli, "pack", wb_dir.name, cwd=str(workdir))
    if rc != 0:
        return rows, 0, 0, failures + [f"pack failed (exit {rc}): {err.strip()[:200]}"]
    if not export_path.exists():
        return rows, 0, 0, failures + ["pack reported success but wrote no file"]

    matched = graded = 0
    if not fixture.stem.startswith("warn_"):
        visual_rows, matched, graded = grade_visual(fixture, export_path)
        rows.extend(visual_rows)

    again = workdir / "again"
    rc, _, err = _run(cli, "unpack", str(export_path), str(again))
    if rc != 0:
        failures.append(f"re-unpack of the export failed (exit {rc}): {err.strip()[:200]}")
    elif read_tree(again) != got_files:
        failures.append("the round-trip is not idempotent: unpack(pack(unpack(x))) != unpack(x)")
        rows.extend(_diff_rows(render([], got_files), render([], read_tree(again))))
    return rows, matched, graded, failures


def _diff_rows(want, got):
    """A unified diff rendered into the parity table's row shape, so one printer serves every gate."""
    diff = difflib.unified_diff(want.split("\n"), got.split("\n"), "frozen", "emitted", lineterm="")
    return [("", "", "DIFF", line, "", "") for line in diff]


def _print_table(title, rows):
    print(f"\n=== {title} ===")
    headers = ("sheet", "where", "verdict", "frozen/source", "emitted/export", "detail")
    widths = (10, 22, 14, 30, 30, 40)
    print("  ".join(h.ljust(w) for h, w in zip(headers, widths)))
    print("  ".join("-" * w for w in widths))
    for sheet, where, verdict, want, got, detail in rows:
        print("  ".join((
            oracle._clip(sheet, widths[0]).ljust(widths[0]),
            oracle._clip(where, widths[1]).ljust(widths[1]),
            verdict.ljust(widths[2]),
            oracle._clip(want, widths[3]).ljust(widths[3]),
            oracle._clip(got, widths[4]).ljust(widths[4]),
            oracle._clip(detail, widths[5]),
        )))


def run():
    cli = Path(os.environ.get("FSA1_CLI", oracle.DEFAULT_CLI))
    if not cli.exists():
        print(f"fsa1-cli not found at {cli} (build it: cargo build -p fsa1-cli)", file=sys.stderr)
        return 2
    fixtures = sorted(FIXTURE_DIR.glob("*.xlsx"))
    if not fixtures:
        print(f"no fixtures under {FIXTURE_DIR}", file=sys.stderr)
        return 2

    unfrozen = [f.name for f in fixtures if not (EXPECTED_DIR / f"{f.stem}.expected").exists()]
    if unfrozen:
        print(f"no expectation under {EXPECTED_DIR} for: {', '.join(unfrozen)}", file=sys.stderr)
        print("Freeze each by reading the openpyxl-authored fixture; see PROVENANCE.md.",
              file=sys.stderr)
        return 2

    artifacts = HERE / ".artifacts"
    artifacts.mkdir(exist_ok=True)
    total_matched = total_graded = 0
    failed = []
    with tempfile.TemporaryDirectory(prefix="presentation-", dir=str(artifacts)) as work_root:
        for fixture in fixtures:
            workdir = Path(work_root) / fixture.stem
            workdir.mkdir(parents=True, exist_ok=True)
            expectation = EXPECTED_DIR / f"{fixture.stem}.expected"
            rows, matched, graded, failures = grade_fixture(cli, fixture, expectation, workdir)
            if rows:
                _print_table(fixture.name, rows)
            else:
                print(f"\n=== {fixture.name} ===\n  no divergences")
            print(f"  -> {matched}/{graded} visual facts survive the round-trip")
            for failure in failures:
                print(f"  FAILURE: {failure}")
            total_matched += matched
            total_graded += graded
            if failures or any(r[2] != "DIFF" for r in rows):
                failed.append(fixture.name)

    print()
    print(f"presentation: {total_matched}/{total_graded} visual facts survive across "
          f"{len(fixtures)} fixture(s)")
    if failed:
        print(f"PRESENTATION FAILURES: {', '.join(sorted(set(failed)))}")
        return 1
    print("no presentation divergences -- every fixture's appearance survives the round-trip")
    return 0


if __name__ == "__main__":
    sys.exit(run())
