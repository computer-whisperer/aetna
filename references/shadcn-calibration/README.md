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
npm run dev -- --host 127.0.0.1
npm run screenshot
```

The screenshot target uses an `1180x780` viewport so it lines up with
`crates/aetna-core/examples/polish_calibration.rs`'s logical viewport.

Outputs:

- `out/shadcn-calibration.png` — local steelman for Aetna's first fixture.
- `out/shadcn-dashboard-01.png` — local dashboard-01-style density target.

The dashboard page is available at `/?view=dashboard-01`. It is modeled
after shadcn's dashboard block shape: sidebar, section cards, chart
region, recent activity, and dense data table.
