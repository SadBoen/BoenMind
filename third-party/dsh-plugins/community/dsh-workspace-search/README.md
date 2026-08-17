# dsh-workspace-search

[![Awesome DSH Plugin](https://awesome-dsh-plugin.com/badge.svg)](https://github.com/awesome-dsh-plugin/awesome-dsh-plugin)

VS Code-style workspace keyword search for the DeepSeek Harness (dsh) web GUI,
registered as a **Search tab** inside
[dsh-better-sidebar](https://github.com/omdsh-dev/DSH-better-sidebar).

<img width="734" height="238" alt="image" src="https://github.com/user-attachments/assets/0aceb14e-b4f5-4ab4-bf1e-72da35d147b9" />


- Keyword search across the session workspace: file **names** and file
  **contents**, grouped per file with line numbers
- VS Code-style search rules: **files to include / files to exclude** glob
  patterns (comma-separated, supports `**`, `*`, `?`, `{a,b}`), regular
  expression queries (`.*` toggle), and case toggle (`Aa`)
- Click a match to open the file in better-sidebar's built-in editor
- VS Code default excludes: hidden files, `.git`, `node_modules`, `dist`,
  `build`, `.next`, `target` etc. are skipped; caps on files, matches, and
  line length are reported honestly as truncation

## Install

```sh
dsh plugin --profile web add ./plugins/dsh-workspace-search
```

(Requires `dsh-better-sidebar` ≥ 0.4.0, which exposes `ctx.betterSidebar`.)

## Configuration

All fields optional (profile patch layer):

```yaml
- id: workspace-search
  config:
    maxFiles: 5000       # hard cap on files scanned per search
    maxMatches: 300      # hard cap on total content matches
    maxLineLength: 300   # hard cap on one reported line
    maxFileBytes: 1048576  # files above this skip the content scan
```

## How it works

Dual-face bundle. The host half registers a Connection RPC channel
`/workspace-search` (loopback trust fence, same as the built-in `/api`).
The browser half registers a tab through `ctx.betterSidebar.registerTab`.
Search results open in the sidebar editor via `openTab({type: 'editor'})`.

## License

MIT
