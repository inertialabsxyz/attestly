# `docs/site/` — Attestly documentation site

Source for the documentation site hosted at
[`docs.attestly.xyz`](https://docs.attestly.xyz). Built with
[`mkdocs`](https://www.mkdocs.org/) and the
[`mkdocs-material`](https://squidfunk.github.io/mkdocs-material/) theme.

## Local preview

```bash
pip install mkdocs mkdocs-material
cd docs/site
mkdocs serve
# → http://127.0.0.1:8000
```

## Build for deploy

```bash
mkdocs build
# → ./site/ static output
```

## Sections

- **Quickstart** — the 60-second path that mirrors
  `https://attestly.xyz/quickstart`
- **Concepts** — attestations, sinks, verifiers, why it matters
- **LangGraph integration** — full SDK reference
- **Self-hosted** — OSS-only path (FileSink, static viewer, no cloud)
- **Compliance** — pointers to the SR 11-7 mapping in
  `docs/compliance/SR_11_7.md`
- **Migration** — moving from `FileSink` to `CloudSink` without losing
  attestations

Each section has at least one runnable example.

## Why not Docusaurus

`mkdocs-material` is dependency-light (Python only), renders fast, and
does not lock the project into a JS toolchain. The trade-off is a
slightly smaller theming surface; the trade-off is worth it at this
stage. Docusaurus remains a candidate if the docs grow past what
`material` handles ergonomically.
