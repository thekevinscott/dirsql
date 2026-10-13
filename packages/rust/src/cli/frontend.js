export const STARTER_QUERY = "SELECT * FROM './' LIMIT 100";
export const DISPLAY_ROW_LIMIT = 1000;

export function formatCell(value) {
  if (value === null || value === undefined) return { text: 'NULL', isNull: true };
  if (typeof value === 'object') return { text: JSON.stringify(value), isNull: false };
  return { text: String(value), isNull: false };
}

async function failureOf(response) {
  let message = `HTTP ${response.status}`;
  try {
    const body = await response.json();
    if (body && typeof body.error === 'string') message = body.error;
  } catch {}
  return { kind: response.status === 400 ? 'query' : 'server', httpStatus: response.status, message };
}

export function createQueryRunner({ fetch, now, onChange }) {
  let current = { status: 'idle' };
  let seq = 0;
  let inflight = null;

  const set = (state) => {
    current = state;
    onChange(state);
  };

  async function execute(sql, id, controller) {
    const started = now();
    const elapsed = () => Math.round(now() - started);
    let outcome;
    try {
      const response = await fetch('/query', { method: 'POST', body: JSON.stringify({ sql }), signal: controller.signal });
      if (response.ok) {
        const rows = await response.json();
        if (!Array.isArray(rows)) throw new Error('unexpected response');
        const shown = rows.slice(0, DISPLAY_ROW_LIMIT);
        outcome = {
          status: 'success',
          sql,
          rows: shown,
          columns: shown.length ? Object.keys(shown[0]) : [],
          rowCount: rows.length,
          shownCount: shown.length,
          truncated: rows.length > shown.length,
        };
      } else {
        outcome = { status: 'error', sql, ...(await failureOf(response)) };
      }
    } catch (error) {
      outcome =
        error instanceof TypeError || error.name === 'AbortError'
          ? { status: 'error', sql, kind: 'network', message: error.message }
          : { status: 'error', sql, kind: 'server', message: error.message };
    }
    if (id !== seq) return;
    inflight = null;
    set({ ...outcome, elapsedMs: elapsed() });
  }

  return {
    state: () => current,
    run(sql) {
      if (inflight && inflight.sql === sql) return Promise.resolve();
      if (inflight) inflight.controller.abort();
      seq += 1;
      const controller = new AbortController();
      inflight = { sql, controller, done: null };
      set({ status: 'running', sql });
      inflight.done = execute(sql, seq, controller);
      return inflight.done;
    },
  };
}

const plural = (n, word) => `${n} ${word}${n === 1 ? '' : 's'}`;

function summaryOf(state) {
  const returned = `${plural(state.rowCount, 'row')} returned in ${state.elapsedMs} ms`;
  if (state.truncated) return `${returned}. Display limit: first ${state.shownCount} shown.`;
  if (state.rowCount === 0) return `${returned}. Column names are not available for an empty result.`;
  return `${returned}.`;
}

function tableOf(doc, state) {
  const table = doc.createElement('table');
  const head = doc.createElement('tr');
  for (const column of state.columns) {
    const th = doc.createElement('th');
    th.setAttribute('scope', 'col');
    th.textContent = column;
    head.append(th);
  }
  table.append(head);
  for (const row of state.rows) {
    const tr = doc.createElement('tr');
    for (const column of state.columns) {
      const { text, isNull } = formatCell(row[column]);
      const td = doc.createElement('td');
      td.textContent = text;
      if (isNull) td.className = 'null';
      tr.append(td);
    }
    table.append(tr);
  }
  return table;
}

const ERROR_LABEL = { query: 'Query error', server: 'Server error', network: 'Could not reach the server' };

function errorText(state) {
  if (state.kind === 'network') return `${ERROR_LABEL.network}: ${state.message}`;
  const status = state.httpStatus ? ` (HTTP ${state.httpStatus})` : '';
  return `${ERROR_LABEL[state.kind]}${status}: ${state.message}`;
}

export function renderResult(doc, container, state) {
  const note = doc.createElement('p');
  if (state.status === 'running') {
    note.textContent = 'Running…';
    container.replaceChildren(note);
    return;
  }
  if (state.status === 'error') {
    note.className = 'error';
    note.setAttribute('role', 'alert');
    note.textContent = errorText(state);
    container.replaceChildren(note);
    return;
  }
  if (state.status !== 'success') {
    container.replaceChildren();
    return;
  }
  note.textContent = summaryOf(state);
  container.replaceChildren(...(state.rowCount ? [note, tableOf(doc, state)] : [note]));
}

export async function loadState(fetch) {
  try {
    const response = await fetch('/frontend/state');
    if (!response.ok) {
      const { message } = await failureOf(response);
      return { status: response.status === 503 ? 'unavailable' : 'failed', message };
    }
    const { root, tables } = await response.json();
    return { status: 'ready', root, tables };
  } catch (error) {
    return { status: 'failed', message: error.message };
  }
}

export function previewQuery(table) {
  return `SELECT * FROM "${table.replaceAll('"', '""')}" LIMIT 100`;
}

const PATH_COLUMNS = [
  ['path', 'path relative to the query root'],
  ['basename', 'filename with extension'],
  ['dir', 'parent directory'],
  ['ext', 'extension without the dot'],
  ['size', 'size in bytes'],
  ['mtime', 'modification time, Unix seconds'],
  ['ctime', 'creation/change time, Unix seconds'],
];

function node(doc, tag, ...children) {
  const el = doc.createElement(tag);
  el.append(...children);
  return el;
}

const code = (doc, text) => node(doc, 'code', text);

function choice(doc, label, sql, onChoose) {
  const button = node(doc, 'button', label);
  button.setAttribute('type', 'button');
  button.className = 'choice';
  button.addEventListener('click', () => onChoose(sql));
  return button;
}

function pathTableGuide(doc, onChoose) {
  return [
    node(doc, 'h2', 'Path-tables'),
    node(
      doc,
      'p',
      'Query files with no configuration: write a quoted path or glob where a table name goes, such as ',
      code(doc, "'./'"),
      ' (files directly in the query root) or ',
      code(doc, "'./**/*.md'"),
      ' (Markdown files at any depth). Edit the path in the SQL to choose files.',
    ),
    choice(doc, STARTER_QUERY, STARTER_QUERY, onChoose),
    node(doc, 'ul', ...PATH_COLUMNS.map(([name, meaning]) => node(doc, 'li', code(doc, name), ` ${meaning}`))),
    node(
      doc,
      'p',
      code(doc, 'content'),
      " holds the file's text. It is left out of SELECT * and read only when you name it, as in ",
      code(doc, "SELECT path, content FROM './*.md' LIMIT 10"),
      '.',
    ),
  ];
}

function tableCatalog(doc, tables, onChoose) {
  if (!tables.length) return [node(doc, 'h2', 'Declared tables'), node(doc, 'p', 'No declared tables.')];
  return [
    node(doc, 'h2', 'Declared tables'),
    node(
      doc,
      'p',
      "Declared-table globs are anchored at their config file's directory; path-table queries use the query root. Choose a table to load a preview query, then run it.",
    ),
    ...tables.map((table) =>
      node(
        doc,
        'div',
        choice(doc, table.name, previewQuery(table.name), onChoose),
        node(
          doc,
          'ul',
          ...table.columns.map((column) => node(doc, 'li', code(doc, column.name), column.type ? ` ${column.type}` : '')),
        ),
      ),
    ),
  ];
}

export function renderState(doc, container, state, onChoose) {
  if (state.status === 'ready') {
    container.className = 'panel';
    container.replaceChildren(
      node(doc, 'h2', 'Query root'),
      node(doc, 'p', code(doc, state.root)),
      ...pathTableGuide(doc, onChoose),
      ...tableCatalog(doc, state.tables, onChoose),
      node(
        doc,
        'p',
        "Run again to refresh: path-tables read the filesystem on every query; declared tables reflect the server's index.",
      ),
    );
    return;
  }
  const title = node(doc, 'h2', state.status === 'unavailable' ? 'Configuration could not load' : 'Could not connect');
  title.setAttribute('role', 'alert');
  container.className = 'panel error';
  if (state.status === 'unavailable') {
    container.replaceChildren(
      title,
      node(doc, 'pre', state.message),
      node(
        doc,
        'p',
        'Fix the configuration, stop this server (Ctrl+C), and restart it with ',
        code(doc, 'dirsql server --frontend -c <config>'),
        '.',
      ),
    );
    return;
  }
  container.replaceChildren(title, node(doc, 'p', `Could not load the server state: ${state.message}`));
}

if (typeof document !== 'undefined') {
  const sql = document.getElementById('sql');
  const run = document.getElementById('run');
  const result = document.getElementById('result');
  const panel = document.getElementById('state');
  sql.value = STARTER_QUERY;
  loadState((url) => fetch(url)).then((state) =>
    renderState(document, panel, state, (query) => {
      sql.value = query;
      sql.focus();
    }),
  );
  const runner = createQueryRunner({
    fetch: (url, init) => fetch(url, init),
    now: () => performance.now(),
    onChange(state) {
      run.setAttribute('aria-busy', String(state.status === 'running'));
      renderResult(document, result, state);
    },
  });
  const submit = () => runner.run(sql.value);
  run.addEventListener('click', submit);
  sql.addEventListener('keydown', (event) => {
    if (event.key === 'Enter' && (event.ctrlKey || event.metaKey)) {
      event.preventDefault();
      submit();
    }
  });
}
