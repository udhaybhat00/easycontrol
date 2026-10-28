import test from "node:test";
import assert from "node:assert/strict";
import { readFile, stat } from "node:fs/promises";
import { execFileSync } from "node:child_process";
import path from "node:path";
import { fileURLToPath } from "node:url";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const config = JSON.parse(
  await readFile(path.join(root, "site.config.json"), "utf8"),
);
execFileSync(process.execPath, ["build.mjs"], { cwd: root });
const pages = Object.fromEntries(
  await Promise.all(
    ["index.html", "docs.html", "404.html"].map(async (name) => [
      name,
      await readFile(path.join(root, "dist", name), "utf8"),
    ]),
  ),
);
const attributes = (source, attribute) =>
  [...source.matchAll(new RegExp(`\\b${attribute}="([^"]*)"`, "g"))].map(
    (match) => match[1],
  );

test("all pages have unique IDs, one h1, descriptive metadata and resolved canonical tokens", () => {
  for (const [file, html] of Object.entries(pages)) {
    const ids = attributes(html, "id");
    assert.equal(ids.length, new Set(ids).size, `${file}: duplicate ID`);
    assert.equal((html.match(/<h1[ >]/g) || []).length, 1, `${file}: h1 count`);
    assert.match(html, /<html lang="en"/);
    assert.match(html, /<meta name="viewport"/);
    assert.match(html, /<title>[^<]+<\/title>/);
    assert.ok(!html.includes("__SITE_URL__"));
    if (file !== "404.html")
      assert.ok(
        html.includes(`${config.url}/${file === "index.html" ? "" : file}`),
      );
  }
});

test("all relative links, fragment references and assets resolve at a Pages project subpath", async () => {
  const base = new URL(`${config.url}/`);
  for (const [file, html] of Object.entries(pages)) {
    for (const reference of [
      ...attributes(html, "href"),
      ...attributes(html, "src"),
    ]) {
      if (
        !reference ||
        reference.startsWith("https://") ||
        reference.startsWith("mailto:")
      )
        continue;
      assert.ok(
        !reference.startsWith("/"),
        `${file}: root-relative URL ${reference}`,
      );
      const target = new URL(reference, new URL(file, base));
      assert.ok(target.pathname.startsWith(base.pathname));
      let local = decodeURIComponent(
        target.pathname.slice(base.pathname.length),
      );
      if (!local || local.endsWith("/")) local += "index.html";
      assert.ok(
        (await stat(path.join(root, "dist", local))).isFile(),
        `${file}: missing ${local}`,
      );
      if (target.hash) {
        const source =
          pages[local] ||
          (await readFile(path.join(root, "dist", local), "utf8"));
        assert.ok(
          attributes(source, "id").includes(
            decodeURIComponent(target.hash.slice(1)),
          ),
          `${file}: missing anchor ${reference}`,
        );
      }
    }
  }
});

test("interactive controls have named targets and all images have alternative text", () => {
  for (const [file, html] of Object.entries(pages)) {
    for (const element of html.matchAll(/<img\b[^>]*>/g))
      assert.match(element[0], /\balt="[^"]*"/, `${file}: image alt`);
    for (const id of [
      ...attributes(html, "data-copy"),
      ...attributes(html, "aria-controls"),
      ...attributes(html, "aria-labelledby"),
    ]) {
      for (const value of id.split(" "))
        assert.ok(
          attributes(html, "id").includes(value),
          `${file}: missing target ${value}`,
        );
    }
  }
  assert.match(pages["index.html"], /role="tablist"/);
  assert.match(pages["docs.html"], /type="search"/);
});

test("site ships no external executable resources, analytics or private app material", async () => {
  for (const html of Object.values(pages)) {
    for (const element of html.matchAll(/<(?:script|iframe)\b[^>]*>/g)) {
      for (const src of attributes(element[0], "src")) {
        assert.ok(
          !src.startsWith("http") || src.startsWith(`${config.url}/`),
          `External executable resource: ${src}`,
        );
      }
    }
    assert.ok(!/<form\b/.test(html));
    assert.ok(
      !/\/Users\/|\/private\/|BEGIN .*PRIVATE KEY|ghp_[A-Za-z0-9]+/.test(html),
    );
  }
  const packageJson = JSON.parse(
    await readFile(path.join(root, "package.json"), "utf8"),
  );
  assert.equal(Object.keys(packageJson.dependencies || {}).length, 0);
  const css = await readFile(path.join(root, "styles.css"), "utf8");
  assert.match(css, /prefers-reduced-motion\s*:\s*reduce/);
  assert.match(css, /focus-visible/);
  assert.ok(!/https?:\/\//.test(css));
  assert.ok((await stat(path.join(root, "assets", "dashboard.png"))).size > 0);
});

test("build emits a sitemap, robots, 404 and static-host marker", async () => {
  const sitemap = await readFile(
    path.join(root, "dist", "sitemap.xml"),
    "utf8",
  );
  assert.ok(sitemap.includes(`${config.url}/docs.html`));
  assert.ok(
    (await readFile(path.join(root, "dist", "robots.txt"), "utf8")).includes(
      "sitemap.xml",
    ),
  );
  assert.ok((await stat(path.join(root, "dist", ".nojekyll"))).isFile());
  assert.ok(pages["404.html"].includes(`${config.url}/`));
});

test("JavaScript parses without adding build/runtime dependencies", () => {
  execFileSync(process.execPath, ["--check", "app.js"], { cwd: root });
});
