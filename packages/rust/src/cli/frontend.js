export const STARTER_QUERY = '';
export const DISPLAY_ROW_LIMIT = 0;

export function formatCell() {
  return { text: '', isNull: false };
}

export function createQueryRunner() {
  return { run: async () => {}, state: () => ({ status: 'idle' }) };
}

export function renderResult() {}
