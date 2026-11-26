# Example save: Company Cars (70%)

The save this importer was developed and tested with. It loads in-game under ztrx.

- **Mission:** Company Cars (Chapter 4), 70.29% completion
- **Modified save:** about 999 million in cash, 95% trophy completion
- **Source:** a decrypted PS5 save (`PPSA01547`) shared publicly on the NextGenUpdate forums
  ("GTA 6 Modded Game Save 999 Million Cash and 95% Trophy Completion"). Credit to its uploader.

## Files

| File | What it is |
|---|---|
| `SARC0001` | the decrypted story data (`JSAV`): everything the game loads |
| `ps5-header.bin` | the save's 368-byte PS5 `ASAV` header, used only for the slot's completion % |

Left out on purpose: `param.sfo` (contains the original owner's PSN account ID),
`icon0.png` (Rockstar artwork), `keystone` and the encrypted `chives.dat` (console-bound).
None of them are needed.

## Use

Drag this folder onto `Import-PS5Save.bat` (see the main README), then start the game
with `Play-GTA6-SaveMod.bat`.

---
Imported from https://github.com/LilBigJP/ztrx-ps5-save-import (GPL-3.0) — `examples/company-cars-70pct`. Credit to the original authors. The importer tool itself is not included here.
