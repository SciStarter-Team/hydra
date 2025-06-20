#!/bin/bash

mkdir -p frontend/static/css
mkdir -p frontend/static/img
mkdir -p frontend/static/js
mkdir -p frontend/static/design
cp -rv ../hydra-frontend-static/build/css frontend/static
cp -rv ../hydra-frontend-static/build/img frontend/static
cp -rv ../hydra-frontend-static/build/js  frontend/static
cp -v ../hydra-frontend-static/build/*.html frontend/static/design
