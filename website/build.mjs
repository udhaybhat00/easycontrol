import { cp, mkdir, readFile, readdir, rm, writeFile } from 'node:fs/promises';
import { fileURLToPath } from 'node:url';
import path from 'node:path';

const root = path.dirname(fileURLToPath(import.meta.url));
const config = JSON.parse(await readFile(path.join(root, 'site.config.json'), 'utf8'));
const url = new URL(process.env.SITE_URL || config.url).href.replace(/\/$/, '');
const out = path.join(root, 'dist');
await rm(out, { recursive: true, force: true });
await mkdir(out, { recursive: true });
for (const file of ['index.html', 'docs.html', '404.html', 'styles.css', 'app.js', 'assets', '.nojekyll']) {
  await cp(path.join(root, file), path.join(out, file), { recursive: true });
}
for (const file of (await readdir(out)).filter(file => file.endsWith('.html'))) {
  const content = await readFile(path.join(out, file), 'utf8');
  await writeFile(path.join(out, file), content.replaceAll('__SITE_URL__', url));
}
await writeFile(path.join(out, 'robots.txt'), `User-agent: *\nAllow: /\nSitemap: ${url}/sitemap.xml\n`);
await writeFile(path.join(out, 'sitemap.xml'), `<?xml version="1.0" encoding="UTF-8"?>\n<urlset xmlns="http://www.sitemaps.org/schemas/sitemap/0.9"><url><loc>${url}/</loc></url><url><loc>${url}/docs.html</loc></url></urlset>\n`);
console.log(`Built static website → ${out}\nCanonical URL: ${url}/\nNo runtime dependencies or third-party requests.`);
