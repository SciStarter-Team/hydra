#!/bin/bash

# HTMX in both readable and minified forms
# 4.0.0 is published under the "next" dist-tag, so it must be pinned exactly;
# "@4" would resolve to the 2.x "latest" tag.
HTMX=4.0.0
curl -L https://cdn.jsdelivr.net/npm/htmx.org@$HTMX/dist/htmx.js -o htmx.js
curl -L https://cdn.jsdelivr.net/npm/htmx.org@$HTMX/dist/htmx.min.js -o htmx.min.js

# HTMX extension to enable Server-Sent Events handling
curl -L https://cdn.jsdelivr.net/npm/htmx.org@$HTMX/dist/ext/hx-sse.js -o hx-sse.js
curl -L https://cdn.jsdelivr.net/npm/htmx.org@$HTMX/dist/ext/hx-sse.min.js -o hx-sse.min.js

# HTMX extension providing DOM-oriented reactive client-side scripting
curl -L https://cdn.jsdelivr.net/npm/htmx.org@$HTMX/dist/ext/hx-live.js -o hx-live.js
curl -L https://cdn.jsdelivr.net/npm/htmx.org@$HTMX/dist/ext/hx-live.min.js -o hx-live.min.js




# Official ulid implementation for JS
curl -L https://cdn.jsdelivr.net/npm/ulid@3/dist/browser/index.js -o ulid.js
curl -L https://cdn.jsdelivr.net/npm/ulid@3/dist/browser/index.min.js -o ulid.min.js

# Quill rich text editor
curl -L https://cdn.jsdelivr.net/npm/quill@2/dist/quill.js -o quill.min.js
curl -L https://cdn.jsdelivr.net/npm/quill@2/dist/quill.snow.css -o quill.snow.min.css

# Lit Web Component framework
curl -L https://cdn.jsdelivr.net/gh/lit/dist@3/core/lit-core.min.js -o lit-core.min.js

# MapLibre GL
curl -L https://cdn.jsdelivr.net/npm/maplibre-gl@5/dist/maplibre-gl.min.js -o maplibre-gl.min.js
curl -L https://cdn.jsdelivr.net/npm/maplibre-gl@5/dist/maplibre-gl.min.css -o maplibre-gl.min.css

# Slick
curl -L https://cdn.jsdelivr.net/npm/@accessible360/accessible-slick@1/slick/fonts/slick.woff -o fonts/slick.woff
curl -L https://cdn.jsdelivr.net/npm/@accessible360/accessible-slick@1/slick/fonts/slick.ttf -o fonts/slick.ttf
curl -L https://cdn.jsdelivr.net/npm/@accessible360/accessible-slick@1/slick/slick.min.css -o slick.min.css
curl -L https://cdn.jsdelivr.net/npm/@accessible360/accessible-slick@1/slick/accessible-slick-theme.min.css -o accessible-slick-theme.min.css
curl -L https://cdn.jsdelivr.net/npm/@accessible360/accessible-slick@1/slick/slick.min.js -o slick.min.js

# Web fonts, self-hosted so that no page sends its visitors' addresses to
# Google Fonts. Fontsource publishes the same faces with the same per-script
# unicode-range split, so a browser still downloads only the scripts a page
# uses. Every face goes into fonts/fonts.css, with the files under fonts/.
#
# Each entry is: npm package, the package's stylesheet, the family name it
# declares, and the family name our CSS refers to it by.
FONTSOURCE_VERSION=5.3.0
FONT_FACES=(
  "@fontsource-variable/inter opsz.css 'Inter Variable' Inter"
  "@fontsource-variable/inter opsz-italic.css 'Inter Variable' Inter"
  "@fontsource/barlow 400.css Barlow Barlow"
  "@fontsource/barlow 700.css Barlow Barlow"
  "@fontsource/barlow 800.css Barlow Barlow"
  "@fontsource/barlow 900.css Barlow Barlow"
  # The design mockups use every weight and style of this one.
  "@fontsource/barlow-semi-condensed 100.css 'Barlow Semi Condensed' 'Barlow Semi Condensed'"
  "@fontsource/barlow-semi-condensed 100-italic.css 'Barlow Semi Condensed' 'Barlow Semi Condensed'"
  "@fontsource/barlow-semi-condensed 200.css 'Barlow Semi Condensed' 'Barlow Semi Condensed'"
  "@fontsource/barlow-semi-condensed 200-italic.css 'Barlow Semi Condensed' 'Barlow Semi Condensed'"
  "@fontsource/barlow-semi-condensed 300.css 'Barlow Semi Condensed' 'Barlow Semi Condensed'"
  "@fontsource/barlow-semi-condensed 300-italic.css 'Barlow Semi Condensed' 'Barlow Semi Condensed'"
  "@fontsource/barlow-semi-condensed 400.css 'Barlow Semi Condensed' 'Barlow Semi Condensed'"
  "@fontsource/barlow-semi-condensed 400-italic.css 'Barlow Semi Condensed' 'Barlow Semi Condensed'"
  "@fontsource/barlow-semi-condensed 500.css 'Barlow Semi Condensed' 'Barlow Semi Condensed'"
  "@fontsource/barlow-semi-condensed 500-italic.css 'Barlow Semi Condensed' 'Barlow Semi Condensed'"
  "@fontsource/barlow-semi-condensed 600.css 'Barlow Semi Condensed' 'Barlow Semi Condensed'"
  "@fontsource/barlow-semi-condensed 600-italic.css 'Barlow Semi Condensed' 'Barlow Semi Condensed'"
  "@fontsource/barlow-semi-condensed 700.css 'Barlow Semi Condensed' 'Barlow Semi Condensed'"
  "@fontsource/barlow-semi-condensed 700-italic.css 'Barlow Semi Condensed' 'Barlow Semi Condensed'"
  "@fontsource/barlow-semi-condensed 800.css 'Barlow Semi Condensed' 'Barlow Semi Condensed'"
  "@fontsource/barlow-semi-condensed 800-italic.css 'Barlow Semi Condensed' 'Barlow Semi Condensed'"
  "@fontsource/barlow-semi-condensed 900.css 'Barlow Semi Condensed' 'Barlow Semi Condensed'"
  "@fontsource/barlow-semi-condensed 900-italic.css 'Barlow Semi Condensed' 'Barlow Semi Condensed'"
  "@fontsource-variable/nunito wght.css 'Nunito Variable' Nunito"
  "@fontsource-variable/nunito wght-italic.css 'Nunito Variable' Nunito"
  # The embedded WordPress pages' theme asks for these two as well; see
  # crate::wordpress, which drops its Google Fonts import.
  "@fontsource-variable/montserrat wght.css 'Montserrat Variable' Montserrat"
  "@fontsource-variable/montserrat wght-italic.css 'Montserrat Variable' Montserrat"
  "@fontsource-variable/roboto-slab wght.css 'Roboto Slab Variable' 'Roboto Slab'"
)
rm -rf fonts/inter fonts/barlow fonts/barlow-semi-condensed fonts/nunito fonts/montserrat fonts/roboto-slab
echo "/* Generated by vendor.sh from Fontsource $FONTSOURCE_VERSION. Do not edit. */" > fonts/fonts.css
for face in "${FONT_FACES[@]}"; do
  eval "set -- $face"
  package=$1 sheet=$2 declared=$3 family=$4
  dir=fonts/$(echo "$family" | tr '[:upper:] ' '[:lower:]-')
  mkdir -p "$dir"
  base=https://cdn.jsdelivr.net/npm/$package@$FONTSOURCE_VERSION
  css=$(curl -fsSL "$base/$sheet")
  for file in $(echo "$css" | grep -oE 'url\(\./files/[^)]+\.woff2\)' | sed -E 's/^url\(\.\/files\/(.*)\)$/\1/'); do
    curl -fsSL "$base/files/$file" -o "$dir/$file"
  done
  # Every browser that runs this site's scripts reads woff2, so the woff
  # fallbacks are dropped rather than fetched.
  echo "$css" \
    | sed -E "s#, url\([^)]*\.woff\) format\('woff'\)##; s#url\(\./files/#url(/-static/$dir/#g; s#font-family: '$declared'#font-family: '$family'#" \
    >> fonts/fonts.css
done

# Third-party files this site serves itself instead of having visitors'
# browsers fetch them from a CDN. Each is kept at mirror/<host>/<path>, so its
# local address is its original one with the scheme swapped for
# /-static/mirror/, and every version is pinned in the path. crate::mirror
# looks files up the same way, to point embedded WordPress pages at them.
# shenzi.toml copies mirror/ as it is, without the renaming done elsewhere.
MIRROR_FILES=(
  # jQuery UI, which the embedded WordPress pages load
  https://code.jquery.com/ui/1.14.2/jquery-ui.min.js
  https://code.jquery.com/ui/1.14.2/themes/base/jquery-ui.css
  https://code.jquery.com/ui/1.14.2/themes/base/images/ui-icons_444444_256x240.png
  https://code.jquery.com/ui/1.14.2/themes/base/images/ui-icons_555555_256x240.png
  https://code.jquery.com/ui/1.14.2/themes/base/images/ui-icons_777620_256x240.png
  https://code.jquery.com/ui/1.14.2/themes/base/images/ui-icons_777777_256x240.png
  https://code.jquery.com/ui/1.14.2/themes/base/images/ui-icons_cc0000_256x240.png
  https://code.jquery.com/ui/1.14.2/themes/base/images/ui-icons_ffffff_256x240.png
  # Citizen Science Month's animations and countdown
  https://unpkg.com/@lottiefiles/lottie-player@2.0.12/dist/lottie-player.js
  https://unpkg.com/@pqina/flip@1.8.4/dist/flip.min.js
  # Location search
  https://cdn.jsdelivr.net/npm/@opencage/geosearch-bundle@0.4.0/dist/js/opencage-geosearch-bundle.js
  https://cdn.jsdelivr.net/npm/@opencage/geosearch-bundle@0.4.0/dist/css/autocomplete-theme-classic.min.css
  # Staff maps
  https://unpkg.com/maplibre-gl@4.7.1/dist/maplibre-gl.css
  https://unpkg.com/maplibre-gl@4.7.1/dist/maplibre-gl.js
  https://cdn.jsdelivr.net/npm/@turf/turf@7.4.0/turf.min.js
  # Design mockups
  https://cdn.jsdelivr.net/npm/flowbite@2.5.1/dist/flowbite.css
  https://cdn.jsdelivr.net/npm/flowbite@2.5.1/dist/flowbite.min.js
)
# Component libraries whose loaders fetch further files from beside
# themselves, so the whole directory is kept.
MIRROR_DIRECTORIES=(
  # The exchange pages
  https://cdn.jsdelivr.net/npm/@awesome.me/webawesome@3.0.0-beta.3/dist-cdn/
  # The library network sign-up form
  https://cdn.jsdelivr.net/npm/@shoelace-style/shoelace@2.20.1/cdn/
)
for url in "${MIRROR_FILES[@]}"; do
  path=mirror/${url#https://}
  mkdir -p "$(dirname "$path")"
  curl -fsSL "$url" -o "$path"
done
for url in "${MIRROR_DIRECTORIES[@]}"; do
  path=mirror/${url#https://}
  rm -rf "$path"
  mkdir -p "$path"
  jsdl "$url" "$path"
done

# JQuery
curl -L https://code.jquery.com/jquery-3.7.1.min.js -o jquery.min.js

# Shoelace component library
# WebAwesome is the next version of Shoelace
#jsdl https://cdn.jsdelivr.net/npm/@shoelace-style/shoelace/cdn/ shoelace

# WebAwesome component library
rm -rf webawesome
jsdl https://cdn.jsdelivr.net/npm/@awesome.me/webawesome/dist-cdn/ webawesome

# Font Awesome Free's icons, which <wa-icon> draws by default. Web Awesome
# fetches each from Font Awesome's CDN unless given somewhere else to look, so
# js/webawesome.js points it here. The version is the one this copy of Web
# Awesome was built against.
FONTAWESOME=$(grep -ohE 'FA_VERSION = "[^"]+"' webawesome/chunks/*.js | head -1 | cut -d'"' -f2)
: "${FONTAWESOME:?no FA_VERSION found in webawesome/chunks}"
rm -rf fontawesome
mkdir -p fontawesome/svgs
jsdl https://cdn.jsdelivr.net/npm/@fortawesome/fontawesome-free@$FONTAWESOME/svgs/ fontawesome/svgs

#curl -L https://cdn.jsdelivr.net/npm/@awesome.me/webawesome/dist/llms.txt -o webawesome/llms.txt
