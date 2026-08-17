# dsh-frosted-window

English | [中文](README.zh.md)

A DeepSeek Harness Web theme plugin. Upload an image, fill the window, and sit the sidebar, conversation, and details on **one frosted-glass layer**. Official Light / Dark / System stay in charge.

```sh
dsh plugin --profile web add github:SenryLee/dsh-frosted-window
```

Restart `dsh web`, then open **Settings → Frosted**.

![Settings: wallpaper upload, frost sliders, Save / Delete](docs/preview.jpg)

## Features

- Local JPEG / PNG / WebP / GIF upload, no file-size cap
- Full-window wallpaper with the same frost on sidebar, conversation, and details
- Sliders: glass density, blur, saturation, wallpaper dim
- **Save / Delete** so you can preview, then persist, then replace
- Toggle: green on, gray off; turn it off to restore the official look
- Does not steal official appearance: `overrideTokens` only, never `setTheme('custom')`

The image lives in IndexedDB; knobs live in `localStorage`. Nothing is written to `$DSH_HOME/settings.yaml`, and nothing is fetched from a URL.

## Install

`dsh web` must already run (the usual profile name is `web`).

### A. From GitHub (recommended)

```sh
dsh plugin --profile web add github:SenryLee/dsh-frosted-window
```

pnpm ≥ 10 refuses git `prepare` scripts until you allow them. If the first `add` fails, put this in the profile's `pnpm-workspace.yaml` (usually `~/.dsh/profiles/web/pnpm-workspace.yaml`) and `add` again:

```yaml
allowBuilds:
  dsh-frosted-window: true
```

The repo already ships `lib/`. Allowing the build only recompiles on a source install.

### B. From a Release tarball

No `allowBuilds` needed:

```sh
dsh plugin --profile web add https://github.com/SenryLee/dsh-frosted-window/releases/latest/download/dsh-frosted-window-0.1.0.tgz
```

### If `dsh` is not on PATH

```sh
npx @deepseek-ai/dsh plugin --profile web add github:SenryLee/dsh-frosted-window
```

`pnpm` must be on PATH as well.

## Use

1. **Quit and start** `dsh web` again (a page refresh is not enough).
2. Open **Settings** at the bottom of the left rail.
3. Open **Frosted** (or the same panel under **General**, below Appearance).
4. Enable the theme, drop an image, tune the sliders.
5. Click **Save**. The chip reads **Saved** when the next launch will restore it.

Replace: pick another image → **Save**. Clear: **Delete**.

Settings → **Plugins** lists the technical name `frosted-window` / `dsh-frosted-window`. Upload lives on the Frosted settings page, not in that catalog.

## Uninstall

```sh
dsh plugin --profile web remove dsh-frosted-window
```

Then restart `dsh web`.

## Develop

```sh
npm install
npm test
npm run build
```

Ships as `dsh.bundle` + `dsh.client` per the official [package-and-install](https://github.com/deepseek-ai/deepseek-harness/blob/master/docs/user/develop/basic/publish.md) contract.

## License

MIT
