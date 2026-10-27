// @vitest-environment jsdom
import { describe, expect, it } from 'vitest';
import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { localImagePath, renderMarkdown, resolveVolumePath, sanitizeSvg, sanitizeMath, splitFrontMatter } from './markdown';

const sample = (n: string) => readFileSync(resolve(__dirname, '../../public/mock/v', n), 'utf8');
const dom = (html: string) => {
  const d = document.createElement('div');
  d.innerHTML = html;
  return d;
};

describe('markdown renderer', () => {
  it('the rich sample: every feature', () => {
    const r = renderMarkdown(sample('taccuino.md'));
    const d = dom(r.html);
    expect(r.front).toContainEqual(['title', 'Taccuino di casa']);
    expect(d.textContent).not.toContain('title: Taccuino');
    expect(d.querySelector('h1')?.id).toMatch(/^md-/);
    expect(r.toc.length).toBeGreaterThan(4);
    expect(d.querySelectorAll('table th').length).toBeGreaterThan(1);
    expect(d.querySelector('th.md-al-right, td.md-al-right')).not.toBeNull();
    expect(d.querySelectorAll('li.md-task').length).toBeGreaterThanOrEqual(2);
    expect(d.querySelector('.md-box[data-done="1"]')).not.toBeNull();
    expect(d.querySelector('section.footnotes li')).not.toBeNull();
    expect(d.querySelector('sup.footnote-ref a')?.getAttribute('href')).toMatch(/^#fn-md-/);
    expect(d.querySelector('dl dt')).not.toBeNull();
    expect(d.querySelector('mark')).not.toBeNull();
    expect(d.querySelector('sub')).not.toBeNull();
    expect(d.querySelector('sup:not(.footnote-ref)')).not.toBeNull();
    expect(d.querySelector('abbr[title]')).not.toBeNull();
    expect(d.querySelector('del, s')).not.toBeNull();
    expect(d.textContent).toContain('🎉');
    expect(d.querySelector('blockquote.md-alert-warning .md-alert-title')).not.toBeNull();
    expect(d.querySelector('pre.md-code .t-kw')).not.toBeNull();
    expect(r.math).toBe(true);
    expect(d.querySelector('.md-math[data-display="0"]')).not.toBeNull();
    expect(d.querySelector('.md-math[data-display="1"]')).not.toBeNull();
    expect(r.mermaid).toBe(true);
    expect(d.querySelector('.md-mermaid pre')?.textContent).toContain('flowchart');
    expect(d.querySelector('.md-img-local')?.getAttribute('data-path')).toBe('Lago.webp');
  });

  it('the malicious sample: nothing can run, load or navigate', () => {
    const r = renderMarkdown(sample('sospetto.md'));
    const d = dom(r.html);
    for (const sel of ['script', 'iframe', 'img', 'form', 'input', 'object', 'embed', 'svg', 'style', 'video', 'audio', 'link', 'meta', 'base'])
      expect(d.querySelector(sel), sel).toBeNull();
    for (const el of d.querySelectorAll('*')) {
      for (const a of el.getAttributeNames()) {
        expect(a.startsWith('on'), a).toBe(false);
        expect(a).not.toBe('style');
      }
    }
    for (const a of d.querySelectorAll('a')) expect(a.getAttribute('href')).toMatch(/^(https?:\/\/|mailto:|#)/);
    expect(d.textContent).toContain('<script>');
    expect(d.textContent).toContain('<img src=x onerror=alert(1)>');
  });

  it('raw HTML stays text, javascript: links are dead text', () => {
    const d = dom(renderMarkdown('<b>x</b> [a](javascript:alert(1)) [b](https://ok.example/p) <mailto:a@b.it> [c](data:text/html,x) [d](#md-x)').html);
    expect(d.querySelector('b')).toBeNull();
    expect(d.textContent).toContain('<b>x</b>');
    expect([...d.querySelectorAll('a')].map((a) => a.getAttribute('href'))).toEqual(['https://ok.example/p', 'mailto:a@b.it', '#md-x']);
    expect(d.querySelectorAll('.md-deadlink')).toHaveLength(2);
  });

  it('images are placeholders: remote and data: never loaded, relative ones marked', () => {
    const d = dom(renderMarkdown('![r](https://t.example/p.png) ![d](data:image/png;base64,AAAA) ![l](Foto/a%20b.jpg) ![p](//cdn.example/x.png)').html);
    expect(d.querySelector('img')).toBeNull();
    expect([...d.querySelectorAll('.md-img-local')].map((e) => e.getAttribute('data-path'))).toEqual(['Foto/a b.jpg']);
    expect(d.querySelectorAll('.md-img')).toHaveLength(4);
  });

  it('math: $…$ inline, $$ block, money is not math', () => {
    const r = renderMarkdown('Costa $5 e $10.\n\nInline $a^2+b^2$ qui.\n\n$$\n\\int_0^1 x\\,dx\n$$');
    const d = dom(r.html);
    expect(r.math).toBe(true);
    expect([...d.querySelectorAll('.md-math')].map((e) => e.textContent)).toEqual(['a^2+b^2', '\\int_0^1 x\\,dx']);
    expect(d.textContent).toContain('$5 e $10');
    expect(renderMarkdown('just $5').math).toBe(false);
  });

  it('mermaid fence detected; other fences highlighted', () => {
    expect(renderMarkdown('```mermaid\ngraph TD; A-->B\n```').mermaid).toBe(true);
    const r = renderMarkdown('```rust\nfn main() {}\n```');
    expect(r.mermaid).toBe(false);
    expect(dom(r.html).querySelector('pre[data-lang="rust"] .t-kw')?.textContent).toBe('fn');
  });

  it('front matter, alerts, task lists, tables', () => {
    expect(splitFrontMatter('---\ntitle: "A"\ntags:\n  - x\n  - y\n---\n# T').front).toEqual([
      ['title', 'A'],
      ['tags', 'x, y'],
    ]);
    const d = dom(renderMarkdown('> [!NOTE]\n> Hello\n\n- [x] done\n- [ ] todo\n\n| a | b |\n|:-|-:|\n| 1 | 2 |').html);
    expect(d.querySelector('blockquote.md-alert-note')?.textContent).toContain('Hello');
    expect(d.querySelector('blockquote')?.textContent).not.toContain('[!NOTE]');
    expect(d.querySelectorAll('li.md-task')).toHaveLength(2);
    expect(d.querySelector('td.md-al-right')?.textContent).toBe('2');
  });

  it('volume paths', () => {
    expect(localImagePath('../Foto/x.png?v=1')).toBe('../Foto/x.png');
    expect(localImagePath('https://x/y.png')).toBeNull();
    expect(localImagePath('file:///etc/passwd')).toBeNull();
    expect(resolveVolumePath('/A/B', '../C/x.png')).toEqual(['A', 'C', 'x.png']);
    expect(resolveVolumePath('/A', '../../x.png')).toBeNull();
    expect(resolveVolumePath('/A', '/R/x.png')).toEqual(['R', 'x.png']);
  });

  it('KaTeX / Mermaid output is sanitized again', () => {
    expect(sanitizeSvg('<svg><script>alert(1)</script><a href="javascript:x"><text>t</text></a><foreignObject><div>h</div></foreignObject><rect onload="x"/></svg>')).not.toMatch(/script|javascript|foreignObject|onload/);
    expect(sanitizeMath('<span style="height:1em" onclick="x"><a href="https://x">y</a><img src=x></span>')).toBe('<span style="height:1em">y</span>');
  });
});
