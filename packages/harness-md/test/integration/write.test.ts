import { cp, mkdtemp, readFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { expect, test } from 'vitest';
import { write } from 'harness-md';

const fixtures = fileURLToPath(new URL('./__fixtures__', import.meta.url));

const project = async () => {
  const root = await mkdtemp(path.join(tmpdir(), 'harness-md-test-'));
  await cp(path.join(fixtures, 'harnessmd.toml'), path.join(root, 'harnessmd.toml'));
  return root;
};

test('writes one file per test, surface and language, at the paths harnessmd.toml gives', async () => {
  const root = await project();
  const written = await write(path.join(fixtures, 'data.md'), { root });

  expect(written).toEqual([
    path.join(root, 'build/python/sdk/it_runs_a_query_test.py'),
    path.join(root, 'build/typescript/sdk/it_runs_a_query.test.ts'),
    path.join(root, 'build/rust/sdk/it_runs_a_query.rs'),
  ]);
});

test('writes data in scope as python variables, in document order', async () => {
  const root = await project();
  await write(path.join(fixtures, 'data.md'), { root });

  expect(await readFile(path.join(root, 'build/python/sdk/it_runs_a_query_test.py'), 'utf8')).toBe(`fixture = "tests/fixtures/simple"

import pytest

ms = {
  "max": 500,
  "ok": True,
}

cases = [
  [
    "SELECT 1",
    [
      {
        "n": 1,
      },
    ],
  ],
]

@pytest.mark.parametrize(("query", "expected"), cases)
def test_it_runs_a_query(query, expected):
  assert run(fixture, query) == expected
`);
});

test('leaves out data under a sibling heading', async () => {
  const root = await project();
  await write(path.join(fixtures, 'data.md'), { root });

  expect(await readFile(path.join(root, 'build/typescript/sdk/it_runs_a_query.test.ts'), 'utf8')).not.toContain('baseline');
});
