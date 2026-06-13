# Command Change Requirements

- Every time a command or subcommand is added, changed, or removed, the typeahead/autocomplete MUST be synced.
- Every time a command is added, changed, or removed, the help output MUST be synced.
- Every command and subcommand MUST support `-h`, `--help`, and `<cmd> <sub> help` to display command/subcommand-specific help.
- Every command alias (for example `history clear` and `clear --history`) MUST be reflected in autocomplete and help output.
- Every command behavior change MUST preserve keyboard UX guarantees (`Enter` submit, `Tab` autocomplete, `ArrowUp/ArrowDown` history recall, `Ctrl+C` input clear) unless explicitly changed in phase scope.
- Every command change MUST be validated with `make build` before closing the task.
