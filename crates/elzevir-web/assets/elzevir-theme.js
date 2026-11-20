// SPDX-License-Identifier: GPL-3.0-or-later
// Light or dark theme (interface §22 item 9). Loaded synchronously in <head> so the saved
// choice applies before the first paint; without it the page follows the system.
(() => {
  "use strict";
  const KEY = "elzevir-theme";
  const root = document.documentElement;
  const read = () => {
    try {
      return localStorage.getItem(KEY);
    } catch {
      return null; // Storage blocked: follow the system.
    }
  };
  const saved = read();
  if (saved === "light" || saved === "dark") root.dataset.theme = saved;
  // The switch is hidden until this script runs, so a page without scripts never shows a
  // button that does nothing.
  root.classList.add("theme-switchable");
  const current = () =>
    root.dataset.theme ||
    (window.matchMedia("(prefers-color-scheme: dark)").matches ? "dark" : "light");
  document.addEventListener("click", (event) => {
    if (!(event.target instanceof Element) || !event.target.closest("[data-theme-toggle]")) return;
    const next = current() === "dark" ? "light" : "dark";
    root.dataset.theme = next;
    try {
      localStorage.setItem(KEY, next);
    } catch {
      // Not saved: the choice lasts for this page only.
    }
  });
})();
