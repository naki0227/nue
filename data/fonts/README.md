# Fonts

`muscle/scripts/fetch-fonts.sh` downloads the fonts used by the editing formats from
[google/fonts](https://github.com/google/fonts) into this folder (the Docker
image bakes them in at build time). All are licensed under the SIL Open Font License
1.1; the license texts are saved next to them as `OFL-*.txt`.

| Font (family name for styles) | Used by |
| --- | --- |
| Rounded Mplus 1c ExtraBold / Black (M PLUS Rounded 1c) | shorts, variety captions |
| Dela Gothic One | shorts / variety titles and telops |
| Zen Maru Gothic Medium / Bold | vlog captions |
| Zen Old Mincho Regular / SemiBold | vlog and cinematic titles, cinematic captions |
| Courier Prime | vlog date stamp |
| BIZ UDPGothic Bold | business captions |
| Zen Kaku Gothic New Medium / Bold | business telops and titles |

Add any other font file here and reference its family or full name from a
format in `data/styles/<name>.json`. Do not add fonts whose license forbids
redistribution (for example ラノベPOP) to the repository; keep them local.
Without these files, text falls back to Noto Sans CJK.
