import { readFileSync, readdirSync } from 'node:fs';
import { dirname, join, relative } from 'node:path';
import { fileURLToPath } from 'node:url';
import { parse } from 'postcss';
import { describe, expect, it } from 'vitest';

const sourceRoot = fileURLToPath(new URL('../src', import.meta.url));
const tokenPath = join(sourceRoot, 'tokens.css');
const tokens = new Map<string, string>();
parse(readFileSync(tokenPath, 'utf8')).walkDecls((declaration) => {
  if (declaration.prop.startsWith('--')) tokens.set(declaration.prop, declaration.value);
});

function cssFiles(directory: string): string[] {
  return readdirSync(directory, { withFileTypes: true }).flatMap((entry) => {
    const path = join(directory, entry.name);
    return entry.isDirectory() ? cssFiles(path) : entry.name.endsWith('.css') ? [path] : [];
  });
}

describe('application design system', () => {
  it('defines a readable scale with a distinct role for labels, content, resource names and headings', () => {
    const scale = [
      ['--text-caption', 13], ['--text-small', 14], ['--text-body', 16],
      ['--text-title', 18], ['--text-section', 20], ['--text-page', 28],
    ] as const;
    let previous = 0;
    for (const [name, minimum] of scale) {
      const value = tokens.get(name);
      expect(value, `${name} is a pixel based type token`).toMatch(/^\d+(?:\.\d+)?px$/);
      const size = parseFloat(value!);
      expect(size, `${name} must remain legible`).toBeGreaterThanOrEqual(minimum);
      expect(size, `${name} must preserve the reading hierarchy`).toBeGreaterThan(previous);
      previous = size;
    }
    expect(parseFloat(tokens.get('--control-height') ?? ''), 'full size inputs and toolbar actions').toBeGreaterThanOrEqual(44);
    expect(parseFloat(tokens.get('--control-height-sm') ?? ''), 'compact row actions').toBeGreaterThanOrEqual(36);
  });

  it('uses the shared type, weight and leading tokens in every application stylesheet', () => {
    const problems: string[] = [];
    const properties = new Map([
      ['font-size', '--text-'], ['font-weight', '--weight-'], ['line-height', '--leading-'],
    ]);
    for (const path of cssFiles(sourceRoot)) {
      if (path === tokenPath) continue;
      // Generated model HTML is isolated in an iframe and is not an application
      // stylesheet. Its content is intentionally outside this typography contract.
      parse(readFileSync(path, 'utf8'), { from: path }).walkDecls((declaration) => {
        const prefix = properties.get(declaration.prop);
        if (!prefix && declaration.prop !== 'font') return;
        const value = declaration.value.trim();
        if (/^(inherit|initial|unset|revert|revert-layer|normal|0)$/.test(value)) return;
        const token = /^var\((--[a-z0-9-]+)\)$/.exec(value)?.[1];
        if (token && prefix && token.startsWith(prefix) && tokens.has(token)) return;
        problems.push(`${relative(dirname(sourceRoot), path)}:${declaration.source?.start?.line} ${declaration.prop}: ${value}`);
      });
    }
    expect(problems, 'Do not introduce per-page typography or an undefined semantic token').toEqual([]);
  });
});
