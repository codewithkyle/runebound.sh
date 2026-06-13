# Command Change Requirements

- Every time a command or subcommand is added, changed, or removed, the typeahead/autocomplete MUST be synced.
- Every time a command is added, changed, or removed, the help output MUST be synced.
- Every command and subcommand MUST support `-h`, `--help`, and `<cmd> <sub> help` to display command/subcommand-specific help.
- If a command requires subcommands, clicking that root command in output MUST execute its help command (for example `config` click -> `config --help`).
- Every command alias (for example `history clear` and `clear --history`) MUST be reflected in autocomplete and help output.
- Every valid command and subcommand shown in output/help/history MUST be rendered as clickable and executable.
- Every command behavior change MUST preserve keyboard UX guarantees (`Enter` submit, `Tab` autocomplete, `ArrowUp/ArrowDown` history recall, `Ctrl+C` input clear) unless explicitly changed in phase scope.
- Every command change MUST be validated with `make build` before closing the task.
