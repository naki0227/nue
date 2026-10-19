#!/usr/bin/env sh
# Regenerates the bundled synthetic sound effects in data/se with ffmpeg only.
# They are original, license-free sounds; drop your own <name>.wav|mp3 files
# next to them to extend the library (the file name is the SE "type").
set -eu
out="${1:-$(dirname "$0")/../data/se}"
mkdir -p "$out"
gen() { # name duration expression [extra filters]
  ffmpeg -y -hide_banner -loglevel error -f lavfi \
    -i "aevalsrc='$3':s=48000:d=$2" -af "${4:-anull},afade=t=out:st=$(awk "BEGIN{print $2*0.7}"):d=$(awk "BEGIN{print $2*0.3}"),alimiter=limit=0.9" \
    -ac 2 "$out/$1.wav"
}
# Deep hit: falling sine with a noisy transient.
gen impact 0.6 "0.9*sin(2*PI*(55+90*exp(-t*18))*t)*exp(-t*6)+0.3*(random(0)*2-1)*exp(-t*60)"
# Air sweep: band-passed noise swelling in and out.
gen whoosh 0.7 "(random(0)*2-1)*sin(PI*t/0.7)^2" "bandpass=f=1200:width_type=o:w=2,highpass=f=300"
# Bubble pop: very short rising blip.
gen pop 0.15 "sin(2*PI*(400+2600*t)*t)*exp(-t*35)"
# Bell: two inharmonic partials.
gen ding 1.2 "0.6*sin(2*PI*1320*t)*exp(-t*3)+0.3*sin(2*PI*3300*t)*exp(-t*5)"
# Cartoon spring: wobbling pitch.
gen boing 0.7 "sin(2*PI*(180+60*sin(2*PI*14*t))*t)*exp(-t*3.5)"
# Quiz-show wrong answer.
gen buzzer 0.6 "0.25*sgn(sin(2*PI*110*t))+0.15*sgn(sin(2*PI*116*t))" "lowpass=f=1800"
# Glitter: fast high arpeggio.
gen sparkle 0.9 "sin(2*PI*(1800+600*floor(t*12))*t)*exp(-mod(t,1/12)*30)*exp(-t*2)"
echo "Generated sound effects in $out"
