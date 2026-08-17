# Contributing

Thanks for considering contributing to dsh-catppuccin!

## Workflow

1. Fork the repository and create a feature branch off `main`.
2. Make your changes. Do not hand-edit theme colors — regenerate them with the
   generator script:
   ```sh
   node scripts/gen-themes.mjs
   ```
3. Commit and open a pull request.

## Theme colors

Theme data is generated from the official
[catppuccin/palette](https://github.com/catppuccin/palette) `palette.json`
(check it in under `palette/palette.json`). The Catppuccin color → `--dsw-alias-*`
token mapping lives in `scripts/gen-themes.mjs`. To change a color assignment,
edit the mapping there and re-run the script — never edit `themes/*.json`
or the token tables in `lib/client.js` by hand.

## Style

- Keep the browser bundle (`lib/client.js`) in the same hand-written
  `window.__ModuleLoader__` CJS format as the shipped ui-* packages; no build
  step required.
- Follow the Catppuccin [style guide](https://github.com/catppuccin/catppuccin)
  for any palette-related naming.

## License

All contributions are licensed under the MIT license.
