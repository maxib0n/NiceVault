import type { ApiError, EventMap, EventName, FsNode, NodeId, SearchFilters, SearchHit, SearchResult, SearchSort, Tag, TagId, VaultId } from './types';
import { ROOT } from './types';
import { switches } from './switches';
import { DEMO_TAGS } from './mockDemo';
import { MAX_TAG_NAME, TAG_COLORS, fold, highlights, matchesFilters, naturalCmp, rank, terms } from './search';

export interface SearchMockHost {
  volume(vault: VaultId): { path: string; nodes: Map<NodeId, FsNode> };
  seeded(path: string): boolean;
  wait(ms: number): Promise<void>;
  emit<E extends EventName>(event: E, payload: EventMap[E]): void;
}

interface StoredTag {
  id: TagId;
  name: string;
  norm: string;
  color: string;
}

interface VolumeTags {
  tags: Map<TagId, StoredTag>;
  nextId: number;
  assigned: Map<NodeId, Set<TagId>>;
}

const MAX_TAGS = 1000;

function err(code: string, message: string): ApiError {
  return { code, message };
}

const tagNorm = (name: string) => fold(name).split(/\s+/).filter(Boolean).join(' ');

const SEED: [string, string, string[]][] = [
  ['Importante', 'velluto', ['Contratto affitto.pdf', 'Passaporto (scansione).jpg', 'Buste']],
  ['Vacanze', 'ottanio', ['Vacanze 2024', 'Lago di Braies.jpg', 'Dolomiti 01.jpg', 'Tramonto sul lago.jpg']],
  ['Famiglia', 'bottiglia', ['Compleanno Giulia', 'Nonna e nonno.jpg', 'IMG_2042.jpg']],
  ['Da rivedere', 'seppia', ['Fattura 2024-031.pdf', 'IMG_2044.jpg']],
  ['Lavoro', 'prussia', ['Budget 2025.xlsx', 'Presentazione Q3.pptx', 'Note riunione.md']],
  ['Nuovo', 'neutral', []],
];

export class SearchMock {
  private vols = new Map<string, VolumeTags>();

  constructor(private host: SearchMockHost) {}

  private state(path: string, nodes: Map<NodeId, FsNode>): VolumeTags {
    let s = this.vols.get(path);
    if (!s) {
      s = { tags: new Map(), nextId: 1, assigned: new Map() };
      this.vols.set(path, s);
      if (this.host.seeded(path)) {
        for (const [name, color, targets] of switches.demo ? DEMO_TAGS : SEED) {
          const id = s.nextId++;
          s.tags.set(id, { id, name, norm: tagNorm(name), color });
          for (const n of nodes.values()) {
            if (n.id !== ROOT && targets.includes(n.name)) {
              let set = s.assigned.get(n.id);
              if (!set) s.assigned.set(n.id, (set = new Set()));
              set.add(id);
            }
          }
        }
      }
    }
    for (const id of [...s.assigned.keys()]) if (!nodes.has(id)) s.assigned.delete(id);
    return s;
  }

  private open(vault: VaultId) {
    const v = this.host.volume(vault);
    return { ...v, s: this.state(v.path, v.nodes) };
  }

  tagsOf(vault: VaultId, id: NodeId): TagId[] {
    const { s } = this.open(vault);
    return [...(s.assigned.get(id) ?? [])].sort((a, b) => a - b);
  }

  private list(s: VolumeTags): Tag[] {
    const count = new Map<TagId, number>();
    for (const set of s.assigned.values()) for (const t of set) count.set(t, (count.get(t) ?? 0) + 1);
    return [...s.tags.values()]
      .sort((a, b) => (a.norm < b.norm ? -1 : a.norm > b.norm ? 1 : a.id - b.id))
      .map((t) => ({ id: t.id, name: t.name, color: t.color, count: count.get(t.id) ?? 0 }));
  }

  private one(s: VolumeTags, id: TagId): Tag {
    const t = this.list(s).find((x) => x.id === id);
    if (!t) throw err('not_found', 'Tag not found.');
    return t;
  }

  private validName(name: unknown): string {
    const n = String(name ?? '').trim();
    // eslint-disable-next-line no-control-regex
    if (!n || [...n].length > MAX_TAG_NAME || /[\u0000-\u001f\u007f-\u009f]/.test(n)) {
      throw err('invalid_name', `Invalid tag name (1 to ${MAX_TAG_NAME} characters, no control characters).`);
    }
    return n;
  }

  private validColor(c: unknown): string {
    if (!(TAG_COLORS as readonly string[]).includes(String(c))) throw err('invalid_color', 'Unknown tag colour.');
    return String(c);
  }

  private changed(vault: VaultId, nodes: Map<NodeId, FsNode>, changed: NodeId[]) {
    const dirs = [...new Set(changed.map((n) => nodes.get(n)?.parent).filter((d): d is NodeId => d !== undefined))].sort((a, b) => a - b);
    this.host.emit('tags-changed', { vault, nodes: changed, dirs });
  }

  private pathOf(nodes: Map<NodeId, FsNode>, dir: NodeId): string {
    const parts: string[] = [];
    for (let cur = dir, i = 0; cur !== ROOT && i < 4096; i++) {
      const n = nodes.get(cur);
      if (!n) break;
      parts.unshift(n.name);
      cur = n.parent;
    }
    return '/' + parts.join('/');
  }

  private search(vault: VaultId, a: Record<string, unknown>): SearchResult {
    const { nodes, s } = this.open(vault);
    const query = String(a.query ?? '');
    if (query.length > 1024) throw err('invalid_query', 'The search text is too long.');
    const sortIn = (a.sort ?? 'relevance') as SearchSort;
    if (!['relevance', 'name', 'size', 'mtime', 'type'].includes(sortIn)) throw err('invalid_query', 'Unknown sort order.');
    const cursor = a.cursor == null || a.cursor === '' ? '' : String(a.cursor);
    if (cursor && !/^o\d+$/.test(cursor)) throw err('invalid_query', 'Invalid cursor.');
    const offset = cursor ? Number(cursor.slice(1)) : 0;
    const limit = Math.min(1000, Math.max(1, Number(a.limit ?? 100) || 100));
    const desc = Boolean(a.desc);
    const f = (a.filters ?? null) as SearchFilters | null;
    const ts = terms(query);
    const within = f?.within != null && f.within !== ROOT ? f.within : null;
    const inside = (id: NodeId): boolean => {
      for (let cur = id, i = 0; i < 4096; i++) {
        if (cur === within) return true;
        if (cur === ROOT) return false;
        const n = nodes.get(cur);
        if (!n) return false;
        cur = n.parent;
      }
      return false;
    };
    const found: { n: FsNode; norm: string; r: number; tags: TagId[] }[] = [];
    for (const n of nodes.values()) {
      if (n.id === ROOT) continue;
      const norm = fold(n.name);
      if (!ts.every((t) => norm.includes(t))) continue;
      const tags = [...(s.assigned.get(n.id) ?? [])].sort((x, y) => x - y);
      if (!matchesFilters(n, tags, f)) continue;
      if (within !== null && (n.id === within || !inside(n.parent))) continue;
      found.push({ n, norm, r: ts.length ? rank(norm, ts) : 0, tags });
    }
    const sort = !ts.length && sortIn === 'relevance' ? 'name' : sortIn;
    const ext = (norm: string) => {
      const i = norm.lastIndexOf('.');
      return i > 0 ? norm.slice(i + 1) : '';
    };
    found.sort((x, y) => {
      const dx = x.n.kind === 'dir';
      const dy = y.n.kind === 'dir';
      const byName = () => naturalCmp(x.norm, y.norm) || (x.n.name < y.n.name ? -1 : x.n.name > y.n.name ? 1 : 0) || x.n.id - y.n.id;
      let r: number;
      if (sort === 'relevance') r = x.r - y.r || (dx === dy ? 0 : dx ? -1 : 1) || byName();
      else {
        if (dx !== dy) return dx ? -1 : 1;
        if (sort === 'size') r = x.n.size - y.n.size || byName();
        else if (sort === 'mtime') r = x.n.mtime - y.n.mtime || byName();
        else if (sort === 'type') r = naturalCmp(ext(x.norm), ext(y.norm)) || byName();
        else r = byName();
      }
      return desc ? -r : r;
    });
    const page = found.slice(offset, offset + limit);
    const end = offset + limit;
    const hits: SearchHit[] = page.map(({ n, tags }) => ({
      node: { id: n.id, parent: n.parent, name: n.name, kind: n.kind, size: n.size, mtime: n.mtime },
      path: this.pathOf(nodes, n.parent),
      highlights: highlights(n.name, ts),
      tags,
    }));
    return { hits, total: found.length, next_cursor: end < found.length ? `o${end}` : null, pending: 0 };
  }

  async invoke(cmd: string, a: Record<string, unknown>): Promise<unknown> {
    const vault = String(a.vault);
    switch (cmd) {
      case 'fs_search':
        await this.host.wait(8);
        return this.search(vault, a);
      case 'tags_list': {
        await this.host.wait(5);
        return this.list(this.open(vault).s);
      }
      case 'tags_create': {
        const { nodes, s } = this.open(vault);
        const name = this.validName(a.name);
        const color = this.validColor(a.color);
        if (s.tags.size >= MAX_TAGS) throw err('too_many_tags', `There are already ${MAX_TAGS} tags.`);
        const norm = tagNorm(name);
        if ([...s.tags.values()].some((t) => t.norm === norm)) throw err('exists', 'A tag with this name already exists.');
        const id = s.nextId++;
        s.tags.set(id, { id, name, norm, color });
        this.changed(vault, nodes, []);
        return this.one(s, id);
      }
      case 'tags_update': {
        const { nodes, s } = this.open(vault);
        const name = a.name == null ? null : this.validName(a.name);
        const color = a.color == null ? null : this.validColor(a.color);
        const t = s.tags.get(Number(a.tag));
        if (!t) throw err('not_found', 'Tag not found.');
        if (name !== null) {
          const norm = tagNorm(name);
          if ([...s.tags.values()].some((x) => x.norm === norm && x.id !== t.id)) throw err('exists', 'A tag with this name already exists.');
          t.name = name;
          t.norm = norm;
        }
        if (color !== null) t.color = color;
        this.changed(vault, nodes, []);
        return this.one(s, t.id);
      }
      case 'tags_delete': {
        const { nodes, s } = this.open(vault);
        const id = Number(a.tag);
        if (!s.tags.has(id)) throw err('not_found', 'Tag not found.');
        const had: NodeId[] = [];
        for (const [n, set] of s.assigned) {
          if (set.delete(id)) had.push(n);
          if (!set.size) s.assigned.delete(n);
        }
        s.tags.delete(id);
        this.changed(vault, nodes, had.sort((x, y) => x - y));
        return null;
      }
      case 'tags_assign':
      case 'tags_unassign': {
        const { nodes, s } = this.open(vault);
        const tags = ((a.tags ?? []) as TagId[]).map(Number);
        for (const t of tags) if (!s.tags.has(t)) throw err('not_found', 'Tag not found.');
        const on = cmd === 'tags_assign';
        const changed = new Set<NodeId>();
        for (const n of ((a.nodes ?? []) as NodeId[]).map(Number)) {
          if (n === ROOT || !nodes.has(n)) continue;
          let set = s.assigned.get(n);
          for (const t of tags) {
            if (on) {
              if (!set) s.assigned.set(n, (set = new Set()));
              if (!set.has(t)) {
                set.add(t);
                changed.add(n);
              }
            } else if (set?.delete(t)) changed.add(n);
          }
          if (set && !set.size) s.assigned.delete(n);
        }
        if (changed.size) this.changed(vault, nodes, [...changed].sort((x, y) => x - y));
        return this.list(s);
      }
      case 'tags_of': {
        const { s } = this.open(vault);
        const out: Record<string, TagId[]> = {};
        for (const n of ((a.nodes ?? []) as NodeId[]).map(Number)) {
          const set = s.assigned.get(n);
          if (set?.size) out[String(n)] = [...set].sort((x, y) => x - y);
        }
        return out;
      }
      default:
        throw err('unknown_command', `Unknown command: ${cmd}`);
    }
  }
}
