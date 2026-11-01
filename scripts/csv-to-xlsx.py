"""Bundle an export directory's CSV tables into one workbook, a sheet per table.

    uv run --with openpyxl python scripts/csv-to-xlsx.py exports/<build>

Numbers stay numbers, the header row is frozen and filterable, and columns are
sized to their contents, so the workbook imports cleanly into chart tools.
"""

import csv
import sys
from pathlib import Path

from openpyxl import Workbook
from openpyxl.styles import Font
from openpyxl.utils import get_column_letter


def typed(value: str):
    if value == "":
        return None
    try:
        number = float(value)
    except ValueError:
        return value
    return int(number) if number.is_integer() and "." not in value else number


def main() -> None:
    folder = Path(sys.argv[1] if len(sys.argv) > 1 else ".")
    tables = sorted(folder.glob("*.csv"))
    if not tables:
        sys.exit(f"no CSV files in {folder}")

    book = Workbook()
    book.remove(book.active)
    for table in tables:
        sheet = book.create_sheet(table.stem[:31])
        with table.open(newline="", encoding="utf-8") as handle:
            for index, row in enumerate(csv.reader(handle)):
                sheet.append(row if index == 0 else [typed(v) for v in row])
        for cell in sheet[1]:
            cell.font = Font(bold=True)
        sheet.freeze_panes = "A2"
        sheet.auto_filter.ref = sheet.dimensions
        for column in sheet.columns:
            width = max(len(str(c.value)) if c.value is not None else 0 for c in column)
            sheet.column_dimensions[get_column_letter(column[0].column)].width = min(max(width + 2, 8), 48)

    out = folder / f"sc-combat-{folder.name}.xlsx"
    book.save(out)
    print(f"wrote {out} with {len(tables)} sheets")


if __name__ == "__main__":
    main()
