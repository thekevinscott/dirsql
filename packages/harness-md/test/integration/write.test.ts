import { mkdtemp, readFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { expect, test } from 'vitest';
import { write } from 'harness-md';

const fixture = (name: string) =>
  fileURLToPath(new URL(`./__fixtures__/${name}`, import.meta.url));

const outDir = () => mkdtemp(path.join(tmpdir(), 'harness-md-test-'));

test('writes the python blocks of a document to a test file', async () => {
  const dir = await outDir();
  const written = await write(fixture('two-languages.md'), { language: 'python', outDir: dir });

  expect(written).toBe(path.join(dir, 'two_languages_test.py'));
  expect(await readFile(written, 'utf8')).toBe([
    'import pytest',
    '',
    'def test_it_adds():',
    '  assert 1 + 1 == 2',
    '',
  ].join('\n'));
});

test('writes the typescript blocks of a document to a test file', async () => {
  const dir = await outDir();
  const written = await write(fixture('two-languages.md'), { language: 'typescript', outDir: dir });

  expect(written).toBe(path.join(dir, 'two-languages.test.ts'));
  expect(await readFile(written, 'utf8')).toBe([
    "import { test, expect } from 'vitest';",
    '',
    "test('It adds', () => {",
    '  expect(1 + 1).toBe(2);',
    '});',
    '',
  ].join('\n'));
});

test('writes to tests/build under the working directory when no directory is given', async () => {
  const cwd = process.cwd();
  const dir = await outDir();
  process.chdir(dir);
  try {
    const written = await write(fixture('two-languages.md'), { language: 'python' });

    expect(written).toBe(path.join(process.cwd(), 'tests', 'build', 'two_languages_test.py'));
    expect(await readFile(written, 'utf8')).toContain('def test_it_adds():');
  } finally {
    process.chdir(cwd);
  }
});

test('writes data in scope as python variables, in document order', async () => {
  const written = await write(fixture('data.md'), { language: 'python', outDir: await outDir() });

  expect(await readFile(written, 'utf8')).toBe(`fixture = "tests/fixtures/simple"

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

test('writes data in scope as typescript constants, in document order', async () => {
  const written = await write(fixture('data.md'), { language: 'typescript', outDir: await outDir() });

  expect(await readFile(written, 'utf8')).toBe(`const fixture = "tests/fixtures/simple";

import { test, expect } from 'vitest';

const ms = {
  "max": 500,
  "ok": true
};

const cases = [
  [
    "SELECT 1",
    [
      {
        "n": 1
      }
    ]
  ]
];

test.for(cases)('It runs a query', ([query, expected]) => {
  expect(run(fixture, query)).toEqual(expected);
});
`);
});

test('leaves out data under a heading that holds no block in the language', async () => {
  const written = await write(fixture('data.md'), { language: 'python', outDir: await outDir() });

  expect(await readFile(written, 'utf8')).not.toContain('baseline');
});

test('writes a rust file: text as a const, other data parsed on first use', async () => {
  const written = await write(fixture('data.md'), { language: 'rust', outDir: await outDir() });

  expect(path.basename(written)).toBe('data.rs');
  expect(await readFile(written, 'utf8')).toBe(`#[allow(non_upper_case_globals)]
const fixture: &str = "tests/fixtures/simple";

use thing::run;

#[allow(non_upper_case_globals)]
static ms: std::sync::LazyLock<serde_json::Value> = std::sync::LazyLock::new(|| {
  serde_json::from_str(r#"{
    "max": 500,
    "ok": true
  }"#).unwrap()
});

#[allow(non_upper_case_globals)]
static cases: std::sync::LazyLock<serde_json::Value> = std::sync::LazyLock::new(|| {
  serde_json::from_str(r#"[
    [
      "SELECT 1",
      [
        {
          "n": 1
        }
      ]
    ]
  ]"#).unwrap()
});

#[test]
fn it_runs_a_query() {
  assert!(cases.is_array());
}
`);
});

test('writes a bash file: data as quoted shell variables, a bash data block as its text', async () => {
  const written = await write(fixture('data.md'), { language: 'bash', outDir: await outDir() });

  expect(path.basename(written)).toBe('data.sh');
  expect(await readFile(written, 'utf8')).toBe(`fixture='tests/fixtures/simple'

ms='{"max":500,"ok":true}'

cases='[["SELECT 1",[{"n":1}]]]'

baseline='find . -type f'

dirsql query
`);
});
