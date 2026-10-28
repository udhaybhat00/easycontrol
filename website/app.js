// Presentation only. No connection, authentication or desktop APIs run here.
(() => {
  const storage = { get: key => { try { return localStorage.getItem(key); } catch { return null; } }, set: (key, value) => { try { localStorage.setItem(key, value); } catch { /* Browsing remains available with storage blocked. */ } } };
  const options = ['system', 'dark', 'light'];
  let theme = storage.get('easycontrol.website.theme') || 'system';
  if (!options.includes(theme)) theme = 'system';
  function applyTheme() {
    if (theme === 'system') delete document.documentElement.dataset.theme;
    else document.documentElement.dataset.theme = theme;
    document.querySelectorAll('.theme-toggle').forEach(button => {
      const next = options[(options.indexOf(theme) + 1) % options.length];
      button.setAttribute('aria-label', `Theme: ${theme}. Switch to ${next}`);
      button.title = `Theme: ${theme}. Switch to ${next}`;
    });
    document.querySelectorAll('[data-theme-select]').forEach(select => { select.value = theme; });
  }
  applyTheme();
  document.querySelectorAll('.theme-toggle').forEach(button => button.addEventListener('click', () => { theme = options[(options.indexOf(theme) + 1) % options.length]; storage.set('easycontrol.website.theme', theme); applyTheme(); }));
  document.querySelectorAll('[data-theme-select]').forEach(select => select.addEventListener('change', () => { theme = select.value; storage.set('easycontrol.website.theme', theme); applyTheme(); }));

  const menu = document.querySelector('.menu-toggle');
  const nav = document.querySelector('#navigation');
  const closeMenu = () => { menu?.setAttribute('aria-expanded', 'false'); nav?.classList.remove('open'); };
  menu?.addEventListener('click', () => { const expanded = menu.getAttribute('aria-expanded') === 'true'; menu.setAttribute('aria-expanded', String(!expanded)); nav?.classList.toggle('open', !expanded); });
  nav?.querySelectorAll('a').forEach(link => link.addEventListener('click', closeMenu));
  document.addEventListener('click', event => { if (!event.target.closest('.nav-wrap')) closeMenu(); });
  document.addEventListener('keydown', event => { if (event.key === 'Escape' && menu?.getAttribute('aria-expanded') === 'true') { closeMenu(); menu.focus(); } });
  matchMedia('(min-width:761px)').addEventListener('change', event => { if (event.matches) closeMenu(); });

  let toastTimer;
  function toast(message) {
    const element = document.getElementById('toast');
    if (!element) return;
    clearTimeout(toastTimer); element.textContent = message; element.hidden = false;
    toastTimer = setTimeout(() => { element.hidden = true; }, 3500);
  }
  document.querySelectorAll('[data-copy]').forEach(button => button.addEventListener('click', async () => {
    const content = document.getElementById(button.dataset.copy)?.textContent;
    if (!content) return;
    try { await navigator.clipboard.writeText(content); toast('Copied'); }
    catch { toast('Copy unavailable. Select the command and copy it manually.'); }
  }));

  const screenshots = {
    home: { src: 'assets/dashboard.png', caption: 'Home · Dark appearance', alt: 'Actual EasyControl Home screen with host stopped and no recent connections.' },
    settings: { src: 'assets/settings.png', caption: 'Settings · Dark appearance', alt: 'Actual EasyControl settings with macOS Screen Recording and Accessibility granted, and remote input off.' }
  };
  const tabs = Array.from(document.querySelectorAll('[data-screen]'));
  function activateTab(tab) {
    const data = screenshots[tab.dataset.screen];
    if (!data) return;
    tabs.forEach(item => { const selected = item === tab; item.setAttribute('aria-selected', String(selected)); item.tabIndex = selected ? 0 : -1; });
    const image = document.getElementById('workspace-image');
    if (image) { image.src = data.src; image.alt = data.alt; }
    document.getElementById('workspace-caption').textContent = data.caption;
    document.getElementById('screen-panel').setAttribute('aria-labelledby', tab.id);
  }
  tabs.forEach((tab, index) => {
    tab.addEventListener('click', () => activateTab(tab));
    tab.addEventListener('keydown', event => {
      let next;
      if (['ArrowDown', 'ArrowRight'].includes(event.key)) next = (index + 1) % tabs.length;
      if (['ArrowUp', 'ArrowLeft'].includes(event.key)) next = (index - 1 + tabs.length) % tabs.length;
      if (event.key === 'Home') next = 0;
      if (event.key === 'End') next = tabs.length - 1;
      if (next !== undefined) { event.preventDefault(); tabs[next].focus(); activateTab(tabs[next]); }
    });
  });

  const search = document.getElementById('docs-search');
  const sections = Array.from(document.querySelectorAll('[data-doc-section]'));
  const docLinks = Array.from(document.querySelectorAll('.docs-nav-links a'));
  search?.addEventListener('input', () => {
    const query = search.value.trim().toLowerCase();
    let matches = 0;
    sections.forEach(section => {
      const show = !query || section.textContent.toLowerCase().includes(query);
      section.hidden = !show;
      if (show) matches++;
      const link = docLinks.find(item => item.hash === `#${section.id}`);
      if (link) link.hidden = !show;
    });
    const message = document.getElementById('search-message');
    message.textContent = query ? `${matches} section${matches === 1 ? '' : 's'} found` : '';
  });
  if ('IntersectionObserver' in window) {
    const sectionObserver = new IntersectionObserver(entries => entries.forEach(entry => {
      if (entry.isIntersecting) docLinks.forEach(link => link.classList.toggle('active', link.hash === `#${entry.target.id}`));
    }), { rootMargin: '-100px 0px -60% 0px' });
    sections.forEach(section => sectionObserver.observe(section));
    if (!matchMedia('(prefers-reduced-motion:reduce)').matches) {
      document.documentElement.classList.add('motion-ready');
      const revealObserver = new IntersectionObserver(entries => entries.forEach(entry => { if (entry.isIntersecting) { entry.target.classList.add('visible'); revealObserver.unobserve(entry.target); } }), { threshold: .08 });
      document.querySelectorAll('.reveal').forEach(element => revealObserver.observe(element));
    }
  }
})();
