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

# JQuery
curl -L https://code.jquery.com/jquery-3.7.1.min.js -o jquery.min.js

# Shoelace component library
# WebAwesome is the next version of Shoelace
#jsdl https://cdn.jsdelivr.net/npm/@shoelace-style/shoelace/cdn/ shoelace

# WebAwesome component library
rm -rf webawesome
jsdl https://cdn.jsdelivr.net/npm/@awesome.me/webawesome/dist-cdn/ webawesome

#curl -L https://cdn.jsdelivr.net/npm/@awesome.me/webawesome/dist/llms.txt -o webawesome/llms.txt
