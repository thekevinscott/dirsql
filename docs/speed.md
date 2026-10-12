# dirsql performance suite

- Commit: 0cd2b68e34d406870d3639d13c4efa185189b38c
- Date: 2026-10-12T01:12:57Z
- Machine: platform: Linux-7.0.0-28-generic-x86_64-with-glibc2.39, cpu: AMD Ryzen 9 9950X 16-Core Processor, cpus: 32
- Versions: bash: GNU bash, version 5.2.21(1)-release (x86_64-pc-linux-gnu), find: find (GNU findutils) 4.9.0, rg: ripgrep 14.1.0, xargs: xargs (GNU findutils) 4.9.0, inotifywait: inotifywait 3.22.6.0
- Method: 10 paired runs per case, interleaved; ratios are dirsql over the native after each side's startup, median across pairs
- Cadence: rerun with `just bench-suite` after a change to how dirsql walks, matches, queries or serializes, and when this report is more than 30 days old

| Case | Fastest native | dirsql / fastest | Spread | vs 1.0x |
|---|---|---|---|---|
| `./*` | rg | 1.45x | 100% | over |
| `./**` | fd | 1.53x | 29% | over |
| `./**/*.js` | fd | 2.27x | 43% | over |
| `./**/*.md` | fd | 2.42x | 29% | over |
| `./**/*.md` | fd | 1.89x | 119% | over |
| `./**/*.md` | bash-globstar | 0.08x | 112% | ok |
| `./**/.*` | fd | 1.27x | 13% | over |
| `./**/[[:digit:]]*.md` | fd | 2.80x | 12% | over |
| `./**/[a-c]*.md` | fd | 3.20x | 10% | over |
| `./**/file?.md` | fd | 6.84x | 24% | over |
| `./**/node_modules/pkg/*.js` | fd | 2.30x | 74% | over |
| `./**/{a,b}*.md` | fd | 3.41x | 73% | over |
| `./*/*.md` | fd | 2.27x | 24% | over |
| `./.cache/**/*.md` | fd | 2.88x | 19% | over |
| `./a/b/**/*.md` | fd | 3.26x | 148% | over |
| `./docs/` | rg | 1.37x | 35% | over |
| `SELECT path, size, mtime FROM './**/*'` | fd+xargs-stat | 0.72x | 76% | ok |
| `dossier` | find+sort | 0.59x | 87% | ok |
| `frontmatter` | find+xargs-P | 2.58x | 46% | over |
| `join` | find+join | 0.37x | 18% | ok |
| `jsonl` | jq+xargs-P | 1.43x | 28% | over |
| `notebook` | python | 0.94x | 24% | ok |
| `transcripts` | find+xargs-P | 0.40x | 38% | ok |

## `./*`

dirsql median 0.724s

| Native | Median s | dirsql / native | Min | Max | Spread |
|---|---|---|---|---|---|
| bash-glob | 43.159 | 0.02x | 0.01x | 0.03x | 137% |
| fd | 0.551 | 1.21x | 0.37x | 2.24x | 154% |
| find | 0.696 | 0.96x | 0.76x | 1.55x | 83% |
| rg | 0.549 | 1.45x | 0.45x | 1.90x | 100% |

## `./**`

dirsql median 0.488s

| Native | Median s | dirsql / native | Min | Max | Spread |
|---|---|---|---|---|---|
| fd | 0.296 | 1.53x | 1.27x | 1.70x | 29% |
| git-ls-files | 1.092 | 0.40x | 0.36x | 0.42x | 14% |
| rg | 0.370 | 1.23x | 1.10x | 1.37x | 22% |

## `./**/*.js`

dirsql median 0.823s

| Native | Median s | dirsql / native | Min | Max | Spread |
|---|---|---|---|---|---|
| fd | 0.336 | 2.27x | 1.92x | 2.89x | 43% |
| find | 1.210 | 0.67x | 0.60x | 0.71x | 16% |
| rg | 0.595 | 1.36x | 1.18x | 1.45x | 20% |

## `./**/*.md`

dirsql median 0.164s

| Native | Median s | dirsql / native | Min | Max | Spread |
|---|---|---|---|---|---|
| fd | 0.051 | 2.42x | 2.14x | 2.83x | 29% |
| find | 0.367 | 0.34x | 0.32x | 0.35x | 11% |
| rg | 0.082 | 1.55x | 1.40x | 1.61x | 13% |

## `./**/*.md`

dirsql median 0.351s

| Native | Median s | dirsql / native | Min | Max | Spread |
|---|---|---|---|---|---|
| bash-globstar | 20.708 | 0.01x | 0.01x | 0.02x | 38% |
| fd | 0.162 | 1.89x | 0.50x | 2.74x | 119% |
| find | 0.700 | 0.45x | 0.11x | 0.64x | 115% |
| rg | 0.269 | 1.15x | 0.93x | 1.49x | 49% |

## `./**/*.md`

dirsql median 0.161s

| Native | Median s | dirsql / native | Min | Max | Spread |
|---|---|---|---|---|---|
| bash-globstar | 1.482 | 0.08x | 0.05x | 0.14x | 112% |

## `./**/.*`

dirsql median 0.102s

| Native | Median s | dirsql / native | Min | Max | Spread |
|---|---|---|---|---|---|
| fd | 0.048 | 1.27x | 1.20x | 1.36x | 13% |
| find | 0.335 | 0.18x | 0.18x | 0.19x | 9% |
| rg | 0.054 | 1.14x | 1.05x | 1.32x | 24% |

## `./**/[[:digit:]]*.md`

dirsql median 0.211s

| Native | Median s | dirsql / native | Min | Max | Spread |
|---|---|---|---|---|---|
| fd | 0.062 | 2.80x | 2.64x | 2.97x | 12% |
| find | 0.554 | 0.31x | 0.29x | 0.33x | 10% |
| rg | 0.138 | 1.25x | 1.14x | 1.36x | 17% |

## `./**/[a-c]*.md`

dirsql median 0.295s

| Native | Median s | dirsql / native | Min | Max | Spread |
|---|---|---|---|---|---|
| fd | 0.079 | 3.20x | 3.15x | 3.46x | 10% |
| find | 0.558 | 0.45x | 0.39x | 0.47x | 18% |
| rg | 0.143 | 1.85x | 1.64x | 1.96x | 17% |

## `./**/file?.md`

dirsql median 0.384s

| Native | Median s | dirsql / native | Min | Max | Spread |
|---|---|---|---|---|---|
| fd | 0.050 | 6.84x | 5.42x | 7.07x | 24% |
| find | 0.546 | 0.65x | 0.59x | 0.67x | 14% |
| rg | 0.138 | 2.55x | 2.19x | 2.74x | 22% |

## `./**/node_modules/pkg/*.js`

dirsql median 0.916s

| Native | Median s | dirsql / native | Min | Max | Spread |
|---|---|---|---|---|---|
| fd | 0.347 | 2.30x | 1.56x | 3.27x | 74% |
| find | 2.146 | 0.52x | 0.06x | 0.89x | 160% |
| rg | 0.430 | 1.91x | 0.74x | 2.64x | 99% |

## `./**/{a,b}*.md`

dirsql median 0.265s

| Native | Median s | dirsql / native | Min | Max | Spread |
|---|---|---|---|---|---|
| fd | 0.065 | 3.41x | 2.25x | 4.74x | 73% |
| find | 0.580 | 0.39x | 0.35x | 0.51x | 42% |
| rg | 0.134 | 1.72x | 1.46x | 2.20x | 43% |

## `./*/*.md`

dirsql median 0.739s

| Native | Median s | dirsql / native | Min | Max | Spread |
|---|---|---|---|---|---|
| fd | 0.298 | 2.27x | 1.93x | 2.48x | 24% |
| find | 1.600 | 0.43x | 0.37x | 0.46x | 20% |
| rg | 0.502 | 1.36x | 1.17x | 1.53x | 27% |

## `./.cache/**/*.md`

dirsql median 0.115s

| Native | Median s | dirsql / native | Min | Max | Spread |
|---|---|---|---|---|---|
| fd | 0.026 | 2.88x | 2.63x | 3.18x | 19% |
| find | 0.185 | 0.40x | 0.38x | 0.42x | 10% |
| rg | 0.048 | 1.54x | 1.40x | 1.68x | 18% |

## `./a/b/**/*.md`

dirsql median 0.145s

| Native | Median s | dirsql / native | Min | Max | Spread |
|---|---|---|---|---|---|
| bash-globstar | 3.195 | 0.04x | 0.02x | 0.08x | 148% |
| fd | 0.031 | 3.26x | 0.59x | 5.43x | 148% |
| find | 0.133 | 0.73x | 0.34x | 1.23x | 121% |
| rg | 0.048 | 2.17x | 1.03x | 2.44x | 65% |

## `./docs/`

dirsql median 0.090s

| Native | Median s | dirsql / native | Min | Max | Spread |
|---|---|---|---|---|---|
| bash-glob | 0.318 | 0.14x | 0.11x | 0.18x | 47% |
| fd | 0.036 | 1.31x | 1.21x | 1.61x | 31% |
| find | 0.047 | 0.99x | 0.84x | 1.20x | 36% |
| rg | 0.034 | 1.37x | 1.18x | 1.65x | 35% |

## `SELECT path, size, mtime FROM './**/*'`

dirsql median 0.285s

| Native | Median s | dirsql / native | Min | Max | Spread |
|---|---|---|---|---|---|
| fd+xargs-stat | 0.329 | 0.72x | 0.32x | 0.87x | 76% |
| find | 0.732 | 0.30x | 0.15x | 0.81x | 219% |

## `dossier`

dirsql median 0.977s

| Native | Median s | dirsql / native | Min | Max | Spread |
|---|---|---|---|---|---|
| find+sort | 1.470 | 0.59x | 0.32x | 0.83x | 87% |

## `frontmatter`

dirsql median 0.600s

| Native | Median s | dirsql / native | Min | Max | Spread |
|---|---|---|---|---|---|
| fd+xargs-P | 0.217 | 2.53x | 1.98x | 2.76x | 31% |
| find+awk | 1.962 | 0.27x | 0.20x | 0.30x | 38% |
| find+xargs-P | 0.209 | 2.58x | 1.69x | 2.88x | 46% |

## `join`

dirsql median 0.446s

| Native | Median s | dirsql / native | Min | Max | Spread |
|---|---|---|---|---|---|
| find+join | 1.080 | 0.37x | 0.34x | 0.40x | 18% |

## `jsonl`

dirsql median 0.711s

| Native | Median s | dirsql / native | Min | Max | Spread |
|---|---|---|---|---|---|
| jq | 1.759 | 0.38x | 0.36x | 0.42x | 16% |
| jq+xargs-P | 0.459 | 1.43x | 1.21x | 1.62x | 28% |

## `notebook`

dirsql median 1.360s

| Native | Median s | dirsql / native | Min | Max | Spread |
|---|---|---|---|---|---|
| python | 1.374 | 0.94x | 0.82x | 1.05x | 24% |

## `transcripts`

dirsql median 0.718s

| Native | Median s | dirsql / native | Min | Max | Spread |
|---|---|---|---|---|---|
| fd+xargs-P | 1.716 | 0.38x | 0.35x | 0.47x | 33% |
| find+python | 2.146 | 0.31x | 0.29x | 0.37x | 27% |
| find+xargs-P | 1.671 | 0.40x | 0.33x | 0.48x | 38% |
