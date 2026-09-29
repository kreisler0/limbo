// Limbo password autofill (runs in the page's main frame at document creation).
// It only *detects* login forms and *reports* submitted credentials. Filling is
// done by the host, only after an explicit click, and only for the origin the
// host verified. The page can see what gets filled (as with any extension-based
// password manager); see docs/DECISIONS.md.
(() => {
  if (window.top !== window || window.__limboAutofill) return;
  Object.defineProperty(window, '__limboAutofill', { value: true });
  const wv = window.chrome && window.chrome.webview;
  if (!wv) return;

  const post = (m) => {
    try {
      wv.postMessage(JSON.stringify(Object.assign({ limbo: 1 }, m)));
    } catch (_) {}
  };

  const visible = (el) => {
    if (!el || el.disabled || el.readOnly) return false;
    const r = el.getBoundingClientRect();
    if (r.width < 8 || r.height < 8) return false;
    const s = getComputedStyle(el);
    return s.visibility !== 'hidden' && s.display !== 'none' && s.opacity !== '0';
  };

  const TEXTISH = ['text', 'email', 'tel', ''];
  const userFieldFor = (pw) => {
    const scope = pw.form || document;
    const inputs = Array.from(scope.querySelectorAll('input'));
    const i = inputs.indexOf(pw);
    for (let j = i - 1; j >= 0; j--) {
      const t = (inputs[j].getAttribute('type') || '').toLowerCase();
      if (TEXTISH.includes(t) && visible(inputs[j])) return inputs[j];
    }
    return document.querySelector('input[autocomplete~="username"], input[autocomplete~="email"]');
  };

  const passwordFields = () =>
    Array.from(document.querySelectorAll('input[type="password"]')).filter(visible);

  // Username typed on a first step (e.g. Google asks for the email first).
  let lastUsername = '';
  document.addEventListener(
    'change',
    (e) => {
      const el = e.target;
      if (!(el instanceof HTMLInputElement)) return;
      const t = (el.getAttribute('type') || '').toLowerCase();
      const ac = (el.getAttribute('autocomplete') || '').toLowerCase();
      if ((t === 'email' || ac.includes('username') || ac.includes('email')) && el.value) {
        lastUsername = el.value;
      }
    },
    true,
  );

  // --- detection -------------------------------------------------------------
  let announced = 0;
  let hasLogins = 0;
  const check = () => {
    const fields = passwordFields();
    if (fields.length !== announced) {
      announced = fields.length;
      if (fields.length) post({ t: 'forms', count: fields.length });
    }
    if (hasLogins && fields.length) attachChip(fields[0]);
  };

  // SPA login forms: watch the DOM at most once a second, for 30 s after load.
  let scheduled = false;
  const started = Date.now();
  const observer = new MutationObserver(() => {
    if (scheduled) return;
    if (Date.now() - started > 30000) {
      observer.disconnect();
      return;
    }
    scheduled = true;
    setTimeout(() => {
      scheduled = false;
      check();
    }, 1000);
  });
  const start = () => {
    check();
    observer.observe(document.documentElement, { childList: true, subtree: true });
  };
  if (document.readyState === 'loading') document.addEventListener('DOMContentLoaded', start, { once: true });
  else start();

  // --- capture -----------------------------------------------------------------
  let lastSent = '';
  const capture = () => {
    for (const pw of Array.from(document.querySelectorAll('input[type="password"]'))) {
      if (!pw.value) continue;
      const user = userFieldFor(pw);
      const u = (user && user.value) || lastUsername || '';
      const key = u + '\u0000' + pw.value;
      if (key === lastSent) continue;
      lastSent = key;
      post({ t: 'submit', u, p: pw.value });
      break;
    }
  };
  document.addEventListener('submit', capture, true);
  document.addEventListener(
    'keydown',
    (e) => {
      if (e.key === 'Enter' && e.target instanceof HTMLInputElement) capture();
    },
    true,
  );
  document.addEventListener(
    'click',
    (e) => {
      const b = e.target instanceof Element && e.target.closest('button, input[type="submit"], [role="button"]');
      if (b) setTimeout(capture, 0);
    },
    true,
  );
  window.addEventListener('pagehide', capture);

  // --- the fill chip (only when the host says there are saved logins) ----------
  let chipHost = null;
  const attachChip = (field) => {
    if (chipHost && chipHost.isConnected) return;
    chipHost = document.createElement('limbo-autofill');
    const root = chipHost.attachShadow({ mode: 'closed' });
    root.innerHTML =
      '<style>button{all:unset;cursor:pointer;display:grid;place-items:center;width:22px;height:22px;' +
      'border-radius:6px;background:rgba(55,53,47,.08);color:#37352F;font:12px system-ui;' +
      'box-shadow:0 1px 2px rgba(15,15,15,.1)}button:hover{background:rgba(35,131,226,.16)}</style>' +
      '<button title="Fill saved password" aria-label="Fill saved password">' +
      '<svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" ' +
      'stroke-linecap="round" stroke-linejoin="round"><circle cx="7.5" cy="15.5" r="5.5"/>' +
      '<path d="m21 2-9.6 9.6M15.5 7.5l3 3L22 7l-3-3"/></svg></button>';
    root.querySelector('button').addEventListener('click', (e) => {
      e.preventDefault();
      e.stopPropagation();
      post({ t: 'fillRequest' });
    });
    const place = () => {
      const r = field.getBoundingClientRect();
      Object.assign(chipHost.style, {
        position: 'fixed',
        zIndex: '2147483647',
        left: r.right - 28 + 'px',
        top: r.top + (r.height - 22) / 2 + 'px',
      });
    };
    place();
    window.addEventListener('scroll', place, { passive: true, capture: true });
    window.addEventListener('resize', place, { passive: true });
    document.documentElement.appendChild(chipHost);
  };

  wv.addEventListener('message', (e) => {
    const d = e.data;
    if (d && d.limbo === 1 && d.t === 'hasLogins') {
      hasLogins = d.count | 0;
      check();
    }
  });
})();
