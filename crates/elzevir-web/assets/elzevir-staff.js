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
  const showHint = (radio) => {
    const box = query();
    if (box && radio.dataset.hint) box.placeholder = radio.dataset.hint;
  };
  document.addEventListener("change", (event) => {
    const radio = event.target;
    if (radio instanceof HTMLInputElement && radio.name === "escopo") showHint(radio);
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
      const radio = document.querySelector(`.bar-scopes input[data-shortcut="${letter}"]`);
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
    const open = document.querySelector(".bar-new[open]");
    if (event.key === "Escape" && open) {
      open.open = false;
      open.querySelector("summary")?.focus();
    }
  });
})();
