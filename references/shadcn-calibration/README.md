# Shadcn Calibration Reference

This side harness produces local reference screenshots for Aetna's polish
calibration loop. It is intentionally isolated from the Rust workspace.

Setup:

```bash
npm install
npm run build
```

Screenshots:

```bash
npm run capture
```

The capture script starts Vite on a free local port, opens Chromium through
Playwright, and pins the browser to a deterministic scale contract:

- viewport: `1180x780` CSS px by default,
- Playwright `deviceScaleFactor`: `1` by default,
- Chromium forced device scale factor: `1`,
- browser zoom: expected to remain at `1`,
- shadcn UI scale: controlled by root `font-size`, not desktop zoom.

Override these with environment variables:

```bash
SHADCN_REFERENCE_WIDTH=1440 \
SHADCN_REFERENCE_HEIGHT=900 \
SHADCN_REFERENCE_PORT=5173 \
SHADCN_REFERENCE_DSF=1 \
SHADCN_REFERENCE_UI_SCALE=1 \
npm run capture
```

Use `SHADCN_REFERENCE_UI_SCALE` to model app-level UI scale. Keep
`SHADCN_REFERENCE_DSF=1` unless the goal is explicitly testing raster
behavior; Aetna calibration compares logical layout first, then backend pixels.

Outputs:

- `out/shadcn-calibration.png` — local steelman for Aetna's first fixture.
- `out/shadcn-dashboard-01.png` — local dashboard-01-style density target.
- matching `out/*.json` files — capture metadata with actual DPR, viewport,
  visual viewport scale, and root font size.

The reference app marks major surfaces with `data-calibration-boundary`.
`npm run capture` fails before writing a screenshot if a visible descendant
overflows one of those boundaries. This is intentionally similar to Aetna's
lint loop: broken reference screenshots should not become calibration targets.

The dashboard page is available at `/?view=dashboard-01`. It is modeled
after shadcn's dashboard block shape: sidebar, section cards, chart
region, recent activity, and dense data table.
