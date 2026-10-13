import assert from 'node:assert/strict';
import { describe, it } from 'node:test';

import { STARTER_QUERY, loadState, previewQuery, renderState } from '../../src/cli/frontend.js';
import { fakeDocument } from './fake-document.mjs';

function response(status, body) {
  return {
    ok: status >= 200 && status < 300,
    status,
    json: async () => {
      if (body === undefined) throw new SyntaxError('no body');
      return body;
    },
  };
}

function render(state) {
  const doc = fakeDocument();
  const container = doc.createElement('section');
  const chosen = [];
  renderState(doc, container, state, (sql) => chosen.push(sql));
  return { container, chosen };
}

const STAT_COLUMNS = ['path', 'basename', 'dir', 'ext', 'size', 'mtime', 'ctime'];

const buttonNamed = (container, label) => container.find('button').find((b) => b.textContent === label);

describe('loadState', () => {
  it('reads only /frontend/state and never opens /events', async () => {
    const urls = [];
    const state = await loadState(async (url) => {
      urls.push(url);
      return response(200, { error: null, root: '/data', tables: [] });
    });
    assert.deepEqual(urls, ['/frontend/state']);
    assert.deepEqual(state, { status: 'ready', root: '/data', tables: [] });
  });

  it('reports a config failure with its diagnostic', async () => {
    const state = await loadState(async () => response(503, { error: 'failed to resolve missing.toml' }));
    assert.deepEqual(state, { status: 'unavailable', message: 'failed to resolve missing.toml' });
  });

  it('reports any other failure as unloaded', async () => {
    assert.deepEqual(await loadState(async () => response(500, { error: 'catalog read failed' })), {
      status: 'failed',
      message: 'catalog read failed',
    });
    assert.deepEqual(await loadState(async () => response(502)), { status: 'failed', message: 'HTTP 502' });
    assert.deepEqual(
      await loadState(async () => {
        throw new TypeError('Failed to fetch');
      }),
      { status: 'failed', message: 'Failed to fetch' },
    );
  });
});

describe('previewQuery', () => {
  it('quotes the table name as an identifier and bounds the preview', () => {
    assert.equal(previewQuery('posts'), 'SELECT * FROM "posts" LIMIT 100');
    assert.equal(previewQuery('order'), 'SELECT * FROM "order" LIMIT 100');
    assert.equal(previewQuery('two "words"'), 'SELECT * FROM "two ""words""" LIMIT 100');
  });
});

describe('configless start', () => {
  const state = { status: 'ready', root: '/home/me/notes', tables: [] };

  it('shows the query root and offers the editable path-table example', () => {
    const { container, chosen } = render(state);
    assert.match(container.textContent, /\/home\/me\/notes/);
    assert.match(container.textContent, /No declared tables/);
    const example = buttonNamed(container, STARTER_QUERY);
    assert.ok(example, 'path-table example button');
    example.click();
    assert.deepEqual(chosen, [STARTER_QUERY]);
  });

  it('documents the seven stat columns and the lazy content column', () => {
    const { container } = render(state);
    const codes = container.find('code').map((c) => c.textContent);
    for (const column of [...STAT_COLUMNS, 'content']) assert.ok(codes.includes(column), column);
    assert.match(container.textContent, /SELECT \*/);
    assert.match(container.textContent, /name it/i);
  });

  it('explains editing the path or glob in SQL', () => {
    const { container } = render(state);
    const codes = container.find('code').map((c) => c.textContent);
    assert.ok(codes.includes("'./**/*.md'"));
    assert.match(container.textContent, /no configuration/i);
  });

  it('explains refresh by rerunning', () => {
    assert.match(render(state).container.textContent, /Run again/);
  });
});

describe('declared tables', () => {
  const state = {
    status: 'ready',
    root: '/srv/blog',
    tables: [
      {
        name: 'posts',
        columns: [
          { name: 'title', type: 'TEXT' },
          { name: 'size', type: 'INTEGER' },
        ],
      },
      { name: 'two "words"', columns: [{ name: 'select', type: '' }] },
    ],
  };

  it('lists each table with its columns and types', () => {
    const { container } = render(state);
    const text = container.textContent;
    for (const fragment of ['posts', 'title', 'TEXT', 'size', 'INTEGER', 'two "words"', 'select']) {
      assert.ok(text.includes(fragment), fragment);
    }
  });

  it('populates a quoted bounded preview without running it', () => {
    const { container, chosen } = render(state);
    buttonNamed(container, 'two "words"').click();
    buttonNamed(container, 'posts').click();
    assert.deepEqual(chosen, ['SELECT * FROM "two ""words""" LIMIT 100', 'SELECT * FROM "posts" LIMIT 100']);
  });

  it('keeps the path-table example and explains the two anchors', () => {
    const { container, chosen } = render(state);
    buttonNamed(container, STARTER_QUERY).click();
    assert.deepEqual(chosen, [STARTER_QUERY]);
    assert.doesNotMatch(container.textContent, /No declared tables/);
    assert.match(container.textContent, /config file's directory/);
    assert.match(container.textContent, /\/srv\/blog/);
    assert.match(container.textContent, /reflect the server's index/);
  });
});

describe('config failure', () => {
  it('shows the diagnostic and restart guidance as an alert', () => {
    const { container } = render({ status: 'unavailable', message: 'failed to resolve missing.toml' });
    assert.match(container.textContent, /Configuration could not load/);
    assert.match(container.textContent, /failed to resolve missing\.toml/);
    assert.match(container.textContent, /restart/i);
    assert.match(container.textContent, /dirsql server --frontend -c/);
    assert.equal(container.find('h2')[0].attrs.role, 'alert');
    assert.equal(container.find('button').length, 0);
  });

  it('shows a failure to load the state', () => {
    const { container } = render({ status: 'failed', message: 'Failed to fetch' });
    assert.match(container.textContent, /Could not load the server state: Failed to fetch/);
    assert.equal(container.find('button').length, 0);
  });

  it('renders diagnostics, roots and names as text only', () => {
    const evil = '<img src=x onerror=alert(1)>';
    for (const state of [
      { status: 'unavailable', message: evil },
      { status: 'ready', root: evil, tables: [{ name: evil, columns: [{ name: evil, type: evil }] }] },
    ]) {
      const { container } = render(state);
      assert.equal(container.find('img').length, 0);
      assert.ok(container.textContent.includes(evil));
    }
  });
});
