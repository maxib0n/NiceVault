// Somma le spese per categoria.
export interface Spesa {
  categoria: string;
  importo: number;
}

export function totali(spese: Spesa[]): Map<string, number> {
  const out = new Map<string, number>();
  for (const s of spese) {
    out.set(s.categoria, (out.get(s.categoria) ?? 0) + s.importo);
  }
  return out;
}

const esempio = totali([{ categoria: 'casa', importo: 120.5 }, { categoria: 'auto', importo: 40 }]);
console.log(`categorie: ${esempio.size}`, null, undefined);
