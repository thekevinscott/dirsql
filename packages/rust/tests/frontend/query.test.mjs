import assert from 'node:assert/strict';
import { describe, it } from 'node:test';

import {
  DISPLAY_ROW_LIMIT,
  STARTER_QUERY,
  createQueryRunner,
  formatCell,
  renderResult,
} from '../../src/cli/frontend.js';

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

function clock(...ticks) {
  let i = 0;
  return () => ticks[Math.min(i++, ticks.length - 1)];
}

function deferred() {
  let resolve;
  const promise = new Promise((r) => {
    resolve = r;
  });
  return { promise, resolve };
}

function fakeDocument() {
  function element(tag) {
    const el = {
      tag,
      children: [],
      attrs: {},
      className: '',
      _text: '',
      get textContent() {
        return el._text + el.children.map((c) => c.textContent).join('');
      },
      set textContent(v) {
        el._text = String(v);
        el.children = [];
      },
      set innerHTML(_) {
        throw new Error('innerHTML must never be used');
      },
      setAttribute(k, v) {
        el.attrs[k] = String(v);
      },
      append(...nodes) {
        el.children.push(...nodes);
      },
      replaceChildren(...nodes) {
        el._text = '';
        el.children = nodes;
      },
      find(tagName) {
        const out = [];
        const walk = (n) => {
          if (n.tag === tagName) out.push(n);
          n.children.forEach(walk);
        };
        walk(el);
        return out;
      },
    };
    return el;
  }
  return { createElement: element };
}

function run(state) {
  const doc = fakeDocument();
  const container = doc.createElement('div');
  renderResult(doc, container, state);
  return container;
}

describe('starter query', () => {
  it('is an editable bounded path-table query with the limit visible', () => {
    assert.equal(STARTER_QUERY, "SELECT * FROM './' LIMIT 100");
  });
});

describe('formatCell', () => {
  it('renders NULL distinctly from the text "NULL"', () => {
    assert.deepEqual(formatCell(null), { text: 'NULL', isNull: true });
    assert.deepEqual(formatCell('NULL'), { text: 'NULL', isNull: false });
  });

  it('renders scalars and multiline text as text', () => {
    assert.deepEqual(formatCell(42), { text: '42', isNull: false });
    assert.deepEqual(formatCell(true), { text: 'true', isNull: false });
    assert.deepEqual(formatCell('a\nb'), { text: 'a\nb', isNull: false });
    assert.deepEqual(formatCell(''), { text: '', isNull: false });
  });

  it('renders structured values as JSON text', () => {
    assert.deepEqual(formatCell({ a: [1] }), { text: '{"a":[1]}', isNull: false });
  });
});

describe('query success', () => {
  it('posts the SQL as written to /query and reports rows and elapsed time', async () => {
    const calls = [];
    const fetch = async (url, init) => {
      calls.push([url, init.method, init.body]);
      return response(200, [{ path: 'a.txt', size: 3 }, { path: 'b.txt', size: 4 }]);
    };
    const changes = [];
    const runner = createQueryRunner({
      fetch,
      now: clock(1000, 1042),
      onChange: (s) => changes.push(s.status),
    });
    const sql = 'select path, size from "./"   limit 5';
    await runner.run(sql);
    assert.deepEqual(calls, [['/query', 'POST', sql]]);
    assert.deepEqual(changes, ['running', 'success']);
    const state = runner.state();
    assert.equal(state.status, 'success');
    assert.deepEqual(state.columns, ['path', 'size']);
    assert.equal(state.rowCount, 2);
    assert.equal(state.shownCount, 2);
    assert.equal(state.truncated, false);
    assert.equal(state.elapsedMs, 42);
  });

  it('separates display truncation from the returned row count', async () => {
    const rows = Array.from({ length: DISPLAY_ROW_LIMIT + 5 }, (_, i) => ({ n: i }));
    const runner = createQueryRunner({
      fetch: async () => response(200, rows),
      now: clock(0, 1),
      onChange() {},
    });
    await runner.run('SELECT 1');
    const state = runner.state();
    assert.equal(state.rowCount, DISPLAY_ROW_LIMIT + 5);
    assert.equal(state.shownCount, DISPLAY_ROW_LIMIT);
    assert.equal(state.truncated, true);
    const text = run(state).textContent;
    assert.match(text, new RegExp(`${DISPLAY_ROW_LIMIT + 5} rows returned`));
    assert.match(text, new RegExp(`first ${DISPLAY_ROW_LIMIT} shown`));
    assert.equal(run(state).find('tr').length, DISPLAY_ROW_LIMIT + 1);
  });

  it('renders a header, one row per result and NULL cells', async () => {
    const runner = createQueryRunner({
      fetch: async () => response(200, [{ a: 1, b: null }]),
      now: clock(0, 5),
      onChange() {},
    });
    await runner.run('SELECT 1');
    const view = run(runner.state());
    assert.deepEqual(
      view.find('th').map((t) => t.textContent),
      ['a', 'b'],
    );
    const cells = view.find('td');
    assert.deepEqual(
      cells.map((c) => c.textContent),
      ['1', 'NULL'],
    );
    assert.match(cells[1].className, /null/);
    assert.match(view.textContent, /1 row returned in 5 ms/);
  });
});

describe('zero rows', () => {
  it('says so explicitly and does not invent columns', async () => {
    const runner = createQueryRunner({
      fetch: async () => response(200, []),
      now: clock(0, 7),
      onChange() {},
    });
    await runner.run("SELECT * FROM './nothing'");
    const state = runner.state();
    assert.equal(state.status, 'success');
    assert.deepEqual(state.columns, []);
    assert.equal(state.rowCount, 0);
    const view = run(state);
    assert.match(view.textContent, /0 rows returned in 7 ms/);
    assert.match(view.textContent, /column names are not available/i);
    assert.equal(view.find('th').length, 0);
    assert.equal(view.find('table').length, 0);
  });
});

describe('read-only rejection', () => {
  it('shows the server message as a query error', async () => {
    const runner = createQueryRunner({
      fetch: async () => response(400, { error: 'attempt to write a readonly database' }),
      now: clock(0, 3),
      onChange() {},
    });
    await runner.run('DELETE FROM t');
    const state = runner.state();
    assert.equal(state.status, 'error');
    assert.equal(state.kind, 'query');
    assert.equal(state.message, 'attempt to write a readonly database');
    const view = run(state);
    assert.match(view.textContent, /Query error/);
    assert.match(view.textContent, /attempt to write a readonly database/);
    assert.equal(view.find('table').length, 0);
    assert.equal(view.find('p')[0].attrs.role, 'alert');
  });
});

describe('server failure', () => {
  it('reports a server error status with its message', async () => {
    const runner = createQueryRunner({
      fetch: async () => response(503, { error: 'config failed to load' }),
      now: clock(0, 3),
      onChange() {},
    });
    await runner.run('SELECT 1');
    const state = runner.state();
    assert.equal(state.kind, 'server');
    assert.equal(state.message, 'config failed to load');
    assert.match(run(state).textContent, /Server error \(HTTP 503\)/);
  });

  it('reports a non-JSON failure response by status', async () => {
    const runner = createQueryRunner({
      fetch: async () => response(502),
      now: clock(0, 3),
      onChange() {},
    });
    await runner.run('SELECT 1');
    const state = runner.state();
    assert.equal(state.kind, 'server');
    assert.match(state.message, /502/);
  });

  it('reports an unreachable server', async () => {
    const runner = createQueryRunner({
      fetch: async () => {
        throw new TypeError('Failed to fetch');
      },
      now: clock(0, 3),
      onChange() {},
    });
    await runner.run('SELECT 1');
    const state = runner.state();
    assert.equal(state.kind, 'network');
    assert.match(run(state).textContent, /Could not reach the server: Failed to fetch/);
  });

  it('reports a malformed success body as a server error', async () => {
    const runner = createQueryRunner({
      fetch: async () => response(200),
      now: clock(0, 3),
      onChange() {},
    });
    await runner.run('SELECT 1');
    assert.equal(runner.state().status, 'error');
    assert.equal(runner.state().kind, 'server');
  });
});

describe('safe rendering', () => {
  it('shows HTML-like cells, headers and errors as text only', async () => {
    const evil = '<img src=x onerror=alert(1)>';
    const runner = createQueryRunner({
      fetch: async () => response(200, [{ [evil]: `${evil}\nline two` }]),
      now: clock(0, 1),
      onChange() {},
    });
    await runner.run('SELECT 1');
    const view = run(runner.state());
    assert.equal(view.find('img').length, 0);
    assert.deepEqual(
      view.find('th').map((t) => t.textContent),
      [evil],
    );
    assert.deepEqual(
      view.find('td').map((t) => t.textContent),
      [`${evil}\nline two`],
    );

    const failing = createQueryRunner({
      fetch: async () => response(400, { error: evil }),
      now: clock(0, 1),
      onChange() {},
    });
    await failing.run('SELECT 1');
    const errView = run(failing.state());
    assert.equal(errView.find('img').length, 0);
    assert.match(errView.textContent, /<img src=x onerror=alert\(1\)>/);
  });
});

describe('request ordering and duplicates', () => {
  it('a delayed earlier response cannot overwrite a later query', async () => {
    const first = deferred();
    const second = deferred();
    const pending = [first, second];
    const fetch = () => pending.shift().promise;
    const runner = createQueryRunner({ fetch, now: clock(0, 1, 2, 3), onChange() {} });
    const a = runner.run('SELECT 1');
    const b = runner.run('SELECT 2');
    second.resolve(response(200, [{ v: 'second' }]));
    await b;
    assert.deepEqual(runner.state().rows, [{ v: 'second' }]);
    first.resolve(response(200, [{ v: 'first' }]));
    await a;
    assert.equal(runner.state().sql, 'SELECT 2');
    assert.deepEqual(runner.state().rows, [{ v: 'second' }]);
  });

  it('a delayed earlier failure cannot overwrite a later success', async () => {
    const first = deferred();
    const second = deferred();
    const pending = [first, second];
    const runner = createQueryRunner({
      fetch: () => pending.shift().promise,
      now: clock(0, 1, 2, 3),
      onChange() {},
    });
    const a = runner.run('SELECT 1');
    const b = runner.run('SELECT 2');
    second.resolve(response(200, [{ v: 1 }]));
    await b;
    first.resolve(response(500, { error: 'late' }));
    await a;
    assert.equal(runner.state().status, 'success');
  });

  it('aborts the superseded request', async () => {
    const signals = [];
    const runner = createQueryRunner({
      fetch: (_url, init) => {
        signals.push(init.signal);
        return new Promise(() => {});
      },
      now: clock(0),
      onChange() {},
    });
    runner.run('SELECT 1');
    runner.run('SELECT 2');
    assert.equal(signals[0].aborted, true);
    assert.equal(signals[1].aborted, false);
  });

  it('ignores an identical submission while one is in flight', async () => {
    const gate = deferred();
    let calls = 0;
    const runner = createQueryRunner({
      fetch: () => {
        calls += 1;
        return gate.promise;
      },
      now: clock(0, 1),
      onChange() {},
    });
    const a = runner.run('SELECT 1');
    await runner.run('SELECT 1');
    assert.equal(calls, 1);
    gate.resolve(response(200, []));
    await a;
    await runner.run('SELECT 1');
    assert.equal(calls, 2);
  });

  it('shows a running state with the SQL in flight', () => {
    const runner = createQueryRunner({
      fetch: () => new Promise(() => {}),
      now: clock(0),
      onChange() {},
    });
    runner.run('SELECT 1');
    assert.deepEqual(
      { status: runner.state().status, sql: runner.state().sql },
      { status: 'running', sql: 'SELECT 1' },
    );
    assert.match(run(runner.state()).textContent, /Running/);
  });
});
