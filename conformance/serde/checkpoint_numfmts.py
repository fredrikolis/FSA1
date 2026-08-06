#!/usr/bin/env python3
# Concern: checks openpyxl and `formulas` both accept an FSA1 export | Non-concern: the calamine leg, SER2 grading, authoring the corpus | IO: (a .xlsx) -> pass/fail

import sys

import openpyxl


def check_openpyxl(path: str) -> None:
    wb = openpyxl.load_workbook(path)
    ws = wb["Sheet1"]
    got = {coord: ws[coord].number_format for coord in ("A1", "A2", "A3", "A4", "A5")}
    expected = {
        "A1": "m/d/yyyy",
        "A2": "$#,##0.00",
        "A3": "0.00%",
        "A4": "0.00",
        "A5": "$#,##0.00",
    }
    for coord, want in expected.items():
        assert got[coord] == want, f"{coord}: openpyxl read {got[coord]!r}, expected {want!r}"
    print(f"openpyxl  PASS  (number_format read back for {sorted(expected)})")


def check_formulas(path: str) -> None:
    import formulas

    xl = formulas.ExcelModel().loads(path).finish()
    solution = xl.calculate()
    # `formulas` keys cells as "'[FILE]SHEET'!A5"; match the suffix so the file name is not baked in.
    a5 = next(
        (v for k, v in solution.items() if k.upper().endswith("!A5")),
        None,
    )
    assert a5 is not None, "formulas produced no A5 value"
    value = a5.value[0, 0] if hasattr(a5.value, "shape") else a5.value
    assert float(value) == 2.0, f"formulas computed A5 = {value!r}, expected 2.0"
    print("formulas  PASS  (formatted formula =1+1 computed to 2.0)")


def main() -> int:
    if len(sys.argv) != 2:
        print("usage: checkpoint_numfmts.py <export.xlsx>", file=sys.stderr)
        return 2
    path = sys.argv[1]
    check_openpyxl(path)
    check_formulas(path)
    print("CHECKPOINT PASS — openpyxl + formulas both accept the <numFmts>/s= block")
    return 0


if __name__ == "__main__":
    sys.exit(main())
