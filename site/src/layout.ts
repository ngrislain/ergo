const mark = `<svg viewBox="0 0 40 42" fill="none" aria-hidden="true"><path d="m3 16 17-9 17 9v16l-17 9L3 32V16Z" fill="currentColor"/><path d="m3 16 17 9 17-9M20 25v16" stroke="var(--paper)" stroke-width="2"/><ellipse cx="15" cy="11" rx="5" ry="3" fill="currentColor" stroke="var(--paper)" stroke-width="1.5"/><ellipse cx="25" cy="16" rx="5" ry="3" fill="currentColor" stroke="var(--paper)" stroke-width="1.5"/></svg>`;

export const header = `
<a class="skip-link" href="#main">Skip to content</a>
<header class="site-header shell">
  <a class="wordmark" href="/" aria-label="Ergo home">${mark}<span>ergo<span class="wordmark-dot">.</span></span></a>
  <button class="menu-toggle" type="button" aria-expanded="false" aria-controls="site-nav">Menu <span aria-hidden="true">☰</span></button>
  <nav class="site-nav" id="site-nav" aria-label="Main navigation">
    <a href="/#how-it-works">How it works</a>
    <a href="/#connect">For your agent</a>
    <a href="https://github.com/ngrislain/ergo">GitHub <span class="external-mark" aria-hidden="true">↗</span></a>
    <a class="button button-small button-ink" href="https://ergosandbox.com/app">Open sandbox</a>
  </nav>
</header>`;

export const footer = `
<footer class="site-footer shell">
  <div class="footer-brand"><a class="wordmark" href="/" aria-label="Ergo home">${mark}<span>ergo<span class="wordmark-dot">.</span></span></a><p>A little imagination.<br>A world of possibilities.</p></div>
  <nav aria-label="Footer navigation"><a href="/llms.txt">Agent instructions</a><a href="/privacy/">Privacy</a><a href="/terms/">Terms</a><a href="https://github.com/ngrislain/ergo">Source code</a></nav>
  <p class="footer-note">An independent, open-source experiment.<br>Not affiliated with or endorsed by the LEGO Group.</p>
</footer>`;
