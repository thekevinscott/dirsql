import type { Language } from './write.js';

const pythonLiteral = (value: unknown, indent = ''): string => {
  const inner = `${indent}  `;
  if (value === null || value === undefined) {
    return 'None';
  }
  if (typeof value === 'boolean') {
    return value ? 'True' : 'False';
  }
  if (Array.isArray(value)) {
    return value.length === 0
      ? '[]'
      : `[\n${value.map(item => `${inner}${pythonLiteral(item, inner)},\n`).join('')}${indent}]`;
  }
  if (typeof value === 'object') {
    const entries = Object.entries(value);
    return entries.length === 0
      ? '{}'
      : `{\n${entries.map(([key, item]) => `${inner}${JSON.stringify(key)}: ${pythonLiteral(item, inner)},\n`).join('')}${indent}}`;
  }
  return JSON.stringify(value);
};

const shellQuote = (text: string) => `'${text.replaceAll("'", "'\\''")}'`;

export const assign: Record<Language, (name: string, value: unknown) => string> = {
  python: (name, value) => `${name} = ${pythonLiteral(value)}`,
  typescript: (name, value) => `const ${name} = ${JSON.stringify(value, null, 2)};`,
  // Rust has no file-level runtime values: text is a const, anything else is parsed on first use.
  rust: (name, value) => typeof value === 'string'
    ? `#[allow(non_upper_case_globals)]\nconst ${name}: &str = ${JSON.stringify(value)};`
    : [
      '#[allow(non_upper_case_globals)]',
      `static ${name}: std::sync::LazyLock<serde_json::Value> = std::sync::LazyLock::new(|| {`,
      `  serde_json::from_str(r#"${JSON.stringify(value, null, 2).replaceAll('\n', '\n  ')}"#).unwrap()`,
      '});',
    ].join('\n'),
  bash: (name, value) => `${name}=${shellQuote(typeof value === 'string' ? value : JSON.stringify(value))}`,
};
