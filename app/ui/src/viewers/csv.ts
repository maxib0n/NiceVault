export const CSV_MAX_ROWS = 2000;
export const CSV_MAX_COLS = 60;

export function guessDelimiter(firstLine: string): string {
  const count = (c: string) => firstLine.split(c).length - 1;
  const cands = [',', ';', '\t', '|'].map((c) => [c, count(c)] as const).sort((a, b) => b[1] - a[1]);
  return cands[0][1] > 0 ? cands[0][0] : ',';
}

export function parseCsv(text: string, delimiter: string | null = null): { rows: string[][]; truncated: boolean } {
  const nl = text.indexOf('\n');
  const d = delimiter ?? guessDelimiter(nl < 0 ? text : text.slice(0, nl));
  const rows: string[][] = [];
  let row: string[] = [];
  let cell = '';
  let quoted = false;
  let i = 0;
  const n = text.length;
  const endRow = () => {
    row.push(cell);
    cell = '';
    if (row.length > CSV_MAX_COLS) row.length = CSV_MAX_COLS;
    if (!(row.length === 1 && row[0] === '')) rows.push(row);
    row = [];
  };
  while (i < n) {
    const ch = text[i];
    if (quoted) {
      if (ch === '"') {
        if (text[i + 1] === '"') {
          cell += '"';
          i += 2;
          continue;
        }
        quoted = false;
        i++;
        continue;
      }
      cell += ch;
      i++;
      continue;
    }
    if (ch === '"' && cell === '') {
      quoted = true;
      i++;
    } else if (ch === d) {
      row.push(cell);
      cell = '';
      i++;
    } else if (ch === '\n' || ch === '\r') {
      endRow();
      if (ch === '\r' && text[i + 1] === '\n') i++;
      i++;
      if (rows.length >= CSV_MAX_ROWS) return { rows, truncated: i < n };
    } else {
      cell += ch;
      i++;
    }
  }
  if (cell !== '' || row.length) endRow();
  return { rows, truncated: false };
}
