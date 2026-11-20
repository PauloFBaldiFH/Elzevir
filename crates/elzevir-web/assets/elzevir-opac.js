// SPDX-License-Identifier: GPL-3.0-or-later
// OPAC home (interface §23): the arrows of the "last catalogued" strip. Without this script
// the strip still scrolls sideways, so the arrows stay hidden until it runs.
(() => {
  "use strict";
  const strip = document.getElementById("recent-strip");
  const nav = document.querySelector(".recent-nav");
  if (!strip || !nav) return;
  nav.hidden = false;
  nav.addEventListener("click", (event) => {
    const button = event.target instanceof Element ? event.target.closest("[data-recent]") : null;
    if (!button) return;
    const card = strip.querySelector(".recent-card");
    const step = card ? card.getBoundingClientRect().width + 16 : strip.clientWidth / 2;
    strip.scrollBy({ left: Number(button.dataset.recent) * step, behavior: "smooth" });
  });
})();
