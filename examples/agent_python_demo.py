# Concern: reads an FSA1 workbook directly from the filesystem with the Python stdlib | Non-concern: evaluating formulas (fsa1-cli does that; see agent_demo.sh) | IO: (a workbook dir) -> stdout
#
# STUB — structure only. The point it exists to make: reading FSA1 needs no library, because a
# filename IS the range and the body IS TSV. Fleshing this out is a later job.

import sys
from pathlib import Path

WB = Path(sys.argv[1] if len(sys.argv) > 1 else Path(__file__).parent / "sample_fsa1_dir")

for tab in sorted(p for p in WB.iterdir() if p.is_dir()):
    print(f"[{tab.name}]")
    for cell_file in sorted(tab.iterdir()):
        rows = cell_file.read_text().split("\n")
        print(f"  {cell_file.name:<10} {rows}")
