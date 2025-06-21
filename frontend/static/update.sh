#!/bin/bash

# HTMX in both readable and minified forms
curl -L https://cdn.jsdelivr.net/npm/htmx.org@2/dist/htmx.js -o htmx.js
curl -L https://cdn.jsdelivr.net/npm/htmx.org@2/dist/htmx.min.js -o htmx.min.js

# HTMX extension to enable Server-Sent Events handling
curl -L https://cdn.jsdelivr.net/npm/htmx-ext-sse@2/sse.js -o hx-sse.js
curl -L https://cdn.jsdelivr.net/npm/htmx-ext-sse@2/sse.min.js -o hx-sse.js

# HTMX extension to enable using the Alpine morph plugin
curl -L https://cdn.jsdelivr.net/npm/htmx-ext-alpine-morph@2/alpine-morph.js -o hx-alpine-morph.js
curl -L https://cdn.jsdelivr.net/npm/htmx-ext-alpine-morph@2/alpine-morph.min.js -o hx-alpine-morph.min.js

# Alpine in both readable and minified forms
curl -L https://cdn.jsdelivr.net/npm/alpinejs@3/dist/cdn.js -o alpine.js
curl -L https://cdn.jsdelivr.net/npm/alpinejs@3/dist/cdn.min.js -o alpine.min.js

# Alpine morph plugin in both readable and minified forms
curl -L https://cdn.jsdelivr.net/npm/@alpinejs/morph@3/dist/cdn.js -o alpine-morph.js
curl -L https://cdn.jsdelivr.net/npm/@alpinejs/morph@3/dist/cdn.min.js -o alpine-morph.min.js

# Official ulid implementation for JS
curl -L https://cdn.jsdelivr.net/npm/ulid@3/dist/browser/index.js -o ulid.js
curl -L https://cdn.jsdelivr.net/npm/ulid@3/dist/browser/index.min.js -o ulid.min.js

# Quill rich text editor
curl -L https://cdn.jsdelivr.net/npm/quill@2/dist/quill.js -o quill.min.js
curl -L https://cdn.jsdelivr.net/npm/quill@2/dist/quill.snow.css -o quill.snow.min.css

# Lit Web Component framework
curl -L https://cdn.jsdelivr.net/gh/lit/dist@3/core/lit-core.min.js -o lit-core.min.js

