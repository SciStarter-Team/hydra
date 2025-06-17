#!/bin/bash

# HTMX in both readable and minified forms
curl -L https://unpkg.com/htmx.org@2/dist/htmx.js -o htmx.js
curl -L https://unpkg.com/htmx.org@2/dist/htmx.min.js -o htmx.min.js

# HTMX extension to enable Server-Sent Events handling
curl -L https://unpkg.com/htmx-ext-sse/sse.js -o hx-sse.js

# HTMX extension to enable using the Alpine morph plugin
curl -L https://unpkg.com/htmx-ext-alpine-morph/alpine-morph.js -o hx-alpine-morph.js

# Alpine in both readable and minified forms
curl -L https://unpkg.com/alpinejs/dist/cdn.js -o alpine.js
curl -L https://unpkg.com/alpinejs/dist/cdn.min.js -o alpine.min.js

# Alpine morph plugin in both readable and minified forms
curl -L https://unpkg.com/@alpinejs/morph/dist/cdn.js -o alpine-morph.js
curl -L https://unpkg.com/@alpinejs/morph/dist/cdn.min.js -o alpine-morph.min.js

# Official ulid implementation for JS
#curl -L https://cdn.jsdelivr.net/npm/ulid@2.4.0/dist/index.umd.js -o ulid.js
#curl -L https://cdn.jsdelivr.net/npm/ulid@2.4.0/dist/index.umd.min.js -o ulid.min.js
curl -L https://cdn.jsdelivr.net/npm/ulid@3/dist/browser/index.js -o ulid.js
curl -L https://cdn.jsdelivr.net/npm/ulid@3/dist/browser/index.min.js -o ulid.min.js

# Quill rich text editor
curl -L https://cdn.jsdelivr.net/npm/quill@2/dist/quill.js -o quill.js
curl -L https://cdn.jsdelivr.net/npm/quill@2/dist/quill.snow.css -o quill.snow.css
# We don't have a non-minified equivalents for Quill
cp quill.js quill.min.js
cp quill.snow.css quill.snow.min.css

# Lit Web Component framework
curl -L https://cdn.jsdelivr.net/gh/lit/dist@3/core/lit-core.min.js -o lit-core.js
cp lit-core.js lit-core.min.js
