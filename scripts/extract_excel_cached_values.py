from __future__ import annotations

import argparse
import json
import re
import zipfile
from pathlib import Path
from typing import Iterable
from xml.etree import ElementTree as ET


NS = {
    "main": "http://schemas.openxmlformats.org/spreadsheetml/2006/main",
    "rel": "http://schemas.openxmlformats.org/officeDocument/2006/relationships",
    "pkgrel": "http://schemas.openxmlformats.org/package/2006/relationships",
}

CELL_RE = re.compile(r"^([A-Z]+)([0-9]+)$")


def column_to_index(column: str) -> int:
    value = 0
    for char in column:
        value = value * 26 + (ord(char) - ord("A") + 1)
    return value


def parse_cell_ref(ref: str) -> tuple[int, int]:
    match = CELL_RE.match(ref.upper())
    if not match:
        raise ValueError(f"invalid cell reference: {ref}")
    column, row = match.groups()
    return int(row), column_to_index(column)


def parse_range(range_ref: str) -> tuple[str, int, int, int, int]:
    if "!" not in range_ref:
        raise ValueError(f"range must include sheet name: {range_ref}")
    sheet, cells = range_ref.split("!", 1)
    start, end = cells.split(":", 1) if ":" in cells else (cells, cells)
    start_row, start_col = parse_cell_ref(start)
    end_row, end_col = parse_cell_ref(end)
    return (
        sheet.strip("'"),
        min(start_row, end_row),
        max(start_row, end_row),
        min(start_col, end_col),
        max(start_col, end_col),
    )


def qname(local: str) -> str:
    return f"{{{NS['main']}}}{local}"


def rel_qname(local: str) -> str:
    return f"{{{NS['rel']}}}{local}"


def relationship_type(local: str) -> str:
    return f"{NS['rel']}/{local}"


def load_shared_strings(xlsx: zipfile.ZipFile) -> list[str]:
    if "xl/sharedStrings.xml" not in xlsx.namelist():
        return []

    values: list[str] = []
    with xlsx.open("xl/sharedStrings.xml") as source:
        for _, elem in ET.iterparse(source, events=("end",)):
            if elem.tag == qname("si"):
                text = "".join(t.text or "" for t in elem.iter(qname("t")))
                values.append(text)
                elem.clear()
    return values


def load_sheet_paths(xlsx: zipfile.ZipFile) -> dict[str, str]:
    workbook = ET.fromstring(xlsx.read("xl/workbook.xml"))
    relationships = ET.fromstring(xlsx.read("xl/_rels/workbook.xml.rels"))
    rel_targets = {
        rel.attrib["Id"]: rel.attrib["Target"].lstrip("/")
        for rel in relationships
        if rel.attrib.get("Type") == relationship_type("worksheet")
    }

    sheet_paths = {}
    for sheet in workbook.findall("main:sheets/main:sheet", NS):
        name = sheet.attrib["name"]
        rel_id = sheet.attrib[rel_qname("id")]
        target = rel_targets[rel_id]
        sheet_paths[name] = target if target.startswith("xl/") else f"xl/{target}"
    return sheet_paths


def cell_value(cell: ET.Element, shared_strings: list[str]):
    value_elem = cell.find("main:v", NS)
    if value_elem is None:
        inline = cell.find("main:is", NS)
        if inline is not None:
            return "".join(t.text or "" for t in inline.iter(qname("t")))
        return None

    raw = value_elem.text or ""
    cell_type = cell.attrib.get("t")
    if cell_type == "s":
        return shared_strings[int(raw)]
    if cell_type == "b":
        return raw == "1"
    if cell_type in {"str", "inlineStr"}:
        return raw
    try:
        number = float(raw)
        return int(number) if number.is_integer() else number
    except ValueError:
        return raw


def extract_range(
    xlsx: zipfile.ZipFile,
    sheet_path: str,
    bounds: tuple[int, int, int, int],
    shared_strings: list[str],
) -> dict[str, object]:
    min_row, max_row, min_col, max_col = bounds
    values: dict[str, object] = {}

    with xlsx.open(sheet_path) as source:
        for _, elem in ET.iterparse(source, events=("end",)):
            if elem.tag != qname("c"):
                continue
            ref = elem.attrib.get("r")
            if not ref:
                elem.clear()
                continue
            row, col = parse_cell_ref(ref)
            if min_row <= row <= max_row and min_col <= col <= max_col:
                values[ref] = cell_value(elem, shared_strings)
            elem.clear()

    return values


def list_sheets(workbook: Path) -> Iterable[str]:
    with zipfile.ZipFile(workbook) as xlsx:
        return load_sheet_paths(xlsx).keys()


def main() -> None:
    parser = argparse.ArgumentParser(
        description="Extract cached values from an XLSX workbook without opening Excel."
    )
    parser.add_argument(
        "--workbook",
        default=".reference/02 Excel기반 에너지 감축계수 조회 tool.xlsx",
        help="XLSX workbook path",
    )
    parser.add_argument("--list-sheets", action="store_true")
    parser.add_argument(
        "--range",
        action="append",
        default=[],
        help="Sheet-qualified A1 range, e.g. MAIN!J8:O11. Can be repeated.",
    )
    parser.add_argument("--out", default="", help="Optional JSON output path")
    args = parser.parse_args()

    workbook = Path(args.workbook)
    if args.list_sheets:
        output = {"sheets": list(list_sheets(workbook))}
    else:
        if not args.range:
            parser.error("provide --list-sheets or at least one --range")

        output = {"workbook": str(workbook), "ranges": {}}
        with zipfile.ZipFile(workbook) as xlsx:
            sheet_paths = load_sheet_paths(xlsx)
            shared_strings = load_shared_strings(xlsx)
            for range_ref in args.range:
                sheet, min_row, max_row, min_col, max_col = parse_range(range_ref)
                if sheet not in sheet_paths:
                    raise KeyError(f"sheet not found: {sheet}")
                output["ranges"][range_ref] = extract_range(
                    xlsx,
                    sheet_paths[sheet],
                    (min_row, max_row, min_col, max_col),
                    shared_strings,
                )

    text = json.dumps(output, ensure_ascii=False, indent=2)
    if args.out:
        Path(args.out).write_text(text + "\n", encoding="utf-8")
    else:
        print(text)


if __name__ == "__main__":
    main()
