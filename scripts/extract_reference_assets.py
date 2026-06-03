from __future__ import annotations

from pathlib import Path


def convert_text(source: Path, target: Path, encoding: str):
    target.parent.mkdir(parents=True, exist_ok=True)
    text = source.read_text(encoding=encoding)
    target.write_text(text, encoding="utf-8", newline="\n")
    print(f"wrote {target}")


def main():
    convert_text(
        Path(".reference/pyCO2module/info.csv"),
        Path("assets/info.csv"),
        "cp949",
    )
    convert_text(
        Path(".reference/pyCO2module/Umap.csv"),
        Path("assets/Umap.csv"),
        "utf-8-sig",
    )


if __name__ == "__main__":
    main()

