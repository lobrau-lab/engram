# Engram Design Resources

Place visual assets for the application in this directory. Keep source files
editable when possible (`.fig`, `.svg`, `.xcf`, or layered originals) and add
exported assets beside them.

## Asset brief

The first UI pass needs **8 visual assets**:

| # | Filename | Purpose | Recommended size | Format |
|---|---|---|---|---|
| 1 | `app-icon` | Desktop application icon and favicon | 1024×1024 | SVG + PNG |
| 2 | `wordmark-light` | Logo on dark UI surfaces | 1200×300 | SVG + transparent PNG |
| 3 | `wordmark-dark` | Logo on light UI surfaces | 1200×300 | SVG + transparent PNG |
| 4 | `empty-vault` | Empty-vault welcome screen illustration | 1200×800 | SVG or PNG |
| 5 | `empty-search` | No-search-results illustration | 800×600 | SVG or PNG |
| 6 | `empty-inbox` | Dump/inbox guidance illustration | 1000×700 | SVG or PNG |
| 7 | `backup-recovery` | Backup, USB, and recovery screen illustration | 1000×700 | SVG or PNG |
| 8 | `assistant-orb` | Local AI assistant avatar/status mark | 512×512 | SVG + PNG |

## Design guidance

- Prefer simple, legible artwork that works at small sizes.
- Provide light and dark variants for assets used on changing surfaces.
- Avoid text embedded in illustrations; UI copy must remain accessible and
  translatable.
- Use transparent backgrounds unless an asset explicitly needs a full-bleed
  background.
- Do not include personal data, cloud-service branding, or remote URLs.
- Keep important details inside a safe 10% margin for responsive cropping.
- Use `viewBox`-based SVGs and avoid embedded raster images where practical.

## Planned UI surfaces

The assets will be used in:

1. application shell and startup screen;
2. empty vault onboarding;
3. search, inbox, and crawler empty states;
4. backup and recovery flows; and
5. local assistant status and conversation views.

When adding an asset, include a short note describing its intended emotional
tone, colors, light/dark behavior, and any animation constraints.
