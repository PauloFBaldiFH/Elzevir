# Static assets

| File | Source | License |
|---|---|---|
| `htmx-2.0.10.min.js` | npm `htmx.org@2.0.10`, `dist/htmx.min.js`, SHA-256 `71ea67185bfa8c98c39d31717c6fce5d852370fcdfd129db4543774d3145c0de` | 0BSD |
| `elzevir.css` | Elzevir | GPL-3.0-or-later |
| `elzevir-mark.svg` | Elzevir, from `docs/marca/elzevir-logo.svg` without the wordmark | GPL-3.0-or-later |
| `elzevir-theme.js` | Elzevir: light/dark switch (interface §22) | GPL-3.0-or-later |
| `elzevir-staff.js` | Elzevir: staff header keyboard (interface §22) | GPL-3.0-or-later |
| `elzevir-opac.js` | Elzevir: OPAC home strip buttons (interface §23) | GPL-3.0-or-later |

Assets are compiled into the binary (`include_bytes!`); there is no CDN dependency.
