// SPDX-License-Identifier: GPL-3.0-or-later
document.addEventListener("htmx:xhr:progress", (event) => {
  const progress = event.target.closest("[data-import-upload]")?.querySelector("progress");
  if (!progress) return;
  progress.hidden = false;
  if (event.detail.total) {
    progress.value = 100 * event.detail.loaded / event.detail.total;
  } else {
    progress.removeAttribute("value");
  }
});

document.addEventListener("htmx:beforeSwap", (event) => {
  if (event.detail.target?.id !== "import-workspace") return;
  const status = event.detail.xhr.status;
  if (status >= 400 && event.detail.xhr.responseText.includes('id="import-workspace"')) {
    event.detail.shouldSwap = true;
  }
});

let submittingChoices = false;
document.addEventListener("submit", (event) => {
  submittingChoices = event.target.matches("[data-import-edit]");
});

document.addEventListener("keydown", (event) => {
  if (event.key === "Enter" && event.ctrlKey && event.target.id === "import-recipe") {
    event.preventDefault();
    event.target.form.requestSubmit();
  }
  if (event.key === "Escape") {
    const open = event.target.closest("#import-workspace details[open]");
    if (open) {
      open.open = false;
      open.querySelector("summary")?.focus();
    }
  }
});

window.addEventListener("beforeunload", (event) => {
  const recipe = document.querySelector("#import-recipe");
  if (recipe && recipe.value !== recipe.defaultValue && !submittingChoices) {
    event.preventDefault();
    event.returnValue = "";
  }
});
