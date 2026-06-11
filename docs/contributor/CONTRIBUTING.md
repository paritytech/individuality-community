# Contributing

Individuality is open source, but **this repository does not accept external pull requests at this time.**

Development happens in a separate, internal repository. This repository contains all public releases and receives all changes in waves.

You are welcome to:

- **Open issues** — bug reports, questions, and suggestions are appreciated. See [Issues](#issues).
- **Read and build the code** — it is public; see [Building](#building).

## Building

Install the dependencies and follow the steps in the [README](../../README.md#-getting-started) and the [Launch guide](../launch.md).

### Debug builds

To improve build times for debug builds, the workspace `Cargo.toml` emits source-line debug information only. If you need full debug info in your local debug builds, find the line `debug = "line-tables-only"` and comment it out.

## Issues

Before opening a new issue, search to see if a similar one already exists and comment there if so. When filing a new issue, include enough detail to reproduce the problem.

## Code of Conduct

In every interaction and contribution, this project adheres to the [Contributor Covenant Code of Conduct](./CODE_OF_CONDUCT.md).
