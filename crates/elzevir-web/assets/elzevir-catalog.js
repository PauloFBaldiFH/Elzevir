// SPDX-License-Identifier: GPL-3.0-or-later
// Cords and focus of the staff catalog listing (docs/specs/catalogo/listagem.md, sketch v9): one
// Bezier curve from each parent block to each child block, with the label the server wrote
// in Fluent. A block may have several parents: `data-parent` lists them separated by
// spaces and `data-cord-label` lists their labels, in the same order, separated by `|`.
// A cord never runs behind a block: between neighbouring columns it stays in the gap, and
// when it skips a column (an authority straight to its Work, past the Hub) it crosses that
// column through a free lane above, between or below the blocks there.
// The bar's toggles hide the Hub and Authorities columns; the choice is a per-viewer
// convenience kept in localStorage. No inline script or style: the CSP forbids them, so
// everything is set from this module.
const SVG = "http://www.w3.org/2000/svg";
// Below this horizontal distance a label would cover the blocks it joins.
const MIN_LABEL_DISTANCE = 45;
// Clearance between a cord crossing a column and the blocks it passes.
const LANE_MARGIN = 10;

function element(name, attributes) {
  const node = document.createElementNS(SVG, name);
  for (const [key, value] of Object.entries(attributes)) node.setAttribute(key, String(value));
  return node;
}

function cordClass(parent) {
  if (parent.classList.contains("card-auth")) return "cord-path dashed";
  if (parent.classList.contains("card-hub") || parent.classList.contains("card-work")) {
    return "cord-path primary";
  }
  return "cord-path";
}

// The y where a cord can cross the blocks in its way: the one closest to where it is going.
function lane(target, obstacles, height) {
  const blocked = (y) => obstacles.some((o) => y > o.top - LANE_MARGIN && y < o.bottom + LANE_MARGIN);
  if (!blocked(target)) return target;
  const candidates = obstacles
    .flatMap((o) => [o.top - LANE_MARGIN, o.bottom + LANE_MARGIN])
    .filter((y) => y >= 0 && y <= height && !blocked(y));
  return candidates.reduce((best, y) => (Math.abs(y - target) < Math.abs(best - target) ? y : best), height);
}

function route(link, cards, box) {
  const { x1, x2 } = link;
  const obstacles = cards
    .filter((card) => card.left - box.left < x2 - 1 && card.right - box.left > x1 + 1)
    .map((card) => ({ left: card.left - box.left, right: card.right - box.left, top: card.top - box.top, bottom: card.bottom - box.top }));
  if (obstacles.length === 0) return link;
  const xa = Math.min(...obstacles.map((o) => o.left));
  const xb = Math.max(...obstacles.map((o) => o.right));
  if (xa - x1 < 8 || x2 - xb < 8) return link;
  return { ...link, lane: { xa, xb, y: lane(link.y2, obstacles, box.height) } };
}

function curve(x1, y1, x2, y2) {
  const dx = Math.abs(x2 - x1) * 0.48;
  return `C ${x1 + dx} ${y1}, ${x2 - dx} ${y2}, ${x2} ${y2}`;
}

function measure(family) {
  const box = family.getBoundingClientRect();
  const blocks = [...family.querySelectorAll(".card, .item-pill")];
  const rects = new Map(blocks.map((block) => [block, block.getBoundingClientRect()]));
  const links = [];
  const labelled = new Set();
  const ports = new Map();
  for (const child of family.querySelectorAll("[data-parent]")) {
    const parents = child.dataset.parent.split(" ").filter(Boolean);
    const labels = (child.dataset.cordLabel || "").split("|");
    const to = child.getBoundingClientRect();
    parents.forEach((id, index) => {
      const parent = family.querySelector(`[data-node="${id}"]`);
      if (!parent) return;
      const from = parent.getBoundingClientRect();
      if (from.width < 10 || to.width < 10 || to.left < from.right) return; // stacked
      // One label per parent and kind of link: its children share the meaning.
      const label = labels[index] || "";
      const key = `${id}|${label}`;
      const show = label !== "" && !labelled.has(key);
      if (show) labelled.add(key);
      const x1 = from.right - box.left;
      const y1 = from.top + from.height / 2 - box.top;
      ports.set(id, { x: x1, y: y1, auth: parent.classList.contains("card-auth") });
      const others = blocks.filter((block) => block !== parent && block !== child).map((block) => rects.get(block)).filter((rect) => rect.width > 0);
      const link = {
        x1,
        y1,
        x2: to.left - box.left,
        y2: to.top + to.height / 2 - box.top,
        kind: cordClass(parent),
        label: show ? label : "",
      };
      links.push(route(link, others, box));
    });
  }
  return { width: box.width, height: box.height, links, ports: [...ports.values()] };
}

function draw(family, { width, height, links, ports }) {
  let svg = family.querySelector(":scope > svg.cords");
  if (!svg) {
    svg = element("svg", { class: "cords", "aria-hidden": "true" });
    family.prepend(svg);
  }
  svg.setAttribute("width", String(width));
  svg.setAttribute("height", String(height));
  const paths = [];
  const labels = [];
  for (const { x1, y1, x2, y2, kind, label, lane: crossing } of links) {
    const d = crossing
      ? `M ${x1} ${y1} ${curve(x1, y1, crossing.xa, crossing.y)} L ${crossing.xb} ${crossing.y} ${curve(crossing.xb, crossing.y, x2, y2)}`
      : `M ${x1} ${y1} ${curve(x1, y1, x2, y2)}`;
    paths.push(element("path", { class: kind, d }));
    if (!label || Math.abs(x2 - x1) <= MIN_LABEL_DISTANCE) continue;
    // A routed cord carries its label on the straight stretch across the skipped column.
    const midX = crossing ? (crossing.xa + crossing.xb) / 2 : (x1 + x2) / 2;
    const midY = crossing ? crossing.y : (y1 + y2) / 2;
    const labelWidth = label.length * 6.2 + 12;
    labels.push(
      element("rect", { class: "cord-label-bg", x: midX - labelWidth / 2, y: midY - 9, width: labelWidth, height: 18, rx: 5 }),
    );
    const text = element("text", { class: "cord-label-text", x: midX, y: midY + 3.5, "text-anchor": "middle" });
    text.textContent = label;
    labels.push(text);
  }
  // One port on the edge of each parent, as in the sketch.
  const dots = ports.map(({ x, y, auth }) =>
    element("circle", { class: auth ? "cord-port auth" : "cord-port", cx: x, cy: y, r: 5 }),
  );
  // Labels and ports go last so no cord crosses over them.
  svg.replaceChildren(...paths, ...labels, ...dots);
}

// Redrawn whenever rows change size: new font, window, wrapped text, or a row that
// becomes visible (rows off screen are not laid out). Every position is read before
// anything is written, so layout runs once per batch.
const observer = new ResizeObserver((entries) => {
  const measured = entries.map((entry) => [entry.target, measure(entry.target)]);
  for (const [family, geometry] of measured) draw(family, geometry);
});

function watch() {
  for (const grid of document.querySelectorAll(".family-grid:not([data-cords])")) {
    grid.dataset.cords = "";
    observer.observe(grid);
  }
}

function redrawAll() {
  const families = [...document.querySelectorAll(".family-focus[open] > .family-grid")];
  const measured = families.map((family) => [family, measure(family)]);
  for (const [family, geometry] of measured) draw(family, geometry);
}

// Column toggles: `data-toggle` names the class set on the catalog when the column is hidden.
function storedHidden(name) {
  try {
    return localStorage.getItem(`elzevir.catalog.${name}`) === "hidden";
  } catch {
    return false; // storage blocked: every column starts shown
  }
}

function setHidden(catalog, button, hidden) {
  const name = button.dataset.toggle;
  catalog.classList.toggle(`hide-${name}`, hidden);
  button.setAttribute("aria-pressed", String(!hidden));
  try {
    localStorage.setItem(`elzevir.catalog.${name}`, hidden ? "hidden" : "shown");
  } catch {
    // Storage blocked: the choice lasts for this page only.
  }
}

function toggles() {
  const catalog = document.querySelector(".catalog");
  if (!catalog) return;
  for (const button of catalog.querySelectorAll("button[data-toggle]")) {
    setHidden(catalog, button, storedHidden(button.dataset.toggle));
    button.hidden = false;
    button.addEventListener("click", () => {
      setHidden(catalog, button, button.getAttribute("aria-pressed") === "true");
      for (const grid of catalog.querySelectorAll(".family-grid[data-focus]")) layout(grid);
      redrawAll();
    });
  }
}

// Focus (listagem.md, items 28 to 32): one family open at a time, kept in the URL fragment,
// with the trail naming it. The <details> works without this module; this only adds the rules.
function trail(details) {
  const nav = document.querySelector(".trail");
  if (!nav) return;
  const family = details?.open ? details.closest("li.family") : null;
  nav.hidden = !family;
  const name = nav.querySelector(".trail-family");
  if (name) name.textContent = family?.querySelector(".row-title strong")?.textContent ?? "";
  // The level in focus, named as its column header names it.
  const level = nav.querySelector(".trail-level");
  const grid = family?.querySelector(".family-grid");
  const header = grid?.dataset.focus ? grid.querySelector(`.track-${grid.dataset.focus === "instance" ? "editions" : grid.dataset.focus} > .col-header`) : null;
  if (level) {
    level.hidden = !header;
    level.textContent = header ? `› ${header.textContent}` : "";
  }
}

// Focus on one level of the open family (listagem.md, items 37 to 41).
function clearLevel(grid) {
  delete grid.dataset.focus;
  for (const node of grid.querySelectorAll(".focused, .dimmed, [inert]")) {
    node.classList.remove("focused", "dimmed");
    node.inert = false;
  }
  layout(grid);
}

// Level focus layout (listagem.md, item 38): the focused column takes what is left, the
// columns beside it show a glance, and the far ones fold into a rail.
const LEVELS = ["auth", "hub", "work", "instance", "items"];
const WIDTHS = { glance: "8.5rem", rail: "2.5rem" };

function shownLevels(grid) {
  const catalog = grid.closest(".catalog");
  return LEVELS.filter((level) => {
    if (level === "auth") return !catalog?.classList.contains("hide-auth");
    if (level === "hub") return !grid.classList.contains("no-hub") && !catalog?.classList.contains("hide-hub");
    return true;
  });
}

function levelParts(grid, level) {
  const headers = grid.querySelectorAll(".track-editions > .col-header");
  if (level === "instance") return [headers[0], ...grid.querySelectorAll(".card-instance")];
  if (level === "items") return [headers[1], ...grid.querySelectorAll(".items-stack")];
  return [...grid.querySelectorAll(`.track-${level} > .col-header, .track-${level} .card`)];
}

function layout(grid) {
  for (const node of grid.querySelectorAll(".glance, .rail")) node.classList.remove("glance", "rail");
  const focus = grid.dataset.focus;
  // On a narrow screen the columns stack, so there is no width to share.
  if (!focus || matchMedia("(max-width: 60rem)").matches) {
    grid.style.removeProperty("grid-template-columns");
    return;
  }
  const levels = shownLevels(grid);
  const at = levels.indexOf(focus);
  // Items belong to their Instance: in Instance focus they stay a glance, never a rail.
  const view = (index) => (index === at ? "focus" : Math.abs(index - at) === 1 || (focus === "instance" && levels[index] === "items") ? "glance" : "rail");
  const tracks = levels.map((level, index) => {
    const kind = view(index);
    if (kind !== "focus") for (const node of levelParts(grid, level)) node?.classList.add(kind);
    return kind === "focus" ? "minmax(0, 1fr)" : WIDTHS[kind];
  });
  grid.style.setProperty("grid-template-columns", tracks.join(" "));
}

function focusLevel(card) {
  const grid = card.closest(".family-grid");
  if (!grid) return;
  const again = card.classList.contains("focused");
  // Leaving a block with an edit not sent asks first (edicao.md, item 32).
  const pending = dirtyForm(grid);
  if (pending && (again || !card.contains(pending))) {
    askDiscard(pending);
    return;
  }
  closeItemGrids(grid);
  clearLevel(grid);
  if (!again) {
    grid.dataset.focus = card.dataset.level;
    card.classList.add("focused");
    if (card.dataset.level === "instance") {
      // The other Instances and their items step back, so nobody touches the wrong item.
      const own = card.dataset.node;
      for (const other of grid.querySelectorAll('.card[data-level="instance"]')) {
        if (other === card) continue;
        other.classList.add("dimmed");
        other.inert = true;
      }
      for (const stack of grid.querySelectorAll(".items-stack")) {
        if (stack.querySelector(`[data-parent="${own}"]`)) continue;
        stack.classList.add("dimmed");
        stack.inert = true;
      }
    }
    layout(grid);
    loadFields(card);
  }
  follow(grid);
  trail(grid.closest(".family-focus"));
}

// Every filled field of a focused Work or Instance (listagem.md, items 42 to 45), fetched
// once and kept in the block.
async function loadFields(card) {
  const url = card.dataset.fields;
  if (!url || card.dataset.fieldsState) return;
  card.dataset.fieldsState = "loading";
  try {
    const response = await fetch(url, { credentials: "same-origin", headers: { Accept: "text/html" } });
    if (!response.ok) throw new Error(String(response.status));
    const box = document.createElement("div");
    box.className = "all-fields-box";
    // Server-rendered by Askama, which escapes every value.
    box.innerHTML = await response.text();
    card.append(box);
    card.dataset.fieldsState = "loaded";
    startForm(card);
    const grid = card.closest(".family-grid");
    if (grid) follow(grid);
  } catch {
    // The short fields stay; focusing the block again tries once more.
    delete card.dataset.fieldsState;
  }
}

// Cords follow the columns while they animate.
function follow(grid) {
  const until = performance.now() + 400;
  const step = () => {
    draw(grid, measure(grid));
    if (performance.now() < until) requestAnimationFrame(step);
  };
  requestAnimationFrame(step);
}

document.addEventListener("click", (event) => {
  const target = event.target;
  if (!(target instanceof Element) || target.closest("a, button, summary, details.export-menu, input, select, textarea, .all-fields, .record-form, elz-item-grid")) return;
  const card = target.closest(".family-grid .card[data-level]");
  if (card) focusLevel(card);
});

// ← and → move to the first card of the column before or after.
function neighbour(card, step) {
  const grid = card.closest(".family-grid");
  const columns = ["auth", "hub", "work", "instance"].filter((level) =>
    grid?.querySelector(`.card[data-level="${level}"]:not([inert])`)?.getBoundingClientRect().width,
  );
  const at = columns.indexOf(card.dataset.level);
  const level = columns[at + step];
  return level ? grid.querySelector(`.card[data-level="${level}"]:not([inert])`) : null;
}

function focused(details) {
  for (const other of document.querySelectorAll(".family-focus[open]")) {
    if (other !== details) other.open = false;
  }
  const head = details.closest("li.family")?.dataset.head;
  if (head) history.replaceState(null, "", `#f-${head}`);
  trail(details);
}

function unfocused(details) {
  if (document.querySelector(".family-focus[open]")) return;
  history.replaceState(null, "", location.pathname + location.search);
  trail(details);
}

document.addEventListener(
  "toggle",
  (event) => {
    const details = event.target;
    if (!(details instanceof HTMLDetailsElement) || !details.classList.contains("family-focus")) return;
    if (details.open) focused(details);
    else unfocused(details);
  },
  true, // toggle does not bubble
);

function typing(target) {
  return target instanceof HTMLElement && (target.isContentEditable || /^(INPUT|TEXTAREA|SELECT)$/.test(target.tagName));
}

document.addEventListener("keydown", (event) => {
  // Ctrl+N as the spec of the interface names it, where the browser lets a page have it
  // (Chrome keeps it for a new window, so N alone does the same below).
  if (event.ctrlKey && !event.altKey && !event.shiftKey && event.key.toLowerCase() === "n" && document.querySelector("[data-action=new-family]")) {
    event.preventDefault();
    newFromKey();
    return;
  }
  if (event.defaultPrevented || event.altKey || event.ctrlKey || event.metaKey || typing(event.target)) return;
  if (event.key === "/") {
    event.preventDefault();
    document.getElementById("q")?.focus();
    return;
  }
  // N creates (criacao.md, item 14), where the screen offers it.
  if (event.key.toLowerCase() === "n" && document.querySelector("[data-action=new-family]")) {
    event.preventDefault();
    newFromKey();
    return;
  }
  const card = event.target instanceof Element ? event.target.closest(".card[data-level]") : null;
  if (card && event.key === "Enter") {
    event.preventDefault();
    focusLevel(card);
    return;
  }
  if (card && (event.key === "ArrowLeft" || event.key === "ArrowRight")) {
    event.preventDefault();
    neighbour(card, event.key === "ArrowLeft" ? -1 : 1)?.focus();
    return;
  }
  if (event.key === "Escape") {
    const creating = newPanel()?.querySelector(".record-form");
    if (creating) {
      if (dirty(creating)) askDiscard(creating);
      else closeNew();
      return;
    }
    const open = document.querySelector(".family-focus[open]");
    if (!open) return;
    const pending = dirtyForm(open);
    if (pending) {
      askDiscard(pending);
      return;
    }
    const grid = open.querySelector(".family-grid");
    if (grid) {
      closeItemGrids(grid);
      clearLevel(grid);
    }
    open.open = false;
    open.querySelector(":scope > summary")?.focus();
    return;
  }
  if (event.key !== "ArrowDown" && event.key !== "ArrowUp") return;
  const rows = [...document.querySelectorAll("summary.family-row")];
  if (rows.length === 0) return;
  const current = document.activeElement?.closest?.("li.family")?.querySelector("summary.family-row");
  const at = rows.indexOf(current);
  const next = event.key === "ArrowDown" ? Math.min(at + 1, rows.length - 1) : Math.max(at - 1, 0);
  event.preventDefault();
  rows[at === -1 ? 0 : next].focus();
});

// A link with a family's fragment opens it, when it is on the page.
function openFromFragment() {
  const id = decodeURIComponent(location.hash.slice(1));
  if (!id.startsWith("f-")) return;
  const details = document.getElementById(id)?.querySelector(".family-focus");
  if (!details) return;
  details.open = true;
  details.scrollIntoView({ block: "start" });
}

toggles();
watch();
openFromFragment();
window.addEventListener("hashchange", openFromFragment);
trail(document.querySelector(".family-focus[open]"));
document.addEventListener("htmx:afterSettle", () => {
  watch();
  trail(document.querySelector(".family-focus[open]"));
});

// Editing in the focused block (edicao.md, items 25 to 32). The form's field names are the
// edit's own JSON names, so the value of each field goes into the edit as it is.
function rowObject(row) {
  const object = {};
  for (const cell of row.querySelectorAll("[data-name]")) {
    if (cell.dataset.type === "bool") object[cell.dataset.name] = cell.checked;
    else if (cell.dataset.type === "number") object[cell.dataset.name] = cell.value.trim() ? Number(cell.value.trim()) : null;
    else object[cell.dataset.name] = cell.hasAttribute("data-optional") && !cell.value.trim() ? null : cell.value;
  }
  return object;
}

function fieldValue(field) {
  const part = field.querySelector("[data-shape]");
  if (!part) return null;
  switch (part.dataset.shape) {
    case "text": {
      const value = part.querySelector("input, textarea").value;
      return part.hasAttribute("data-required") || value.trim() ? value : null;
    }
    case "select":
      return part.querySelector("select").value || null;
    case "values":
      return [...part.querySelectorAll(".form-rows input")].map((input) => input.value).filter((value) => value.trim());
    case "rows":
      return [...part.querySelectorAll(".form-rows > .form-row")].map(rowObject);
    case "group":
      return rowObject(part.querySelector(".form-row"));
    case "check":
      return part.querySelector("input").checked;
    default:
      return null;
  }
}

// Each field by its block and change: a new family has a Title in the Work and another in
// the Instance (criacao.md, item 5). An edit form has one block, named "".
// The rows of a BIBFRAME property are one change each, named by the property
// (bibframe-completo.md, items 24 to 26).
function fieldKey(field) {
  return `${field.closest("[data-part]")?.dataset.part ?? ""}|${field.dataset.change}|${field.dataset.property ?? ""}`;
}

function changeOf(field, value) {
  const property = field.dataset.property;
  return property ? { [field.dataset.change]: { property, rows: value } } : { [field.dataset.change]: value };
}

function snapshot(form) {
  const values = {};
  for (const field of form.querySelectorAll(".form-field[data-change]")) {
    values[fieldKey(field)] = JSON.stringify(fieldValue(field));
  }
  return values;
}

function changes(form) {
  const before = JSON.parse(form.dataset.initial || "{}");
  const now = snapshot(form);
  const list = [];
  for (const field of form.querySelectorAll(".form-field[data-change]")) {
    const key = fieldKey(field);
    const value = now[key];
    // A select left empty has no value to send (a category cannot be cleared).
    if (value === before[key] || (value === "null" && field.querySelector("select:not(.cell)"))) continue;
    list.push(changeOf(field, JSON.parse(value)));
  }
  return list;
}

// A new record sends every filled field, the carried-over ones too (criacao.md, item 18).
function filled(scope) {
  const list = [];
  for (const field of scope.querySelectorAll(".form-field[data-change]")) {
    const value = fieldValue(field);
    if (value === null || value === "" || (Array.isArray(value) && value.length === 0)) continue;
    list.push(changeOf(field, value));
  }
  return list;
}

function createBody(form) {
  if (form.dataset.create !== "family") return { changes: filled(form) };
  const part = (name) => form.querySelector(`[data-part="${name}"]`);
  return { work: filled(part("work")), instance: filled(part("instance")) };
}

const dirty = (form) => changes(form).length > 0;

// A form or an item grid with something not sent yet.
function dirtyForm(scope) {
  return [...scope.querySelectorAll(".record-form, elz-item-grid")].find((editor) => (editor.localName === "elz-item-grid" ? editor.dirty : dirty(editor))) ?? null;
}

function askDiscard(form) {
  const prompt = form.querySelector(".form-discard");
  if (!prompt) return;
  prompt.hidden = false;
  form.closest(".card")?.scrollIntoView({ block: "nearest" });
  prompt.querySelector('[data-action="keep"], [data-grid="keep"]')?.focus();
}

function startForm(card) {
  const form = card.querySelector(".record-form");
  if (!form) return;
  form.dataset.initial = JSON.stringify(snapshot(form));
}

function clearErrors(form) {
  const alert = form.querySelector(".form-alert");
  if (alert) alert.hidden = true;
  for (const error of form.querySelectorAll(".field-error")) error.hidden = true;
  for (const field of form.querySelectorAll(".form-field.invalid")) field.classList.remove("invalid");
}

function showAlert(form, text) {
  const alert = form.querySelector(".form-alert");
  if (!alert) return;
  alert.textContent = text;
  alert.hidden = false;
}

async function reloadFields(card) {
  card.querySelector(".all-fields-box")?.remove();
  delete card.dataset.fieldsState;
  await loadFields(card);
}

function showFieldErrors(form, answer) {
  for (const error of answer.errors) {
    const field = form.querySelector(`.form-field[data-error="${error.field}"]`);
    const line = field?.querySelector(".field-error");
    if (!field || !line) continue;
    field.classList.add("invalid");
    line.textContent = error.message;
    line.hidden = false;
  }
  form.querySelector(".form-field.invalid input, .form-field.invalid textarea, .form-field.invalid select")?.focus();
}

async function submitForm(form) {
  if (form.dataset.create) return submitNew(form);
  const card = form.closest(".card");
  const list = changes(form);
  clearErrors(form);
  if (!card || list.length === 0) return;
  const token = document.querySelector(".catalog")?.dataset.csrf ?? "";
  let response;
  try {
    response = await fetch(form.dataset.endpoint, {
      method: "POST",
      credentials: "same-origin",
      headers: { "Content-Type": "application/json", "X-Elzevir-Csrf": token },
      body: JSON.stringify({ version: Number(form.dataset.version), changes: list }),
    });
  } catch {
    showAlert(form, form.dataset.failed);
    return;
  }
  if (response.ok) {
    const title = list.find((change) => "Title" in change)?.Title;
    if (title !== undefined) {
      const shown = card.querySelector(".title-main");
      if (shown) shown.textContent = title.trim();
    }
    // An authority shows its new heading at once (autoridades.md).
    const heading = list.find((change) => "Heading" in change)?.Heading;
    if (heading !== undefined) {
      const shown = card.querySelector(".field-value");
      if (shown) shown.textContent = heading.trim();
    }
    await reloadFields(card);
    const saved = card.querySelector(".form-saved");
    if (saved) saved.hidden = false;
    return;
  }
  if (response.status === 409) return showAlert(form, form.dataset.stale);
  if (response.status === 422) {
    const answer = await response.json().catch(() => null);
    if (answer?.errors) return showFieldErrors(form, answer);
  }
  showAlert(form, form.dataset.failed);
}

// Creating (criacao.md, items 13 to 20). The blank form comes from the server, with what
// carries over from the record created before in this tab.
const PREVIOUS = "elzevir.catalog.previous";

function remember(key, value) {
  try {
    sessionStorage.setItem(key, value);
  } catch {
    // Storage blocked: the next new family starts blank.
  }
}

function recalled(key) {
  try {
    return sessionStorage.getItem(key);
  } catch {
    return null;
  }
}

function newPanel() {
  return document.querySelector(".new-record-panel");
}

function closeNew() {
  newPanel()?.remove();
}

async function openNew(url, place) {
  const pending = dirtyForm(document);
  if (pending) return askDiscard(pending);
  closeNew();
  let html;
  try {
    const response = await fetch(url, { credentials: "same-origin", headers: { Accept: "text/html" } });
    if (!response.ok) return;
    html = await response.text();
  } catch {
    return; // nothing opened; the key or button tries again
  }
  const panel = document.createElement("section");
  panel.className = "new-record-panel";
  // Server-rendered by Askama, which escapes every value.
  panel.innerHTML = html;
  place(panel);
  const form = panel.querySelector(".record-form");
  if (!form) return;
  form.dataset.initial = JSON.stringify(snapshot(form));
  panel.scrollIntoView({ block: "nearest" });
  // The cursor goes to the first empty field (item 13).
  const empty = [...form.querySelectorAll(".form-field input[type=text], .form-field textarea")].find((input) => !input.value);
  (empty ?? form.querySelector("input, textarea, select"))?.focus();
}

function newFamily(isbn) {
  const params = new URLSearchParams();
  const previous = recalled(PREVIOUS);
  if (previous) params.set("from", previous);
  // A scanned ISBN the catalog does not have (bipe.md, item 10).
  if (isbn) params.set("isbn", isbn);
  const query = params.toString();
  openNew(`/equipe/acervo/nova${query ? `?${query}` : ""}`, (panel) => {
    const families = document.getElementById("families");
    if (families) families.before(panel);
    else document.querySelector(".catalog")?.append(panel);
  });
}

function newInstance(from, copy) {
  openNew(`/equipe/acervo/instancia/${encodeURIComponent(from)}/nova${copy ? "?copiar=1" : ""}`, (panel) => {
    const grid = document.querySelector(".family-focus[open] > .family-grid");
    if (grid) grid.after(panel);
    else document.getElementById("families")?.before(panel);
  });
}

// "+ Authority" links an authority to the Work in focus, or to the family's first Work
// (autoridades.md); the form names that Work.
function linkAuthority(button) {
  const family = button.closest(".family-focus");
  const focusedCard = family?.querySelector(".card.focused");
  const work = focusedCard?.dataset.level === "work" ? focusedCard : family?.querySelector(".card-work");
  const id = work?.dataset.node?.slice(1);
  if (!id) return;
  // A place of publication goes to one Instance of the Work: the first, or the one chosen.
  const instances = [...family.querySelectorAll(`.card-instance[data-parent~="w${id}"]`)];
  const first = instances[0]?.dataset.node.slice(1);
  const query = first ? `?instancia=${encodeURIComponent(first)}` : "";
  openNew(`/equipe/acervo/obra/${encodeURIComponent(id)}/autoridade/nova${query}`, (panel) => {
    const heading = panel.querySelector(".new-record-heading");
    const title = work.querySelector(".title-main")?.textContent;
    if (heading && title) heading.append(`: ${title}`);
    const form = panel.querySelector(".record-form");
    if (form && instances.length > 1) form.querySelector(".form-fields")?.append(instanceChoice(form, instances, button.dataset.instanceLabel));
    family.querySelector(".family-grid")?.after(panel);
  });
}

// The Instance a place of publication goes to, named by the first values on its card.
// It changes the address the form is sent to, not the form's values.
function instanceChoice(form, instances, caption) {
  const label = document.createElement("label");
  label.className = "form-label link-instance";
  const text = document.createElement("span");
  text.textContent = caption ?? "";
  const select = document.createElement("select");
  for (const card of instances) {
    const option = document.createElement("option");
    option.value = card.dataset.node.slice(1);
    option.textContent = [...card.querySelectorAll(".field-value")].slice(0, 3).map((value) => value.textContent.trim()).join(" · ");
    select.append(option);
  }
  select.addEventListener("change", () => {
    form.dataset.endpoint = form.dataset.endpoint.replace(/instancia=\d+/, `instancia=${encodeURIComponent(select.value)}`);
  });
  label.append(text, select);
  return label;
}

// With a family open, N adds an Instance to the Work in focus (or its first Work);
// otherwise it starts a new family (item 14).
function newFromKey() {
  const open = document.querySelector(".family-focus[open]");
  if (!open) return newFamily();
  const focusedCard = open.querySelector(".card.focused");
  if (focusedCard?.dataset.level === "instance") return newInstance(focusedCard.dataset.node.slice(1));
  const work = focusedCard?.dataset.level === "work" ? focusedCard : open.querySelector(".card-work");
  const from = work?.querySelector("[data-action=new-instance]")?.dataset.from;
  return from ? newInstance(from) : newFamily();
}

async function submitNew(form) {
  clearErrors(form);
  const token = document.querySelector(".catalog")?.dataset.csrf ?? "";
  let response;
  try {
    response = await fetch(form.dataset.endpoint, {
      method: "POST",
      credentials: "same-origin",
      headers: { "Content-Type": "application/json", "X-Elzevir-Csrf": token },
      body: JSON.stringify(createBody(form)),
    });
  } catch {
    showAlert(form, form.dataset.failed);
    return;
  }
  // A link answers with the Work's new version; a creation with 201.
  if (response.status === 201 || (form.dataset.create === "authority" && response.ok)) {
    const answer = await response.json().catch(() => ({}));
    // Nothing is pending any more: the panel goes before the page moves.
    form.dataset.initial = JSON.stringify(snapshot(form));
    if (form.dataset.create === "family") {
      remember(PREVIOUS, String(answer.instance));
      if (answer.family) location.assign(answer.family);
      else location.reload();
    } else {
      location.reload();
    }
    return;
  }
  if (response.status === 422) {
    const answer = await response.json().catch(() => null);
    if (answer?.errors) return showFieldErrors(form, answer);
  }
  showAlert(form, form.dataset.failed);
}

// A carried-over value is confirmed once the cataloguer reaches its field (item 18).
document.addEventListener("focusin", (event) => {
  const field = event.target instanceof Element ? event.target.closest(".form-field[data-inherited]") : null;
  if (!field) return;
  field.removeAttribute("data-inherited");
  field.querySelector(":scope > .inherited-mark")?.remove();
});

document.addEventListener("submit", (event) => {
  const form = event.target;
  if (!(form instanceof HTMLFormElement) || !form.classList.contains("record-form")) return;
  event.preventDefault();
  submitForm(form);
});

document.addEventListener("click", (event) => {
  const target = event.target;
  if (!(target instanceof Element)) return;
  const action = target.closest("[data-action]");
  const form = target.closest(".record-form");
  if (action && !form) {
    if (action.dataset.action === "edit-items" && action.dataset.from) openItemGrid(action);
    if (action.dataset.action === "new-family") newFamily();
    else if (action.dataset.action === "new-instance" && action.dataset.from) newInstance(action.dataset.from);
    else if (action.dataset.action === "link-authority") linkAuthority(action);
    else if (action.dataset.action === "copy-instance" && action.dataset.from) newInstance(action.dataset.from, true);
    if (action.dataset.action.startsWith("new-") || action.dataset.action === "edit-items" || action.dataset.action === "copy-instance") return;
  }
  if (action && form) {
    const fieldset = action.closest("fieldset");
    switch (action.dataset.action) {
      case "add-row": {
        const template = fieldset?.querySelector(":scope > template");
        const rows = fieldset?.querySelector(":scope > .form-rows");
        if (template && rows) {
          rows.append(template.content.cloneNode(true));
          rows.lastElementChild?.querySelector("input, select")?.focus();
        }
        break;
      }
      case "remove-row":
        action.closest(".form-row")?.remove();
        break;
      case "discard": {
        if (form.dataset.create) {
          closeNew();
          break;
        }
        const card = form.closest(".card");
        if (card) reloadFields(card);
        break;
      }
      case "cancel-new":
        if (dirty(form)) askDiscard(form);
        else closeNew();
        break;
      case "keep":
        form.querySelector(".form-discard").hidden = true;
        form.querySelector("input, textarea, select")?.focus();
        break;
      default:
        break;
    }
    return;
  }
  // A pending edit is never dropped in silence: closing or switching the family asks first.
  const summary = target.closest("summary.family-row");
  if (!summary) return;
  const pending = dirtyForm(document);
  if (pending) {
    event.preventDefault();
    askDiscard(pending);
  }
}, true);

document.addEventListener("keydown", (event) => {
  const form = event.target instanceof Element ? event.target.closest(".record-form") : null;
  if (!form) return;
  if (event.key === "Enter" && (event.ctrlKey || event.metaKey)) {
    event.preventDefault();
    submitForm(form);
  } else if (event.key === "Escape") {
    // An open suggestion list closes first (handled below).
    if (event.target.closest(".form-row")?.querySelector(":scope > .suggestions")) return;
    event.preventDefault();
    if (dirty(form)) askDiscard(form);
    else if (form.dataset.create) closeNew();
    else form.closest(".card")?.focus();
  }
}, true);

// Suggestions of existing agents and subjects (edicao.md, items 33 to 35).
const SUGGEST_URL = {
  Contributions: "/equipe/acervo/sugestoes/agentes",
  Subjects: "/equipe/acervo/sugestoes/assuntos",
};
let suggestTimer = 0;
let suggestRound = 0;

// The "+ Authority" form looks among agents for a contributor, among subjects otherwise.
function suggestUrl(input) {
  const field = input.closest(".form-field");
  const change = field?.dataset.change;
  if (change !== "Link") return SUGGEST_URL[change];
  const role = field.querySelector('[data-name="role"]')?.value;
  return role === "contribution" ? SUGGEST_URL.Contributions : SUGGEST_URL.Subjects;
}

function suggestInput(target) {
  if (!(target instanceof HTMLInputElement) || !target.matches('.cell[data-name="heading"]')) return null;
  return suggestUrl(target) ? target : null;
}

function closeSuggestions(row) {
  row?.querySelector(":scope > .suggestions")?.remove();
}

function markNew(input, isNew) {
  const row = input.closest(".form-row");
  let mark = row?.querySelector(":scope > .new-mark");
  if (!isNew) return mark?.remove();
  if (!mark && row) {
    mark = document.createElement("span");
    mark.className = "new-mark";
    mark.textContent = input.closest(".record-form")?.dataset.newEntity ?? "";
    input.after(mark);
  }
}

function pick(input, option) {
  const row = input.closest(".form-row");
  input.value = option.dataset.heading;
  const kind = row?.querySelector('[data-name="kind"]');
  if (kind) kind.value = option.dataset.kind;
  const thesaurus = row?.querySelector('[data-name="thesaurus"]');
  if (thesaurus) thesaurus.value = option.dataset.thesaurus ?? "";
  markNew(input, false);
  closeSuggestions(row);
  input.focus();
}

function showSuggestions(input, found) {
  const row = input.closest(".form-row");
  closeSuggestions(row);
  const typed = input.value.trim();
  markNew(input, typed.length > 0 && !found.some((entry) => entry.heading === typed));
  if (!row || found.length === 0) return;
  const kinds = row.querySelector('[data-name="kind"]');
  const list = document.createElement("ul");
  list.className = "suggestions";
  list.setAttribute("role", "listbox");
  for (const entry of found) {
    const option = document.createElement("li");
    option.setAttribute("role", "option");
    option.dataset.heading = entry.heading;
    option.dataset.kind = entry.kind;
    if (entry.thesaurus) option.dataset.thesaurus = entry.thesaurus;
    option.textContent = entry.heading;
    const kind = [...(kinds?.options ?? [])].find((choice) => choice.value === entry.kind)?.textContent;
    if (kind || entry.thesaurus) {
      const detail = document.createElement("span");
      detail.className = "suggestion-detail";
      detail.textContent = [kind, entry.thesaurus].filter(Boolean).join(" · ");
      option.append(" ", detail);
    }
    option.addEventListener("mousedown", (event) => {
      event.preventDefault(); // keep the focus in the input
      pick(input, option);
    });
    list.append(option);
  }
  row.append(list);
}

async function lookup(input) {
  const round = ++suggestRound;
  const typed = input.value.trim();
  if (typed.length < 2) {
    closeSuggestions(input.closest(".form-row"));
    markNew(input, false);
    return;
  }
  try {
    const response = await fetch(`${suggestUrl(input)}?q=${encodeURIComponent(typed)}`, { credentials: "same-origin" });
    if (!response.ok || round !== suggestRound) return;
    showSuggestions(input, await response.json());
  } catch {
    // No suggestions this time; typing still works and the next key tries again.
  }
}

document.addEventListener("input", (event) => {
  const input = suggestInput(event.target);
  if (!input) return;
  clearTimeout(suggestTimer);
  suggestTimer = setTimeout(() => lookup(input), 150);
});

document.addEventListener("focusout", (event) => {
  const input = suggestInput(event.target);
  if (input) setTimeout(() => closeSuggestions(input.closest(".form-row")), 150);
});

document.addEventListener("keydown", (event) => {
  const target = event.target;
  if (!(target instanceof HTMLElement) || !target.closest(".record-form")) return;
  const row = target.closest(".form-row");
  // Alt+↑ and Alt+↓ move the row (item 36).
  if (event.altKey && (event.key === "ArrowUp" || event.key === "ArrowDown") && row?.parentElement?.classList.contains("form-rows")) {
    event.preventDefault();
    const other = event.key === "ArrowUp" ? row.previousElementSibling : row.nextElementSibling;
    if (other) {
      if (event.key === "ArrowUp") other.before(row);
      else other.after(row);
      target.focus();
    }
    return;
  }
  const list = row?.querySelector(":scope > .suggestions");
  if (list && suggestInput(target)) {
    const options = [...list.children];
    const at = options.findIndex((option) => option.classList.contains("active"));
    if (event.key === "ArrowDown" || event.key === "ArrowUp") {
      event.preventDefault();
      options[at]?.classList.remove("active");
      const next = event.key === "ArrowDown" ? Math.min(at + 1, options.length - 1) : Math.max(at - 1, 0);
      options[next]?.classList.add("active");
      return;
    }
    if (event.key === "Enter" && !event.ctrlKey && !event.metaKey && at >= 0) {
      event.preventDefault();
      event.stopPropagation();
      pick(target, options[at]);
      return;
    }
    if (event.key === "Escape") {
      event.preventDefault();
      event.stopPropagation();
      closeSuggestions(row);
      return;
    }
  }
  // A plain Enter in a one-line field does not send the form: Ctrl+Enter does (item 29).
  if (event.key === "Enter" && !event.ctrlKey && !event.metaKey && target instanceof HTMLInputElement) {
    event.preventDefault();
  }
}, true);

// The History of the focused block (historico.md): read from the server the first time
// it is shown; the times the server wrote in UTC are shown in the viewer's own time.
document.addEventListener("click", async (event) => {
  const toggle = event.target instanceof Element ? event.target.closest(".history-toggle") : null;
  const box = toggle?.closest(".history-box");
  const target = box?.querySelector(".history-body");
  if (!toggle || !box || !target) return;
  event.preventDefault();
  const open = toggle.getAttribute("aria-expanded") !== "true";
  toggle.setAttribute("aria-expanded", String(open));
  target.hidden = !open;
  if (!open || box.dataset.loaded) return;
  try {
    const response = await fetch(box.dataset.history, { credentials: "same-origin", headers: { Accept: "text/html" } });
    if (!response.ok) return;
    // Server-rendered by Askama, which escapes every value.
    target.innerHTML = await response.text();
    box.dataset.loaded = "1";
  } catch {
    return; // showing it again tries again
  }
  for (const time of target.querySelectorAll("time[datetime]")) {
    const at = new Date(time.getAttribute("datetime"));
    if (!Number.isNaN(at.getTime())) time.textContent = at.toLocaleString(document.documentElement.lang || undefined, { dateStyle: "short", timeStyle: "short" });
  }
});

// The ISBN field is checked when the cataloguer leaves it (edicao.md, item 38). The server
// says what the field holds and what to show; nothing is decided here.
let isbnRound = 0;

function isbnInput(target) {
  if (!(target instanceof HTMLInputElement) || !target.matches('.cell[data-name="value"]')) return null;
  if (target.closest(".form-field")?.dataset.change !== "Identifiers") return null;
  const kind = target.closest(".form-row")?.querySelector('[data-name="kind"]');
  return kind?.value === "isbn" ? target : null;
}

function showIsbn(input, answer) {
  const row = input.closest(".form-row");
  row?.querySelector(":scope > .isbn-note")?.remove();
  input.classList.toggle("invalid", Boolean(answer.error));
  if (answer.error) input.setAttribute("aria-invalid", "true");
  else input.removeAttribute("aria-invalid");
  if (answer.value !== undefined && input.value !== answer.value) input.value = answer.value;
  const text = answer.error ?? answer.note;
  if (!text || !row) return;
  const note = document.createElement("span");
  note.className = answer.error ? "isbn-note error" : "isbn-note";
  note.setAttribute("role", answer.error ? "alert" : "status");
  note.textContent = text;
  row.append(note);
}

async function checkIsbn(input) {
  const round = ++isbnRound;
  try {
    const response = await fetch(`/equipe/acervo/isbn?q=${encodeURIComponent(input.value)}`, { credentials: "same-origin" });
    if (!response.ok || round !== isbnRound) return;
    showIsbn(input, await response.json());
  } catch {
    // No check this time; the edit command checks the ISBN again when the form is sent.
  }
}

document.addEventListener("focusout", (event) => {
  const input = isbnInput(event.target);
  if (input) checkIsbn(input);
});

// The grid of an Instance's Items (exemplares.md, items 17 to 28): a spreadsheet in the
// Items column, saved as one batch. The server writes the rows; this element edits them
// and sends the new and changed ones.
function nextInventory(start, step) {
  // The last number of the text grows, keeping its zeros: 2026/0009 + 1 = 2026/0010.
  const match = /(\d+)(?!.*\d)/.exec(start);
  if (!match) return start;
  const grown = String(Number(match[1]) + step).padStart(match[1].length, "0");
  return start.slice(0, match.index) + grown + start.slice(match.index + match[1].length);
}

class ItemGridElement extends HTMLElement {
  connectedCallback() {
    if (this.ready) return;
    this.ready = true;
    const template = this.querySelector("template.grid-columns");
    this.columns = [...(template?.content.children ?? [])].map((marker) => ({
      name: marker.dataset.col,
      list: marker.dataset.list,
      inherited: marker.hasAttribute("data-inherited"),
    }));
    this.body = this.querySelector("tbody");
    // Each list: code → name and name or code → code (item 29).
    this.lists = {};
    for (const datalist of this.querySelectorAll("datalist")) {
      const names = new Map();
      const codes = new Map();
      for (const option of datalist.options) {
        const code = option.dataset.code ?? option.value;
        names.set(code, option.value);
        codes.set(code.toLowerCase(), code);
        if (!codes.has(option.value.toLowerCase())) codes.set(option.value.toLowerCase(), code);
      }
      this.lists[datalist.id.replace(/^grid-list-/, "")] = { names, codes };
    }
    for (const cell of this.querySelectorAll("tbody td[role=gridcell]")) cell.dataset.orig = this.value(cell);
    this.saved = false;
    this.addEventListener("keydown", (event) => this.key(event));
    this.addEventListener("click", (event) => this.click(event));
    this.addEventListener("dblclick", (event) => {
      const cell = event.target instanceof Element ? event.target.closest("td[role=gridcell]") : null;
      if (cell && !this.input) this.edit(cell);
    });
    const first = this.querySelector("tbody td[role=gridcell]");
    if (first) first.tabIndex = 0;
    else this.addRows(1);
  }

  cells(row) {
    return [...row.querySelectorAll("td[role=gridcell]")];
  }

  // What the batch carries: the code of a coded cell, which shows the name (item 29).
  value(cell) {
    return cell.dataset.value ?? cell.textContent;
  }

  setValue(cell, value) {
    cell.classList.remove("cell-suggested");
    const list = this.lists[this.columns[this.cells(cell.parentElement).indexOf(cell)]?.list ?? ""];
    if (list) {
      cell.dataset.value = value;
      cell.textContent = list.names.get(value) ?? value;
    } else {
      cell.textContent = value;
    }
  }

  // A typed name or code becomes the code; anything else stays as typed.
  resolve(cell, typed) {
    const list = this.lists[this.columns[this.cells(cell.parentElement).indexOf(cell)]?.list ?? ""];
    return list?.codes.get(typed.toLowerCase()) ?? typed;
  }

  get active() {
    return this.querySelector("tbody td[role=gridcell][tabindex='0']") ?? this.querySelector("tbody td[role=gridcell]");
  }

  get dirty() {
    return Boolean(this.querySelector("tr.row-new, tr.row-changed")) || (this.input !== undefined && this.input.value !== this.input.dataset.before);
  }

  place(cell) {
    for (const other of this.querySelectorAll("tbody td[tabindex='0']")) other.tabIndex = -1;
    cell.tabIndex = 0;
    cell.focus();
  }

  at(cell) {
    const row = cell.parentElement;
    return { row, rows: [...this.body.rows], column: this.cells(row).indexOf(cell) };
  }

  move(cell, down, right) {
    const { row, rows, column } = this.at(cell);
    const target = rows[Math.min(Math.max(rows.indexOf(row) + down, 0), rows.length - 1)];
    const cells = this.cells(target);
    this.place(cells[Math.min(Math.max(column + right, 0), cells.length - 1)]);
  }

  edit(cell, typed) {
    const column = this.columns[this.at(cell).column];
    if (column?.list) {
      this.choose(cell, column);
      return;
    }
    const input = document.createElement("input");
    input.type = "text";
    input.maxLength = 256;
    input.dataset.before = cell.textContent;
    input.value = typed ?? cell.textContent;
    input.setAttribute("aria-label", this.querySelector(`th[data-col="${column?.name}"]`)?.textContent ?? "");
    if (column?.list) input.setAttribute("list", `grid-list-${column.list}`);
    cell.textContent = "";
    cell.classList.add("editing");
    cell.append(input);
    this.input = input;
    input.focus();
    if (typed === undefined) input.select();
  }

  // A column that names a registry row is a select of its names (item 23d); shelving
  // places and collections offer only those of the row's home branch.
  choose(cell, column) {
    const select = document.createElement("select");
    select.dataset.before = cell.textContent;
    select.setAttribute("aria-label", this.querySelector(`th[data-col="${column.name}"]`)?.textContent ?? "");
    const home = this.columns.findIndex((other) => other.name === "home_branch");
    const branch = home >= 0 ? this.value(this.cells(cell.parentElement)[home]) : "";
    const perBranch = column.list === "locations" || column.list === "collections";
    select.append(new Option("", ""));
    for (const option of this.querySelector(`#grid-list-${column.list}`)?.options ?? []) {
      if (perBranch && option.dataset.branch !== branch) continue;
      select.append(new Option(option.value, option.dataset.code ?? option.value));
    }
    select.value = this.value(cell);
    cell.textContent = "";
    cell.classList.add("editing");
    cell.append(select);
    this.input = select;
    select.focus();
  }

  commit() {
    const input = this.input;
    if (!input) return null;
    const cell = input.parentElement;
    this.input = undefined;
    input.remove();
    cell.classList.remove("editing");
    this.setValue(cell, this.resolve(cell, input.value.trim()));
    if (cell.textContent) delete cell.parentElement.dataset.auto;
    this.mark(cell.parentElement);
    return cell;
  }

  cancel() {
    const input = this.input;
    if (!input) return null;
    const cell = input.parentElement;
    this.input = undefined;
    input.remove();
    cell.textContent = input.dataset.before ?? "";
    cell.classList.remove("editing");
    return cell;
  }

  mark(row) {
    if (!row.dataset.id) {
      row.classList.add("row-new");
      return;
    }
    row.classList.toggle("row-changed", this.cells(row).some((cell) => this.value(cell) !== cell.dataset.orig));
  }

  // A new row copies the inherited columns of `from` (item 19).
  newRow(from) {
    const row = document.createElement("tr");
    row.className = "row-new";
    const signal = document.createElement("td");
    signal.className = "grid-signal";
    row.append(signal);
    const above = from ? this.cells(from) : [];
    this.columns.forEach((column, index) => {
      const cell = document.createElement("td");
      cell.setAttribute("role", "gridcell");
      cell.tabIndex = -1;
      row.append(cell);
      this.setValue(cell, column.inherited && above[index] ? this.value(above[index]) : "");
      // The first call number starts as the suggested one, marked until it is confirmed
      // by saving or typed over (numero-de-chamada.md).
      if (column.name === "call_number" && !this.value(cell) && this.dataset.suggested) {
        this.setValue(cell, this.dataset.suggested);
        cell.classList.add("cell-suggested");
      }
    });
    this.body.append(row);
    return row;
  }

  addRows(count, start, step) {
    let last = this.body.rows[this.body.rows.length - 1] ?? null;
    let first = null;
    const inventory = this.columns.findIndex((column) => column.name === "inventory_number");
    for (let index = 0; index < count; index += 1) {
      last = this.newRow(last);
      if (start && inventory >= 0) this.cells(last)[inventory].textContent = nextInventory(start, index * step);
      first ??= last;
    }
    if (first) this.place(this.cells(first)[0]);
  }

  key(event) {
    const target = event.target;
    if (!(target instanceof Element)) return;
    // The tools (rows to add, first inventory number) are ordinary fields.
    if (target.closest(".grid-tools") && !(event.key === "Enter" && (event.ctrlKey || event.metaKey))) {
      if (event.key === "Escape") this.close();
      event.stopPropagation();
      return;
    }
    const cell = target.closest("td[role=gridcell]") ?? this.active;
    if (!cell) return;
    event.stopPropagation(); // the listing's own keys stay out of the grid
    const ctrl = event.ctrlKey || event.metaKey;
    if (ctrl && event.key === "Enter") {
      event.preventDefault();
      this.commit();
      this.save();
      return;
    }
    if (ctrl && event.key.toLowerCase() === "d") {
      // Copy the cell above (item 21).
      event.preventDefault();
      const { row, rows, column } = this.at(this.input?.parentElement ?? cell);
      const above = rows[rows.indexOf(row) - 1];
      const source = above ? this.cells(above)[column] : null;
      if (this.input) this.input.value = (this.input instanceof HTMLSelectElement && source ? this.value(source) : source?.textContent) ?? "";
      else {
        this.setValue(cell, source ? this.value(source) : "");
        this.mark(row);
      }
      return;
    }
    if (this.input) {
      this.editing(event);
      return;
    }
    switch (event.key) {
      case "ArrowUp":
      case "ArrowDown":
      case "ArrowLeft":
      case "ArrowRight": {
        event.preventDefault();
        const [down, right] = { ArrowUp: [-1, 0], ArrowDown: [1, 0], ArrowLeft: [0, -1], ArrowRight: [0, 1] }[event.key];
        this.move(cell, down, right);
        return;
      }
      case "Tab":
        event.preventDefault();
        this.move(cell, 0, event.shiftKey ? -1 : 1);
        return;
      case "Enter":
      case "F2":
        event.preventDefault();
        this.edit(cell);
        return;
      case "Escape":
        event.preventDefault();
        this.close();
        return;
      case "Delete":
      case "Backspace":
        event.preventDefault();
        this.setValue(cell, "");
        this.mark(cell.parentElement);
        return;
      default:
        if (event.key.length === 1 && !ctrl && !event.altKey) {
          // Typing (or a scanner) starts editing with that character.
          event.preventDefault();
          this.edit(cell, event.key);
        }
    }
  }

  editing(event) {
    switch (event.key) {
      case "Enter": {
        // Enter confirms and goes down; a barcode on the last row adds a row (item 19).
        event.preventDefault();
        const cell = this.commit();
        const { row, rows, column } = this.at(cell);
        const last = rows.indexOf(row) === rows.length - 1;
        if (last && this.columns[column]?.name === "barcode" && cell.textContent) {
          const next = this.newRow(row);
          // Waiting for the next scan: left untouched, it is not an Item.
          next.dataset.auto = "";
          const target = this.cells(next)[column];
          this.place(target);
          this.edit(target, "");
        } else {
          this.move(cell, 1, 0);
        }
        return;
      }
      case "Escape": {
        event.preventDefault();
        const cell = this.cancel();
        const row = cell.parentElement;
        // Esc after the last scan drops the row that waited for another one.
        if (row.hasAttribute("data-auto") && row.previousElementSibling) {
          const { column } = this.at(cell);
          const previous = row.previousElementSibling;
          row.remove();
          this.place(this.cells(previous)[column]);
          return;
        }
        this.place(cell);
        return;
      }
      case "Tab": {
        event.preventDefault();
        this.move(this.commit(), 0, event.shiftKey ? -1 : 1);
        return;
      }
      case "ArrowUp":
      case "ArrowDown": {
        // In a select the arrows choose the option.
        if (this.input instanceof HTMLSelectElement) return;
        event.preventDefault();
        this.move(this.commit(), event.key === "ArrowUp" ? -1 : 1, 0);
        return;
      }
      default:
    }
  }

  click(event) {
    const target = event.target;
    if (!(target instanceof Element)) return;
    const action = target.closest("[data-grid]")?.dataset.grid;
    if (action === "save") {
      this.commit();
      this.save();
    } else if (action === "add") {
      this.commit();
      const count = Math.min(Math.max(Number(this.querySelector(".grid-add-count")?.value) || 1, 1), 500);
      const start = this.querySelector(".grid-add-inventory")?.value.trim() ?? "";
      const step = Math.max(Number(this.querySelector(".grid-add-step")?.value) || 1, 1);
      this.addRows(count, start, step);
    } else if (action === "describe") {
      this.describe();
    } else if (action === "close") {
      this.close();
    } else if (action === "discard") {
      this.finish();
    } else if (action === "keep") {
      this.querySelector(".form-discard").hidden = true;
      this.active?.focus();
    } else {
      const cell = target.closest("td[role=gridcell]");
      if (cell && cell !== this.input?.parentElement) {
        this.commit();
        this.place(cell);
      }
    }
  }

  close() {
    this.commit();
    if (this.dirty) {
      askDiscard(this);
      return;
    }
    this.finish();
  }

  finish() {
    this.dispatchEvent(new CustomEvent("grid-close", { bubbles: true }));
  }

  // The other properties of the current row's Item, as a form under the grid, outside it so
  // the grid's keys do not reach its fields (bibframe-completo.md, step 6c).
  describe() {
    const id = this.active?.parentElement?.dataset.id;
    if (!id) return;
    this.parentElement?.querySelector(".item-describe")?.remove();
    const box = document.createElement("div");
    box.className = "card item-describe";
    box.dataset.fields = `/equipe/acervo/exemplar/${id}/campos`;
    this.after(box);
    loadFields(box).then(() => box.querySelector("input, select, textarea")?.focus());
  }

  alert(text) {
    const alert = this.querySelector(".grid-alert");
    if (!alert) return;
    alert.textContent = text;
    alert.hidden = false;
  }

  async save() {
    // A row still waiting for a scan is not sent.
    for (const row of this.body.querySelectorAll("tr[data-auto]")) {
      if (row.previousElementSibling) row.remove();
    }
    const sent = [...this.body.rows].filter((row) => row.classList.contains("row-new") || row.classList.contains("row-changed"));
    for (const cell of this.querySelectorAll("td.invalid")) {
      cell.classList.remove("invalid");
      cell.removeAttribute("title");
      cell.removeAttribute("aria-invalid");
    }
    for (const row of this.querySelectorAll("tr.invalid")) row.classList.remove("invalid");
    this.querySelector(".grid-alert").hidden = true;
    if (sent.length === 0) return;
    const rows = sent.map((row) => {
      const object = { id: row.dataset.id ? Number(row.dataset.id) : null };
      this.cells(row).forEach((cell, index) => {
        object[this.columns[index].name] = this.value(cell);
      });
      return object;
    });
    const token = document.querySelector(".catalog")?.dataset.csrf ?? "";
    let response;
    try {
      response = await fetch(this.dataset.endpoint, {
        method: "POST",
        credentials: "same-origin",
        headers: { "Content-Type": "application/json", "X-Elzevir-Csrf": token },
        body: JSON.stringify({ version: Number(this.dataset.version), rows }),
      });
    } catch {
      this.alert(this.dataset.failed);
      return;
    }
    if (response.ok) {
      const answer = await response.json();
      this.dataset.version = String(answer.version);
      sent.forEach((row, index) => {
        row.dataset.id = String(answer.ids[index]);
        row.classList.remove("row-new", "row-changed");
        for (const cell of this.cells(row)) cell.dataset.orig = this.value(cell);
      });
      this.saved = true;
      this.alert(this.dataset.saved);
      return;
    }
    if (response.status === 409) {
      this.alert(this.dataset.stale);
      return;
    }
    const answer = response.status === 422 ? await response.json().catch(() => null) : null;
    if (!answer?.errors) {
      this.alert(this.dataset.failed);
      return;
    }
    // Each refusal names its row in the batch and its column (item 23).
    for (const error of answer.errors) {
      const row = sent[error.row];
      if (!row) continue;
      const index = this.columns.findIndex((column) => column.name === error.field);
      const cell = index >= 0 ? this.cells(row)[index] : null;
      if (!cell) {
        row.classList.add("invalid");
        row.title = error.message;
        continue;
      }
      cell.classList.add("invalid");
      cell.title = error.message;
      cell.setAttribute("aria-invalid", "true");
    }
    this.alert(this.dataset.refused);
    const first = this.querySelector("td.invalid");
    if (first) this.place(first);
  }
}

customElements.define("elz-item-grid", ItemGridElement);

function closeItemGrids(scope) {
  for (const slot of scope.querySelectorAll(".grid-slot")) {
    slot.closest(".items-stack")?.classList.remove("editing");
    slot.remove();
  }
}

// "Edit items" on a focused Instance opens its grid in the Items column, which takes the
// width while the Instance stays a glance at its left (item 17).
async function openItemGrid(button) {
  const instance = button.closest(".card-instance");
  const grid = button.closest(".family-grid");
  const stack = instance?.nextElementSibling;
  if (!instance || !grid || !stack?.classList.contains("items-stack")) return;
  const pending = dirtyForm(grid);
  if (pending) return askDiscard(pending);
  let html;
  try {
    const response = await fetch(`/equipe/acervo/instancia/${encodeURIComponent(button.dataset.from)}/exemplares`, { credentials: "same-origin", headers: { Accept: "text/html" } });
    if (!response.ok) return;
    html = await response.text();
  } catch {
    return; // nothing opened; the button tries again
  }
  closeItemGrids(grid);
  clearLevel(grid);
  grid.dataset.focus = "items";
  for (const other of grid.querySelectorAll('.card[data-level="instance"]')) {
    if (other === instance) continue;
    other.classList.add("dimmed");
    other.inert = true;
  }
  for (const other of grid.querySelectorAll(".items-stack")) {
    if (other === stack) continue;
    other.classList.add("dimmed");
    other.inert = true;
  }
  const slot = document.createElement("li");
  slot.className = "grid-slot";
  slot.dataset.parent = instance.dataset.node;
  // Server-rendered by Askama, which escapes every value.
  slot.innerHTML = html;
  stack.classList.add("editing");
  stack.append(slot);
  layout(grid);
  follow(grid);
  trail(grid.closest(".family-focus"));
  slot.querySelector("elz-item-grid")?.active?.focus();
}

// One field for everything (bipe.md, items 7 and 8): the server says what a scanned code
// was; the page opens the grid of its Instance, or a new family with the ISBN.
async function openScanned() {
  const families = document.getElementById("families");
  const kind = families?.dataset.scan;
  if (!kind) return;
  // Once per answer: the attributes stay in the page while it is edited.
  delete families.dataset.scan;
  if (kind === "new-isbn") {
    if (document.querySelector('[data-action="new-family"]')) newFamily(families.dataset.scanIsbn);
    return;
  }
  const button = families.querySelector(`[data-action="edit-items"][data-from="${CSS.escape(families.dataset.scanInstance ?? "")}"]`);
  if (!button) return;
  await openItemGrid(button);
  const grid = families.querySelector("elz-item-grid");
  const item = families.dataset.scanItem;
  const row = item ? grid?.querySelector(`tbody tr[data-id="${CSS.escape(item)}"]`) : null;
  const cell = row?.querySelector("td[role=gridcell]");
  if (cell) grid.place(cell);
}
openScanned();
document.addEventListener("htmx:afterSettle", openScanned);

document.addEventListener("grid-close", (event) => {
  const element = event.target;
  if (!(element instanceof HTMLElement)) return;
  const grid = element.closest(".family-grid");
  const instance = grid?.querySelector(`[data-node="${element.closest(".grid-slot")?.dataset.parent}"]`);
  if (element.saved) {
    // The pills show the saved Items after the family is read again (item 24).
    location.reload();
    return;
  }
  if (!grid) return;
  closeItemGrids(grid);
  clearLevel(grid);
  follow(grid);
  if (instance instanceof HTMLElement) instance.focus();
});

// A select whose options have icons shows the chosen one's icon beside it
// (bibframe-completo.md, item 14).
document.addEventListener("change", (event) => {
  const select = event.target instanceof HTMLSelectElement ? event.target : null;
  const icon = select?.previousElementSibling;
  if (!icon?.classList.contains("select-icon")) return;
  const symbol = select.selectedOptions[0]?.dataset.icon ?? "";
  icon.hidden = !symbol;
  icon.querySelector("use")?.setAttribute("href", `/static/elzevir-icons.svg#${symbol}`);
});
