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
