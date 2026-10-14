# dirsql performance suite

- Commit: 462b3c6afccefeb91cf93153141a82a330c318e3
- Date: 2026-10-14T01:49:05Z
- Machine: platform: Linux-7.0.0-28-generic-x86_64-with-glibc2.39, cpu: AMD Ryzen 9 9950X 16-Core Processor, cpus: 32
- Versions: bash: GNU bash, version 5.2.21(1)-release (x86_64-pc-linux-gnu), find: find (GNU findutils) 4.9.0, rg: ripgrep 14.1.0, xargs: xargs (GNU findutils) 4.9.0, inotifywait: inotifywait 3.22.6.0
- Method: 100 paired runs per case, interleaved; ratios are dirsql over the native after each side's startup, median across pairs
- Cadence: rerun with `just bench-suite` after a change to how dirsql walks, matches, queries or serializes, and when this report is more than 30 days old

| Case | Fastest native | dirsql / fastest | Spread | vs 1.0x |
|---|---|---|---|---|
| `./*` | rg | 1.43x | 83% | over |
| `./**` | fd | 1.39x | 28% | over |
| `./**/*.js` | fd | 2.16x | 60% | over |
| `./**/*.md` | fd | 1.61x | 119% | over |
| `./**/*.md (dotfiles)` | fd | 1.81x | 135% | over |
| `./**/*.md (symlinks)` | bash-globstar | 0.12x | 95% | ok |
| `./**/.*` | fd | 0.92x | 106% | ok |
| `./**/[[:digit:]]*.md` | fd | 1.45x | 96% | over |
| `./**/[a-c]*.md` | fd | 1.94x | 361% | over |
| `./**/file?.md` | fd | 1.84x | 185% | over |
| `./**/node_modules/pkg/*.js` | fd | 1.30x | 38% | over |
| `./**/{a,b}*.md` | fd | 2.06x | 32% | over |
| `./*/*.md` | fd | 2.10x | 22% | over |
| `./.cache/**/*.md` | fd | 1.88x | 58% | over |
| `./a/b/**/*.md` | fd | 2.64x | 144% | over |
| `./docs/` | rg | 1.55x | 169% | over |
| `SELECT path, size, mtime FROM './**/*'` | fd+xargs-stat | 0.32x | 108% | ok |
| `dossier` | find+sort | 0.72x | 82% | ok |
| `frontmatter` | fd+xargs-P | 2.41x | 36% | over |
| `join` | find+join | 0.42x | 55% | ok |
| `jsonl` | jq+xargs-P | 1.73x | 61% | over |
| `notebook` | python | 0.87x | 52% | ok |
| `transcripts` | find+python | 0.53x | 39% | ok |
| `watch` | inotifywait | 1.15x | 80% | over |

## `./*`

dirsql median 0.665s

| Native | Median s | dirsql / native | Min | Max | Spread |
|---|---|---|---|---|---|
| bash-glob | 14.463 | 0.04x | 0.04x | 0.04x | 20% |
| fd | 0.502 | 1.23x | 0.49x | 1.54x | 85% |
| find | 0.605 | 1.01x | 0.74x | 1.06x | 32% |
| rg | 0.427 | 1.43x | 0.42x | 1.59x | 83% |

## `./**`

dirsql median 0.267s

| Native | Median s | dirsql / native | Min | Max | Spread |
|---|---|---|---|---|---|
| fd | 0.152 | 1.39x | 1.24x | 1.62x | 28% |
| git-ls-files | 0.559 | 0.38x | 0.30x | 0.46x | 44% |
| rg | 0.196 | 1.09x | 0.95x | 1.53x | 53% |

## `./**/*.js`

dirsql median 0.692s

| Native | Median s | dirsql / native | Min | Max | Spread |
|---|---|---|---|---|---|
| fd | 0.303 | 2.16x | 1.43x | 2.74x | 60% |
| find | 1.181 | 0.55x | 0.34x | 0.73x | 71% |
| rg | 0.567 | 1.14x | 0.94x | 1.44x | 44% |

## `./**/*.md`

dirsql median 0.300s

| Native | Median s | dirsql / native | Min | Max | Spread |
|---|---|---|---|---|---|
| bash-globstar | 24.290 | 0.01x | 0.00x | 0.06x | 510% |
| fd | 0.166 | 1.61x | 0.42x | 2.33x | 119% |
| find | 0.685 | 0.48x | 0.12x | 0.74x | 128% |
| rg | 0.268 | 0.99x | 0.68x | 1.65x | 98% |

## `./**/*.md (dotfiles)`

dirsql median 0.092s

| Native | Median s | dirsql / native | Min | Max | Spread |
|---|---|---|---|---|---|
| fd | 0.028 | 1.81x | 0.37x | 2.83x | 135% |
| find | 0.212 | 0.25x | 0.08x | 0.32x | 99% |
| rg | 0.042 | 1.23x | 1.02x | 1.88x | 70% |

## `./**/*.md (symlinks)`

dirsql median 0.194s

| Native | Median s | dirsql / native | Min | Max | Spread |
|---|---|---|---|---|---|
| bash-globstar | 1.266 | 0.12x | 0.08x | 0.19x | 95% |

## `./**/.*`

dirsql median 0.067s

| Native | Median s | dirsql / native | Min | Max | Spread |
|---|---|---|---|---|---|
| fd | 0.028 | 0.92x | 0.57x | 1.54x | 106% |
| find | 0.170 | 0.15x | 0.07x | 0.24x | 120% |
| rg | 0.029 | 0.88x | 0.66x | 1.63x | 110% |

## `./**/[[:digit:]]*.md`

dirsql median 0.128s

| Native | Median s | dirsql / native | Min | Max | Spread |
|---|---|---|---|---|---|
| fd | 0.059 | 1.45x | 0.34x | 1.73x | 96% |
| find | 0.538 | 0.16x | 0.08x | 0.19x | 67% |
| rg | 0.129 | 0.67x | 0.58x | 0.92x | 52% |

## `./**/[a-c]*.md`

dirsql median 0.191s

| Native | Median s | dirsql / native | Min | Max | Spread |
|---|---|---|---|---|---|
| fd | 0.076 | 1.94x | 1.24x | 8.24x | 361% |
| find | 0.536 | 0.27x | 0.23x | 0.92x | 254% |
| rg | 0.137 | 1.08x | 0.75x | 4.50x | 345% |

## `./**/file?.md`

dirsql median 0.130s

| Native | Median s | dirsql / native | Min | Max | Spread |
|---|---|---|---|---|---|
| fd | 0.047 | 1.84x | 1.47x | 4.88x | 185% |
| find | 0.514 | 0.17x | 0.14x | 0.45x | 187% |
| rg | 0.128 | 0.68x | 0.53x | 1.75x | 180% |

## `./**/node_modules/pkg/*.js`

dirsql median 0.507s

| Native | Median s | dirsql / native | Min | Max | Spread |
|---|---|---|---|---|---|
| fd | 0.355 | 1.30x | 1.02x | 1.51x | 38% |
| find | 1.321 | 0.35x | 0.30x | 0.42x | 33% |
| rg | 0.380 | 1.22x | 1.06x | 1.42x | 30% |

## `./**/{a,b}*.md`

dirsql median 0.175s

| Native | Median s | dirsql / native | Min | Max | Spread |
|---|---|---|---|---|---|
| fd | 0.064 | 2.06x | 1.81x | 2.48x | 32% |
| find | 0.567 | 0.23x | 0.21x | 0.27x | 23% |
| rg | 0.134 | 0.99x | 0.88x | 1.20x | 32% |

## `./*/*.md`

dirsql median 0.617s

| Native | Median s | dirsql / native | Min | Max | Spread |
|---|---|---|---|---|---|
| fd | 0.272 | 2.10x | 1.89x | 2.34x | 22% |
| find | 1.286 | 0.44x | 0.38x | 0.47x | 20% |
| rg | 0.477 | 1.20x | 1.13x | 1.33x | 16% |

## `./.cache/**/*.md`

dirsql median 0.069s

| Native | Median s | dirsql / native | Min | Max | Spread |
|---|---|---|---|---|---|
| fd | 0.015 | 1.88x | 1.47x | 2.56x | 58% |
| find | 0.095 | 0.29x | 0.24x | 0.37x | 45% |
| rg | 0.026 | 1.05x | 0.86x | 1.60x | 71% |

## `./a/b/**/*.md`

dirsql median 0.149s

| Native | Median s | dirsql / native | Min | Max | Spread |
|---|---|---|---|---|---|
| bash-globstar | 1.693 | 0.06x | 0.03x | 0.14x | 171% |
| fd | 0.040 | 2.64x | 1.73x | 5.54x | 144% |
| find | 0.161 | 0.66x | 0.27x | 1.25x | 148% |
| rg | 0.048 | 2.23x | 1.15x | 3.78x | 118% |

## `./docs/`

dirsql median 0.099s

| Native | Median s | dirsql / native | Min | Max | Spread |
|---|---|---|---|---|---|
| bash-glob | 0.361 | 0.13x | 0.08x | 0.27x | 141% |
| fd | 0.030 | 1.53x | 0.83x | 3.42x | 170% |
| find | 0.050 | 0.89x | 0.45x | 1.82x | 154% |
| rg | 0.029 | 1.55x | 0.79x | 3.41x | 169% |

## `SELECT path, size, mtime FROM './**/*'`

dirsql median 0.602s

| Native | Median s | dirsql / native | Min | Max | Spread |
|---|---|---|---|---|---|
| fd+xargs-stat | 1.743 | 0.32x | 0.22x | 0.58x | 108% |
| find | 2.114 | 0.26x | 0.17x | 0.52x | 134% |

## `dossier`

dirsql median 1.270s

| Native | Median s | dirsql / native | Min | Max | Spread |
|---|---|---|---|---|---|
| find+sort | 1.675 | 0.72x | 0.37x | 0.96x | 82% |

## `frontmatter`

dirsql median 0.262s

| Native | Median s | dirsql / native | Min | Max | Spread |
|---|---|---|---|---|---|
| fd+xargs-P | 0.092 | 2.41x | 2.06x | 2.93x | 36% |
| find+awk | 1.287 | 0.17x | 0.16x | 0.21x | 31% |
| find+xargs-P | 0.105 | 2.15x | 1.74x | 2.96x | 56% |

## `join`

dirsql median 0.628s

| Native | Median s | dirsql / native | Min | Max | Spread |
|---|---|---|---|---|---|
| find+join | 1.358 | 0.42x | 0.33x | 0.56x | 55% |

## `jsonl`

dirsql median 0.890s

| Native | Median s | dirsql / native | Min | Max | Spread |
|---|---|---|---|---|---|
| jq | 1.839 | 0.46x | 0.31x | 0.57x | 56% |
| jq+xargs-P | 0.486 | 1.73x | 1.03x | 2.08x | 61% |

## `notebook`

dirsql median 1.462s

| Native | Median s | dirsql / native | Min | Max | Spread |
|---|---|---|---|---|---|
| python | 1.601 | 0.87x | 0.65x | 1.11x | 52% |

## `transcripts`

dirsql median 0.669s

| Native | Median s | dirsql / native | Min | Max | Spread |
|---|---|---|---|---|---|
| fd+xargs-P | 1.327 | 0.47x | 0.40x | 0.59x | 40% |
| find+python | 1.189 | 0.53x | 0.43x | 0.64x | 39% |
| find+xargs-P | 1.317 | 0.48x | 0.40x | 0.57x | 35% |

## `watch`

dirsql median 0.000s

| Native | Median s | dirsql / native | Min | Max | Spread |
|---|---|---|---|---|---|
| inotifywait | 0.000 | 1.15x | 0.73x | 1.65x | 80% |
