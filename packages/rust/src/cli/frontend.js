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
      const response = await fetch('/query', { method: 'POST', body: sql, signal: controller.signal });
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

if (typeof document !== 'undefined') {
  const sql = document.getElementById('sql');
  const run = document.getElementById('run');
  const result = document.getElementById('result');
  sql.value = STARTER_QUERY;
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
