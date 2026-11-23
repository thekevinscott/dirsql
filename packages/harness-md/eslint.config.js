import tseslint from '@typescript-eslint/eslint-plugin';
import tsparser from '@typescript-eslint/parser';
import stylistic from '@stylistic/eslint-plugin';

export default [
  {
    ignores: ['dist/**', 'node_modules/**'],
  },
  {
    files: ['src/**/*.ts', 'test/**/*.ts'],
    languageOptions: {
      parser: tsparser,
    },
    plugins: {
      '@typescript-eslint': tseslint,
      '@stylistic': stylistic,
    },
    rules: {
      ...tseslint.configs.recommended.rules,
      '@typescript-eslint/no-unused-vars': ['error', { argsIgnorePattern: '^_' }],
      // Always require semicolons. `semi` is a deprecated core rule (removed in
      // ESLint v11); @stylistic is the maintained, TS-aware home for it.
      '@stylistic/semi': ['error', 'always'],
      // Always require braces around control-statement bodies.
      curly: ['error', 'all'],
    },
  },
];
