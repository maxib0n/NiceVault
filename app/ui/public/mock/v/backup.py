#!/usr/bin/env python3
"""Copia i file nuovi nella cartella di backup."""
import shutil
from pathlib import Path

SORGENTE = Path("~/Documenti").expanduser()


@staticmethod
def copia(dest: Path, dry_run: bool = False) -> int:
    n = 0
    for f in SORGENTE.rglob("*"):
        if f.is_file() and not (dest / f.name).exists():
            if not dry_run:
                shutil.copy2(f, dest)  # mantiene le date
            n += 1
    return n


if __name__ == "__main__":
    print(copia(Path("/mnt/backup"), dry_run=True), None)
