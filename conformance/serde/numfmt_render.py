# Concern: the independent Python numFmt renderer for the SER2 format gate | Non-concern: reading the .xlsx, grading the round-trip, FSA1's renderer | IO: (a formatCode + a value) -> a string
"""An independent, deterministic, offline Excel-numFmt renderer for the SER2 render-equivalence gate.

Two facts (probed at the pinned SHA) force a bespoke renderer rather than a library:
  1. No offline library renders Excel numFmt — openpyxl exposes only the id->code table, `formulas`
     renders nothing, and babel is CLDR (not Excel numFmt) and not installed.
  2. fsa1-ast's renderer deliberately REJECTS the color/condition/accounting-padding brackets real
     source codes carry (its TEXT() semantics), so it cannot render the RAW source `formatCode`.

This renderer covers EXACTLY the accepted catalog (plan 07 §4.1) plus the cosmetic source tokens SER2
normalizes away. It is bound to the SAME external ECMA-376 golden vectors as fsa1-ast's renderer
(``--selftest``), so a systematically-wrong-but-consistent renderer cannot pass by agreeing with the
other in-house renderer — both must agree with the third-party golden set.
"""

import datetime
import json
import sys
from decimal import ROUND_HALF_UP, Decimal
from pathlib import Path

HERE = Path(__file__).resolve().parent

# The Excel 1900 date system's serial origin. datetime(1899,12,30) + serial days reproduces Excel's
# civil dates for every serial >= 61 (i.e. every date at/after 1900-03-01), which is the whole modern
# range the accepted catalog and the golden vectors exercise; the fictitious 1900-02-29 leap-year-bug
# day (serial 60) is outside that range and never authored into a fixture.
_EPOCH = datetime.datetime(1899, 12, 30)

_MONTHS_ABBR = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"]
_MONTHS_FULL = [
    "January", "February", "March", "April", "May", "June",
    "July", "August", "September", "October", "November", "December",
]

# Currency glyphs that may appear as a bare leading literal in an accepted currency/accounting code.
_CURRENCY_GLYPHS = ("$", "£", "€", "¥", "Rs.")


# ------------------------------------------------------------------ cosmetic-token normalization ----

def _strip_cosmetic(section: str) -> str:
    """Drop the tokens SER2 does not grade and unwrap literals, leaving a bare renderable section:
      * ``[Red]``/``[Blue]``/``[Color 12]`` color brackets -> dropped (cosmetic loss, §4.2);
      * ``[$sym-loc]`` currency bracket -> its bare symbol glyph;
      * ``_x`` (alignment padding, width of the next char) -> dropped;
      * ``*x`` (fill repeat of the next char) -> dropped (a single space would also normalize away);
      * ``"lit"`` -> the literal text ``lit``;
      * a backslash-escaped char ``\\x`` -> the literal ``x``.
    A conditional bracket ``[<100]`` never reaches here (refused upstream); if one did it is dropped
    like a color bracket, which is safe because such a section is never authored into an accept
    fixture. The result carries only placeholders (0 # , . %) + literal characters + date/time letters.
    """
    out = []
    i = 0
    n = len(section)
    while i < n:
        c = section[i]
        if c == "[":
            end = section.find("]", i)
            if end == -1:
                i += 1
                continue
            inner = section[i + 1:end]
            if inner.startswith("$"):
                # [$sym-loc] or [$sym] -> the symbol before an optional -locale.
                sym = inner[1:].split("-", 1)[0]
                out.append(sym)
            # color / condition / anything else -> dropped.
            i = end + 1
        elif c == '"':
            end = section.find('"', i + 1)
            if end == -1:
                i += 1
                continue
            out.append(section[i + 1:end])
            i = end + 1
        elif c == "\\":
            if i + 1 < n:
                out.append(section[i + 1])
                i += 2
            else:
                i += 1
        elif c == "_":
            # alignment padding: skip the underscore AND the next (width-defining) char.
            i += 2
        elif c == "*":
            # fill repeat: skip the star AND the next (repeated) char.
            i += 2
        else:
            out.append(c)
            i += 1
    return "".join(out)


def _split_sections(code: str) -> list:
    """Split a numFmt into its ``pos;neg;zero;text`` sections on top-level ``;`` (a ``;`` inside a
    ``[...]`` bracket or a ``"..."`` string is not a separator)."""
    sections = []
    buf = []
    i = 0
    n = len(code)
    while i < n:
        c = code[i]
        if c == "[":
            end = code.find("]", i)
            end = n if end == -1 else end
            buf.append(code[i:end + 1])
            i = end + 1
        elif c == '"':
            end = code.find('"', i + 1)
            end = n if end == -1 else end
            buf.append(code[i:end + 1])
            i = end + 1
        elif c == ";":
            sections.append("".join(buf))
            buf = []
            i += 1
        else:
            buf.append(c)
            i += 1
    sections.append("".join(buf))
    return sections


# ------------------------------------------------------------------------- number-family render ----

def _is_datetime_code(section: str) -> bool:
    """A section renders a DATE/TIME iff (after cosmetic stripping) it carries a date/time mask letter
    and no numeric placeholder — the accepted catalog never mixes the two in one code."""
    bare = _strip_cosmetic(section)
    return any(ch in "ymdhsYMDHS" for ch in bare) and not any(ch in "0#" for ch in bare)


def _render_number(section: str, value: float) -> str:
    """Render ``value`` (already the correct-sign magnitude for this section) through a bare number
    section: prefix literals + grouped/zero-padded digits at the section's decimals + suffix literals.
    ``%`` both scales (x100 each) and stays a literal in its position."""
    bare = _strip_cosmetic(section)
    percent = bare.count("%")
    v = Decimal(repr(value)) * (Decimal(100) ** percent)

    digit_idx = [i for i, ch in enumerate(bare) if ch in "0#"]
    if not digit_idx:
        return bare  # a pure-literal section (no number placeholder)
    lo, hi = digit_idx[0], digit_idx[-1]
    pattern = bare[lo:hi + 1]
    prefix = bare[:lo]
    suffix = bare[hi + 1:]

    grouping = "," in pattern
    if "." in pattern:
        int_pat, frac_pat = pattern.split(".", 1)
        decimals = frac_pat.count("0") + frac_pat.count("#")
    else:
        int_pat, decimals = pattern, 0
    min_int = int_pat.count("0")

    quant = Decimal(1).scaleb(-decimals)
    d = v.quantize(quant, rounding=ROUND_HALF_UP)
    sign = "-" if d < 0 else ""
    d = abs(d)

    int_val = int(d)
    int_str = str(int_val)
    if len(int_str) < min_int:
        int_str = "0" * (min_int - len(int_str)) + int_str
    if grouping:
        int_str = _group_thousands(int_str)

    if decimals > 0:
        frac_str = str(d - int_val)  # "0.xy"
        frac_digits = frac_str.split(".", 1)[1] if "." in frac_str else ""
        frac_digits = (frac_digits + "0" * decimals)[:decimals]
        number = f"{int_str}.{frac_digits}"
    else:
        number = int_str

    return f"{prefix}{sign}{number}{suffix}"


def _group_thousands(int_str: str) -> str:
    """Insert ``,`` every three digits from the right."""
    rev = int_str[::-1]
    chunks = [rev[i:i + 3] for i in range(0, len(rev), 3)]
    return ",".join(chunks)[::-1]


def _render_number_code(code: str, value: float) -> str:
    """Render a NUMBER-family code, honoring sign-section selection: a value < 0 uses the negative
    section (rendered on its magnitude) if one exists, else the positive section with a leading ``-``;
    a value == 0 uses the zero section if one exists, else the positive."""
    sections = _split_sections(code)
    if value < 0 and len(sections) >= 2 and sections[1].strip():
        return _render_number(sections[1], abs(value))
    if value == 0 and len(sections) >= 3 and sections[2].strip():
        return _render_number(sections[2], 0.0)
    # positive section handles >0, and negatives when there is no dedicated negative section.
    return _render_number(sections[0], value)


# --------------------------------------------------------------------------- date-family render ----

def _classify_month_tokens(tokens: list) -> list:
    """Given the ordered mask tokens (letter-runs + literal chars), decide each ``m``-run's role:
    a MINUTE if the nearest preceding mask token is ``h``/``hh`` or the nearest following mask token is
    ``s``/``ss`` (Excel's context rule); otherwise a MONTH. Returns a parallel list of role strings for
    ``m`` runs (``"month"``/``"minute"``), keyed by token index."""
    letter_positions = [
        i for i, t in enumerate(tokens) if t and t[0] in "ymdhsYMDHS"
    ]
    roles = {}
    for idx in letter_positions:
        tok = tokens[idx].lower()
        if tok[0] != "m":
            continue
        # nearest preceding letter token
        prev = None
        for j in letter_positions:
            if j < idx:
                prev = tokens[j].lower()
            else:
                break
        # nearest following letter token
        nxt = None
        for j in letter_positions:
            if j > idx:
                nxt = tokens[j].lower()
                break
        if (prev and prev[0] == "h") or (nxt and nxt[0] == "s"):
            roles[idx] = "minute"
        else:
            roles[idx] = "month"
    return roles


def _tokenize_mask(section: str) -> list:
    """Split a date/time section into a list of tokens: each maximal run of one mask letter is one
    token; every other char is its own single-char literal token. AM/PM is folded into a token."""
    tokens = []
    i = 0
    n = len(section)
    lower = section.lower()
    while i < n:
        # AM/PM marker
        if lower[i:i + 5] == "am/pm":
            tokens.append("AM/PM")
            i += 5
            continue
        if lower[i:i + 3] == "a/p":
            tokens.append("A/P")
            i += 3
            continue
        c = section[i]
        if c in "ymdhsYMDHS":
            j = i
            while j < n and section[j].lower() == c.lower():
                j += 1
            tokens.append(section[i:j])
            i = j
        else:
            tokens.append(c)
            i += 1
    return tokens


def _serial_to_datetime(section_tokens: list, serial: float) -> datetime.datetime:
    """Convert an Excel serial to a civil datetime, ROUNDING to the finest time unit the mask displays
    (seconds if ``s`` present, else minutes if a time field is present, else the whole day for a
    date-only mask) so a datetime like 44331.5625 with an ``h:mm`` mask lands exactly on 13:30."""
    has_seconds = any(t and t[0].lower() == "s" for t in section_tokens)
    roles = _classify_month_tokens(section_tokens)
    has_time = any(t and t[0].lower() == "h" for t in section_tokens) or any(
        r == "minute" for r in roles.values()
    )
    if has_seconds:
        total_seconds = round(serial * 86400)
    elif has_time:
        total_seconds = round(serial * 1440) * 60
    else:
        total_seconds = round(serial) * 86400
    return _EPOCH + datetime.timedelta(seconds=total_seconds)


def _render_date_code(code: str, serial: float) -> str:
    """Render a DATE/TIME code. Date/time codes have a single section in the accepted catalog."""
    section = _split_sections(code)[0]
    section = _strip_cosmetic(section)
    tokens = _tokenize_mask(section)
    roles = _classify_month_tokens(tokens)
    dt = _serial_to_datetime(tokens, serial)

    twelve_hour = any(t in ("AM/PM", "A/P") for t in tokens)
    out = []
    for idx, tok in enumerate(tokens):
        low = tok.lower()
        if tok in ("AM/PM", "A/P"):
            am = dt.hour < 12
            if tok == "AM/PM":
                out.append("AM" if am else "PM")
            else:
                out.append("A" if am else "P")
        elif low and low[0] == "y":
            out.append(f"{dt.year:04d}" if len(tok) >= 3 else f"{dt.year % 100:02d}")
        elif low and low[0] == "d":
            if len(tok) >= 4:
                out.append(dt.strftime("%A"))          # dddd -> weekday full
            elif len(tok) == 3:
                out.append(dt.strftime("%a"))          # ddd  -> weekday abbr
            elif len(tok) == 2:
                out.append(f"{dt.day:02d}")
            else:
                out.append(str(dt.day))
        elif low and low[0] == "m":
            if roles.get(idx) == "minute":
                out.append(f"{dt.minute:02d}" if len(tok) >= 2 else str(dt.minute))
            else:
                if len(tok) >= 4:
                    out.append(_MONTHS_FULL[dt.month - 1])
                elif len(tok) == 3:
                    out.append(_MONTHS_ABBR[dt.month - 1])
                elif len(tok) == 2:
                    out.append(f"{dt.month:02d}")
                else:
                    out.append(str(dt.month))
        elif low and low[0] == "h":
            hour = dt.hour
            if twelve_hour:
                hour = hour % 12
                if hour == 0:
                    hour = 12
            out.append(f"{hour:02d}" if len(tok) >= 2 else str(hour))
        elif low and low[0] == "s":
            out.append(f"{dt.second:02d}" if len(tok) >= 2 else str(dt.second))
        else:
            out.append(tok)
    return "".join(out)


# --------------------------------------------------------------------------------- public entry ----

def numfmt_render(format_code: str, value) -> str:
    """Render ``value`` through the accepted-catalog ``format_code`` to its Excel display string.

    ``value`` is numeric (an int/float/Decimal, or a ``datetime``/``date`` which is converted to its
    Excel serial). ``General`` and the empty code render the bare number. The exotic tail never reaches
    here (it is refused upstream), so this renderer need not handle conditional switches or masks.
    """
    if isinstance(value, datetime.datetime):
        value = (value - _EPOCH).total_seconds() / 86400.0
    elif isinstance(value, datetime.date):
        value = (datetime.datetime(value.year, value.month, value.day) - _EPOCH).days
    value = float(value)

    code = (format_code or "").strip()
    if code == "" or code.lower() == "general":
        # General: an integer prints without a trailing .0; a float prints minimally.
        return str(int(value)) if float(value).is_integer() else repr(value)

    if _is_datetime_code(code):
        return _render_date_code(code, value)
    return _render_number_code(code, value)


# --------------------------------------------------------------------------------------- selftest ---

def _selftest() -> int:
    """Assert this renderer reproduces every ECMA-376 golden vector (the external anchor). Any mismatch
    is a BUILD FAILURE — fix the renderer, never edit the golden (an FSA1 divergence is an FSA1
    bug). The Rust leg (conformance/tests/golden_numfmt.rs) binds fsa1-ast's renderer to the SAME
    vectors, so the two in-house renderers cannot pass by agreeing only with each other."""
    golden = json.loads((HERE / "golden_numfmt.json").read_text(encoding="utf-8"))
    vectors = golden["vectors"]
    assert len(vectors) == golden["count"] == 12, (
        f"expected 12 golden vectors, found {len(vectors)} / count={golden['count']}"
    )
    failures = []
    for v in vectors:
        got = numfmt_render(v["format"], v["value"])
        status = "OK  " if got == v["expected"] else "FAIL"
        if got != v["expected"]:
            failures.append((v["id"], v["format"], v["value"], v["expected"], got))
        print(f"  [{status}] #{v['id']:>2} {v['category']:<17} {v['format']!r:<26} "
              f"{v['value']!r:>12} -> {got!r}  (golden {v['expected']!r})")
    if failures:
        print(f"\nnumfmt_render SELFTEST FAILED: {len(failures)} vector(s) diverge from the golden set")
        for vid, code, val, exp, got in failures:
            print(f"  #{vid}: numfmt_render({code!r}, {val!r}) = {got!r}, golden = {exp!r}")
        return 1
    print(f"\nnumfmt_render SELFTEST PASS — all {len(vectors)} ECMA-376 golden vectors reproduced")
    return 0


if __name__ == "__main__":
    if len(sys.argv) == 2 and sys.argv[1] == "--selftest":
        sys.exit(_selftest())
    if len(sys.argv) == 3:
        print(numfmt_render(sys.argv[1], float(sys.argv[2])))
        sys.exit(0)
    print("usage: numfmt_render.py --selftest | <format_code> <value>", file=sys.stderr)
    sys.exit(2)
