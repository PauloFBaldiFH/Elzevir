// SPDX-License-Identifier: GPL-3.0-or-later
// Run against a disposable server with a staff account allowed to inventory sources.
import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import { chromium } from "playwright";

const base = process.env.ELZEVIR_URL ?? "http://127.0.0.1:8080";
const login = process.env.ELZEVIR_LOGIN ?? "admin";
const password = process.env.ELZEVIR_PASSWORD;
assert.ok(password, "ELZEVIR_PASSWORD is required");
const fixture = new URL("../samples-import-tool/02-folio/datasets/cornell/", import.meta.url);
const records = await Promise.all(
  ["instances.tsv", "recordholdings.tsv", "items.tsv"].map(async (name) => ({
    name,
    mimeType: "application/octet-stream",
    buffer: await readFile(new URL(name, fixture)),
  })),
);

const browser = await chromium.launch({ executablePath: process.env.CHROMIUM_PATH });
try {
  const page = await browser.newPage();
  await page.goto(`${base}/equipe/login`);
  await page.fill("#login", login);
  await page.fill("#password", password);
  await page.click("form.login button[type=submit]");
  await page.waitForURL(`${base}/equipe`);

  async function stage(files) {
    await page.goto(`${base}/equipe/acervo/importar`);
    await page.fill("#source-set-origin", "cornell-e2e");
    await page.locator("#source-set-files").setInputFiles(files);
    await page.locator('form[action$="/conjuntos"] button').click();
    await page.locator("#relation-method").waitFor();
    assert.equal(await page.locator("tbody tr").count(), files.length);
  }

  async function review(plan) {
    if (plan) {
      await page.selectOption("#relation-method", "explicit");
      await page.fill("#relation-plan", JSON.stringify(plan));
    }
    await page.fill("#relation-from", "100");
    await page.locator('form[action$="/relacoes"] button').click();
    const report = JSON.parse(await page.locator("details pre").textContent()).report;
    const tables = await page.locator("tbody").allTextContents();
    assert.ok(tables.some((t) => ["100 @ 54370", "5 @ 1044", "instance (2 @ 1298)"].every((s) => t.includes(s))));
    return report;
  }

  await stage(records);
  const first = await review();
  assert.deepEqual(first.edges.map((edge) => edge.counts), [{ resolved: 107 }, { resolved: 536 }]);
  await stage(records.toReversed().map((record, index) => ({ ...record, name: `source-${index}` })));
  const replay = await review(first.plan);
  assert.equal(replay.evidence, "explicit-v2");
  assert.deepEqual(replay.plan, first.plan);
  assert.deepEqual(replay.edges, first.edges);
  assert.deepEqual(replay.identities.map((d) => d.member), [3, 2, 1]);
  console.log("e2e: Cornell hierarchy and explicit recipe replay passed after renaming and reordering");
} finally {
  await browser.close();
}
