# Mirador — Landing Page

Marketing site for **Mirador**, a cross-platform terminal for AI coding-agent workflows ("a lookout tower over your agents"). Vite + React 18 + TypeScript, plain CSS with Catppuccin Mocha custom properties.

## Develop

```bash
npm install
npm run dev
```

## Build

```bash
npm run build      # type-checks then bundles to dist/
npm run preview    # preview the production build
```

## Editing content

All external links and copy toggles live in `src/config.ts`:

- `GITHUB_URL` — repository link
- `DOWNLOAD_URL` — direct .dmg for the latest release
- `DOWNLOAD_VERSION` — label shown on the download buttons
- `SHOW_APP_MOCKUP`, `SHOW_SHORTCUTS` — section toggles

Palette lives as CSS variables in `src/index.css` (`:root`).

## Structure

```
src/
  main.tsx           entry
  App.tsx            page composition
  config.ts          links + toggles
  index.css          reset, tokens, keyframes, link styles
  components/
    Nav.tsx  Hero.tsx  AppMockup.tsx  Features.tsx
    CliShowcase.tsx  Install.tsx  Keybindings.tsx  Footer.tsx
    Logo.tsx  CopyButton.tsx
```
