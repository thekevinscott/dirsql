import type { Element } from 'hast';
import { fromHtml } from 'hast-util-from-html';
import { fromMarkdown } from 'mdast-util-from-markdown';

export interface Code {
  type: 'code';
  language: string | undefined;
  content: string;
  path: number[];
}

export interface Data {
  type: 'data';
  name: string;
  language: string | undefined;
  source: string;
  path: number[];
}

export type Item = Code | Data;

const dataTag = (html: string): Element | undefined =>
  fromHtml(html, { fragment: true }).children.find(
    (node): node is Element => node.type === 'element' && node.tagName === 'data',
  );

const text = (element: Element): string =>
  element.children.map(child => (child.type === 'text' ? child.value : '')).join('');

const attribute = (element: Element, key: string): string | undefined => {
  const value = element.properties[key];
  return typeof value === 'string' ? value : undefined;
};

export const readDocument = (markdown: string): Item[] => {
  const items: Item[] = [];
  const path: number[] = [];
  let open: Element | undefined;

  fromMarkdown(markdown).children.forEach((node, index) => {
    if (node.type === 'heading') {
      path.length = node.depth - 1;
      for (let depth = 0; depth < node.depth - 1; depth++) {
        path[depth] ??= -1;
      }
      path.push(index);
      return;
    }
    // A one-line <data>value</data> is a paragraph in markdown, not an html block.
    const raw = markdown.slice(node.position?.start.offset, node.position?.end.offset);
    if (node.type === 'html' || (node.type === 'paragraph' && raw.startsWith('<data'))) {
      if (/^\s*<\/data>/.test(raw)) {
        open = undefined;
        return;
      }
      const tag = dataTag(raw);
      if (tag === undefined) {
        return;
      }
      const name = attribute(tag, 'name');
      if (name === undefined) {
        throw new Error(`A <data> tag on line ${node.position?.start.line} has no name`);
      }
      if (raw.includes('</data>')) {
        items.push({ type: 'data', name, language: attribute(tag, 'language'), source: text(tag).trim(), path: [...path] });
      } else {
        open = tag;
      }
      return;
    }
    if (node.type !== 'code') {
      return;
    }
    const language = node.lang ?? undefined;
    if (open === undefined) {
      items.push({ type: 'code', language, content: node.value, path: [...path] });
      return;
    }
    items.push({ type: 'data', name: attribute(open, 'name')!, language, source: node.value, path: [...path] });
  });

  return items;
};
