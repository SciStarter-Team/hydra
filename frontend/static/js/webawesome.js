// Web Awesome, with <wa-icon> drawing Font Awesome's icons from this site.
// Left to itself, Web Awesome fetches every icon from Font Awesome's CDN,
// which tells that CDN who is looking at the page. vendor.sh keeps the icons
// at this path, at the version this copy of Web Awesome expects.
//
// Web Awesome finds its own files through a data-webawesome attribute if
// there is one, and otherwise by looking for a <script> named like its
// loader, which would find this one. The attribute has to be in place before
// the loader runs, so the loader is imported only once it is.
//
// The icon path is read when an icon is drawn, and the loader defines its
// components with dynamic imports, which cannot finish before the rest of
// this module has run; so no icon is looked up before the path is set.
document.documentElement.dataset.webawesome = "/-static/webawesome/";

const { setIconPath } = await import("/-static/webawesome/webawesome.loader.js");

setIconPath("/-static/fontawesome/svgs");
