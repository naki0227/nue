#!/usr/bin/env sh
# Downloads the open-licensed (SIL OFL 1.1) fonts used by the editing formats
# into data/fonts. Without them, text falls back to Noto Sans CJK.
set -eu
out="${1:-$(dirname "$0")/../../data/fonts}"
base="https://raw.githubusercontent.com/google/fonts/main/ofl"
mkdir -p "$out"
for f in \
  mplusrounded1c/MPLUSRounded1c-ExtraBold.ttf \
  mplusrounded1c/MPLUSRounded1c-Black.ttf \
  delagothicone/DelaGothicOne-Regular.ttf \
  zenmarugothic/ZenMaruGothic-Medium.ttf \
  zenmarugothic/ZenMaruGothic-Bold.ttf \
  zenoldmincho/ZenOldMincho-Regular.ttf \
  zenoldmincho/ZenOldMincho-SemiBold.ttf \
  courierprime/CourierPrime-Regular.ttf \
  bizudpgothic/BIZUDPGothic-Bold.ttf \
  zenkakugothicnew/ZenKakuGothicNew-Bold.ttf \
  zenkakugothicnew/ZenKakuGothicNew-Medium.ttf
do
  name=$(basename "$f")
  if [ ! -s "$out/$name" ]; then
    curl -fsSL --retry 3 -o "$out/$name" "$base/$f"
  fi
done
# Keep the licenses next to the fonts (M PLUS Rounded 1c is OFL per its
# METADATA.pb but ships no OFL.txt in google/fonts).
for d in delagothicone zenmarugothic zenoldmincho courierprime bizudpgothic zenkakugothicnew; do
  [ -s "$out/OFL-$d.txt" ] || curl -fsSL --retry 3 -o "$out/OFL-$d.txt" "$base/$d/OFL.txt" || true
done
echo "Fonts in $out"
