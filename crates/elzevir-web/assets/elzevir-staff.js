// SPDX-License-Identifier: GPL-3.0-or-later
// Keyboard for the staff header (interface §22 item 6): Alt+letter picks a search scope,
// Alt+N opens "New" and "/" goes to the search box. The header arrives by htmx after the
// page loads, so every handler is delegated from the document.
(() => {
  "use strict";
  const query = () => document.getElementById("bar-query");
  const typing = (element) =>
    element instanceof HTMLElement &&
    (element.isContentEditable || ["INPUT", "TEXTAREA", "SELECT"].includes(element.tagName));
  // The chip inside the search box names the chosen scope, in its tone, as in the draft.
  const showHint = (radio) => {
    const box = query();
    if (box && radio.dataset.hint) box.placeholder = radio.dataset.hint;
    const label = document.getElementById("bar-scope-label");
    if (!label) return;
    label.className = `active-scope tone-${radio.dataset.tone}`;
    label.querySelector("use")?.setAttribute("href", `/static/elzevir-icons.svg#${radio.dataset.icon}`);
    const name = label.querySelector("span");
    if (name) name.textContent = radio.dataset.label;
  };
  document.addEventListener("change", (event) => {
    const radio = event.target;
    if (radio instanceof HTMLInputElement && radio.name === "escopo") {
      showHint(radio);
      query()?.focus();
    }
  });
  // A click outside an open menu closes it.
  document.addEventListener("click", (event) => {
    for (const menu of document.querySelectorAll("details.menu[open]")) {
      if (!(event.target instanceof Node) || !menu.contains(event.target)) menu.open = false;
    }
  });
  document.addEventListener("keydown", (event) => {
    if (event.altKey && !event.ctrlKey && !event.metaKey && event.code.startsWith("Key")) {
      // The physical key, so Alt still works where it types accented letters (macOS).
      const letter = event.code.slice(3).toLowerCase();
      const menu = document.querySelector(".bar-new");
      if (letter === "n" && menu) {
        event.preventDefault();
        menu.open = !menu.open;
        if (menu.open) menu.querySelector("a")?.focus();
        return;
      }
      const radio = document.querySelector(`.scope-actions input[data-shortcut="${letter}"]:not(:disabled)`);
      if (radio instanceof HTMLInputElement) {
        event.preventDefault();
        radio.checked = true;
        showHint(radio);
        query()?.focus();
      }
      return;
    }
    if (event.key === "/" && !typing(event.target) && query()) {
      event.preventDefault();
      query().focus();
      return;
    }
    const open = document.querySelector("details.menu[open]");
    if (event.key === "Escape" && open) {
      open.open = false;
      open.querySelector("summary")?.focus();
    }
  });
})();

// Zoom, cords and the crossing of the wall (interface §24). The zoom is only an attribute
// on the page; the cookie lets the server send the next page already in the chosen zoom.
(() => {
  "use strict";
  const SVG = "http://www.w3.org/2000/svg";
  const CROSS = "elzevir-cross";
  const ARRIVED = "elzevir-arrived";
  const reduced = () => window.matchMedia("(prefers-reduced-motion: reduce)").matches;
  const root = () => document.querySelector("[data-zoom-screen]");
  let firstDraw = true;

  const drawCords = () => {
    const wall = document.querySelector(".wall");
    const svg = wall?.querySelector(".wall-cords");
    const target = wall?.querySelector(".patron-port");
    if (!wall || !svg) return;
    svg.replaceChildren();
    if (!target || getComputedStyle(svg).display === "none") return;
    const box = wall.getBoundingClientRect();
    const centre = (element) => {
      const r = element.getBoundingClientRect();
      return [r.left + r.width / 2 - box.left, r.top + r.height / 2 - box.top];
    };
    const [tx, ty] = centre(target);
    for (const port of wall.querySelectorAll(".loan-port")) {
      const [x, y] = centre(port);
      const middle = (x + tx) / 2;
      const path = document.createElementNS(SVG, "path");
      path.setAttribute("d", `M${x} ${y} C${middle} ${y} ${middle} ${ty} ${tx} ${ty}`);
      const loan = port.closest(".loan");
      if (loan?.classList.contains("urgency-late")) path.classList.add("late");
      if (firstDraw && loan?.classList.contains("loan-new")) path.classList.add("drawn-new");
      svg.append(path);
    }
    firstDraw = false;
  };

  const setZoom = (mode, animate) => {
    const page = root();
    if (!page || page.dataset.zoom === mode) return;
    const apply = () => {
      page.dataset.zoom = mode;
      const full = mode === "full";
      const button = page.querySelector("[data-zoom-toggle]");
      if (button) {
        button.setAttribute("aria-pressed", String(full));
        button.title = full ? button.dataset.tipFull : button.dataset.tipCompact;
      }
      const status = page.querySelector("[data-zoom-status]");
      if (status) status.textContent = full ? status.dataset.saidFull : status.dataset.saidCompact;
      const secure = window.location.protocol === "https:" ? "; secure" : "";
      document.cookie = `elzevir_zoom_${page.dataset.zoomScreen}=${mode}; path=/equipe; max-age=31536000; samesite=lax${secure}`;
      drawCords();
    };
    if (animate && document.startViewTransition && !reduced()) {
      document.startViewTransition(apply).finished.then(drawCords, drawCords);
    } else {
      apply();
    }
  };
  const toggle = () => {
    const page = root();
    if (page) setZoom(page.dataset.zoom === "full" ? "compact" : "full", true);
  };

  document.addEventListener("click", (event) => {
    if (!(event.target instanceof Element)) return;
    if (event.target.closest("[data-zoom-toggle]")) {
      toggle();
      return;
    }
    // A link to what only the full view shows opens it first, so the anchor exists.
    if (event.target.closest("[data-zoom-to]")) setZoom("full", false);
    const card = event.target.closest("[data-cross]");
    const arrow = event.target.closest(".pill-arrow");
    const item = card?.dataset.cross ?? arrow?.getAttribute("href")?.split("/").pop();
    if (item) {
      try {
        sessionStorage.setItem(CROSS, JSON.stringify({ item, to: card ? "catalog" : "patron" }));
      } catch {
        // Without storage the page still opens; only the animation is lost.
      }
    }
  });
  document.addEventListener("keydown", (event) => {
    if (event.altKey && !event.ctrlKey && !event.metaKey && event.code === "KeyZ" && root()) {
      event.preventDefault();
      toggle();
    }
  });

  const read = (key) => {
    try {
      return JSON.parse(sessionStorage.getItem(key) ?? "null");
    } catch {
      return null;
    }
  };
  const named = (item) => document.querySelector(`[data-item="${CSS.escape(String(item))}"]`);
  // Only a crossing animates; any other page change stays as it was.
  window.addEventListener("pageswap", (event) => {
    const transition = event.viewTransition;
    if (!transition) return;
    let cross = read(CROSS);
    const arrived = read(ARRIVED);
    if (!cross && arrived?.url === window.location.href && event.activation?.navigationType === "traverse") {
      cross = { item: arrived.item, to: arrived.to === "catalog" ? "patron" : "catalog" };
      try {
        sessionStorage.setItem(CROSS, JSON.stringify(cross));
      } catch {
        // The way back then has no animation.
      }
    }
    if (!cross) {
      // Skipping rejects `ready`; nobody waits on it, so the rejection is not an error.
      transition.ready.catch(() => {});
      transition.skipTransition();
      return;
    }
    transition.types?.add(`to-${cross.to}`);
    const own = named(cross.item);
    if (own instanceof HTMLElement) own.style.viewTransitionName = "crossing";
  });
  window.addEventListener("pagereveal", (event) => {
    const cross = read(CROSS);
    try {
      sessionStorage.removeItem(CROSS);
      if (cross) sessionStorage.setItem(ARRIVED, JSON.stringify({ ...cross, url: window.location.href }));
    } catch {
      // Nothing to keep.
    }
    const transition = event.viewTransition;
    if (!transition) return;
    if (!cross) {
      // Skipping rejects `ready`; nobody waits on it, so the rejection is not an error.
      transition.ready.catch(() => {});
      transition.skipTransition();
      return;
    }
    transition.types?.add(`to-${cross.to}`);
    const target = named(cross.item);
    if (target instanceof HTMLElement) {
      target.style.viewTransitionName = "crossing";
      transition.finished.finally(() => {
        target.style.viewTransitionName = "";
      });
    }
  });

  const start = () => {
    const wall = document.querySelector(".wall");
    if (!wall) return;
    drawCords();
    new ResizeObserver(drawCords).observe(wall);
  };
  if (document.readyState === "loading") document.addEventListener("DOMContentLoaded", start);
  else start();
})();
