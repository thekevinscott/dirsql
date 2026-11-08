**Fixed** The pip and npm launchers no longer read an argument after `--` as
a config path when it starts with `-c` or `--config`. Only `-c`/`--config`
flags before the first `--` name configs, as clap parses them.
