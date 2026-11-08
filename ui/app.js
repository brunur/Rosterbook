'use strict';

const { glyphSvg } = window.AL_ICONS;
const WARBAND = 'warband';
const SORTS = [['custom', 'My order'], ['level', 'Level'], ['name', 'Name'], ['gold', 'Gold'], ['recent', 'Last played']];

const $ = (s, root = document) => root.querySelector(s);
const $$ = (s, root = document) => [...root.querySelectorAll(s)];
const roster = $('#roster');
const detail = $('#detail');
const search = $('#search');
const sheet = $('#sheet');
const sheetBody = $('#sheet-body');

function load(k) { try { return JSON.parse(localStorage.getItem(`ledger:${k}`)); } catch { return null; } }
function save(k, v) { try { localStorage.setItem(`ledger:${k}`, JSON.stringify(v)); } catch { /* storage unavailable */ } }

const state = {
  session: null,
  data: null,
  game: load('game') ?? 0,     // index into data.games (per device)
  overview: false,              // viewing the all-games summary instead of one game
  picks: load('picks') || {},   // selected character per game (per device)
  tab: load('tab') || 'overview',
  query: '',
  highlight: null,
  arranging: false,
  panel: null,                  // 'filter' | 'sort' | null
};

// ---------- helpers ----------
const esc = (s) => String(s ?? '').replace(/[&<>"']/g, (c) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' }[c]));
const fmt = (n) => Number(n || 0).toLocaleString();
const plural = (n, one, many = `${one}s`) => `${n} ${n === 1 ? one : many}`;

async function api(method, path, body) {
  const res = await fetch(path, {
    method,
    cache: 'no-store',
    headers: { 'X-Alt-Ledger': '1', ...(body !== undefined ? { 'Content-Type': 'application/json' } : {}) },
    body: body !== undefined ? JSON.stringify(body) : undefined,
  });
  if (!res.ok) {
    let msg = `Request failed (${res.status})`;
    try { msg = (await res.json()).error || msg; } catch { /* not JSON */ }
    const err = new Error(msg);
    err.status = res.status;
    throw err;
  }
  return res.status === 204 ? null : res.json();
}

let toastTimer;
function toast(msg) {
  const t = $('#toast');
  t.textContent = msg;
  t.classList.add('show');
  clearTimeout(toastTimer);
  toastTimer = setTimeout(() => t.classList.remove('show'), 2600);
}

function coins(copper) {
  copper = Math.max(0, Math.floor(copper || 0));
  const g = Math.floor(copper / 10000), s = Math.floor((copper % 10000) / 100), c = copper % 100;
  const parts = [];
  if (g) parts.push(`<span class="coin g">${fmt(g)}<i></i></span>`);
  if (g || s) parts.push(`<span class="coin s">${s}<i></i></span>`);
  parts.push(`<span class="coin c">${c}<i></i></span>`);
  return `<span class="coins num" aria-label="${g} gold ${s} silver ${c} copper">${parts.join('')}</span>`;
}

function duration(sec) {
  sec = Math.max(0, sec || 0);
  const d = Math.floor(sec / 86400), h = Math.floor((sec % 86400) / 3600), m = Math.floor((sec % 3600) / 60);
  if (d) return `${d}d ${h}h`;
  if (h) return `${h}h ${m}m`;
  return `${m}m`;
}

function ago(ts) {
  if (!ts) return 'never';
  const s = Math.max(0, Date.now() / 1000 - ts);
  if (s < 90) return 'just now';
  if (s < 3600) return `${Math.round(s / 60)} min ago`;
  if (s < 86400) return `${Math.round(s / 3600)} h ago`;
  return plural(Math.round(s / 86400), 'day') + ' ago';
}

function xpBar(c, big = false) {
  const { current, max, rested } = c.xp;
  if (!max) return '';
  const fill = Math.min(100, (current / max) * 100);
  const rest = Math.min(100, ((current + rested.now) / max) * 100);
  return `<div class="xpbar${big ? ' big' : ''}" role="img" aria-label="${fmt(current)} of ${fmt(max)} XP, ${fmt(rested.now)} rested">
    <span class="rest" style="width:${rest}%"></span><span class="fill" style="width:${fill}%"></span>${big ? '<span class="ticks"></span>' : ''}
  </div>`;
}

function icon(item, count = 0) {
  // The game's own picture when "item pictures from my WoW install" is on and
  // this one has been read; Rosterbook's drawing otherwise (and if it fails to load).
  const g = cur();
  const pic = item.icon && g?.iconSet?.has(item.icon)
    ? `<img class="gp" src="/api/icon/${encodeURIComponent(g.id)}/${item.icon}" alt="" loading="lazy" decoding="async">`
    : '';
  return `<span class="ico${pic ? ' has-pic' : ''}" style="--q:var(--q${item.quality ?? 1})">${pic}${glyphSvg(item)}${count > 1 ? `<span class="c num">${fmt(count)}</span>` : ''}</span>`;
}

// A picture that can't load falls back to the drawing (no inline handlers: CSP).
document.addEventListener('error', (e) => {
  if (e.target instanceof HTMLImageElement && e.target.classList.contains('gp')) {
    e.target.parentElement?.classList.remove('has-pic');
    e.target.remove();
  }
}, true);

const slotsUsed = (bags) => bags.reduce((a, b) => [a[0] + (b.size - b.free), a[1] + b.size], [0, 0]);

// ---------- theme ----------
const darkQuery = matchMedia('(prefers-color-scheme: dark)');
function applyTheme(theme) {
  const t = theme === 'light' || theme === 'dark' ? theme : (darkQuery.matches ? 'dark' : 'light');
  document.documentElement.dataset.theme = t;
  document.querySelector('meta[name="theme-color"]').content = t === 'dark' ? '#18202c' : '#eef1f5';
}
darkQuery.addEventListener('change', () => applyTheme(currentTheme()));
const currentTheme = () => state.data?.theme || state.session?.theme || 'system';

// ---------- game / view ----------
function cur() {
  const games = state.data?.games || [];
  return games[state.game] || games[0];
}

function viewOf(g) {
  g.view = g.view || {};
  g.view.sort = g.view.sort || 'custom';
  for (const k of ['order', 'pinned', 'hidden']) g.view[k] = g.view[k] || [];
  g.view.filters = g.view.filters || {};
  for (const k of ['realms', 'factions', 'races', 'classes']) g.view.filters[k] = g.view.filters[k] || [];
  return g.view;
}

let saveTimer;
function saveView(g) {
  clearTimeout(saveTimer);
  const view = JSON.parse(JSON.stringify(viewOf(g)));
  saveTimer = setTimeout(() => {
    api('PUT', `/api/view/${encodeURIComponent(g.id)}`, view).catch((e) => toast(e.message));
  }, 250);
}

const FILTER_DIMS = [
  ['factions', 'Faction', (c) => c.faction],
  ['realms', 'Realm', (c) => c.realm],
  ['races', 'Race', (c) => c.race],
  ['classes', 'Class', (c) => c.className],
];

function activeFilterCount(view) {
  return FILTER_DIMS.reduce((n, [k]) => n + (view.filters[k].length ? 1 : 0), 0);
}

function passesFilters(c, view) {
  return FILTER_DIMS.every(([k, , get]) => !view.filters[k].length || view.filters[k].includes(get(c)));
}

function sortedChars(g, { includeHidden = false } = {}) {
  const view = viewOf(g);
  let list = g.characters.filter((c) => passesFilters(c, view));
  if (!includeHidden) list = list.filter((c) => !view.hidden.includes(c.key));
  const pos = new Map(view.order.map((k, i) => [k, i]));
  const by = {
    custom: (a, b) => (pos.get(a.key) ?? 1e6) - (pos.get(b.key) ?? 1e6) || b.level - a.level || a.name.localeCompare(b.name),
    level: (a, b) => b.level - a.level || a.name.localeCompare(b.name),
    name: (a, b) => a.name.localeCompare(b.name),
    gold: (a, b) => b.money - a.money,
    recent: (a, b) => b.lastLogout - a.lastLogout,
  }[view.sort] || ((a, b) => b.level - a.level);
  const pinned = (c) => (view.pinned.includes(c.key) ? 0 : 1);
  return [...list].sort((a, b) => pinned(a) - pinned(b) || by(a, b));
}

// ---------- roster ----------
const SVG = {
  filter: '<svg viewBox="0 0 24 24" aria-hidden="true"><path d="M4 5h16l-6 7.5V19l-4 1.5v-8z"/></svg>',
  arrange: '<svg viewBox="0 0 24 24" aria-hidden="true"><path d="M7 4v16M4 7l3-3 3 3M17 20V4M14 17l3 3 3-3"/></svg>',
  handle: '<svg viewBox="0 0 16 16" aria-hidden="true"><circle cx="5.5" cy="4" r="1.3"/><circle cx="10.5" cy="4" r="1.3"/><circle cx="5.5" cy="8" r="1.3"/><circle cx="10.5" cy="8" r="1.3"/><circle cx="5.5" cy="12" r="1.3"/><circle cx="10.5" cy="12" r="1.3"/></svg>',
  pin: '<svg viewBox="0 0 24 24" aria-hidden="true"><path d="M9 4h6l-1 6 4 3v2h-5v5l-1 1-1-1v-5H6v-2l4-3z"/></svg>',
  pinFill: '<svg class="pin" viewBox="0 0 24 24" aria-hidden="true"><path d="M9 4h6l-1 6 4 3v2h-5v5l-1 1-1-1v-5H6v-2l4-3z"/></svg>',
  eye: '<svg viewBox="0 0 24 24" aria-hidden="true"><path d="M2 12s3.5-7 10-7 10 7 10 7-3.5 7-10 7S2 12 2 12z"/><circle cx="12" cy="12" r="3"/></svg>',
  eyeOff: '<svg viewBox="0 0 24 24" aria-hidden="true"><path d="M3 3l18 18M10.6 5.1A10.4 10.4 0 0 1 12 5c6.5 0 10 7 10 7a17.6 17.6 0 0 1-3.2 4.2M6.6 6.6C3.9 8.4 2 12 2 12s3.5 7 10 7c1.8 0 3.4-.5 4.8-1.3M9.9 9.9a3 3 0 0 0 4.2 4.2"/></svg>',
};

function gameTabs() {
  const games = state.data.games;
  if (games.length < 2) return '';
  return `<div class="games" role="tablist" aria-label="Game">
    <button class="game-btn" role="tab" data-overview aria-selected="${state.overview}">All</button>
    ${games.map((x, i) => `<button class="game-btn" role="tab" data-game="${i}" aria-selected="${!state.overview && i === state.game}">${esc(x.name)}</button>`).join('')}
  </div>`;
}

function renderRoster() {
  if (state.overview) {
    roster.classList.remove('arranging');
    roster.innerHTML = gameTabs();
    return;
  }
  const g = cur();
  const games = state.data.games;
  const view = viewOf(g);
  const selected = state.picks[g.id];
  const all = g.characters;
  const shown = sortedChars(g, { includeHidden: state.arranging });
  const hiddenCount = all.filter((c) => view.hidden.includes(c.key)).length;
  const nFilters = activeFilterCount(view);

  const label = shown.length === all.length || state.arranging
    ? plural(all.length, 'character')
    : `${shown.length} of ${all.length} characters`;

  roster.classList.toggle('arranging', state.arranging);
  roster.innerHTML = `
    ${suggestionBanner()}
    ${gameTabs()}
    <div class="account">
      <div class="account-gold">${coins(g.totals.money + (g.warband?.money || 0))}</div>
      <div class="account-meta">${plural(g.totals.characters, 'character')}, ${duration(g.totals.played)} played</div>
    </div>
    <div class="roster-tools">
      <span class="label">${label}${hiddenCount && !state.arranging ? `, ${hiddenCount} hidden` : ''}</span>
      <button class="icon-btn" data-panel="filter" aria-pressed="${state.panel === 'filter'}" aria-label="Filter characters" title="Filter">
        ${SVG.filter}${nFilters ? `<span class="badge">${nFilters}</span>` : ''}
      </button>
      <button class="icon-btn" data-panel="sort" aria-pressed="${state.panel === 'sort'}" aria-label="Sort and arrange" title="Sort and arrange">${SVG.arrange}</button>
    </div>
    ${state.panel === 'filter' ? filterPanel(g) : ''}
    ${state.panel === 'sort' ? sortPanel(g) : ''}
    <div class="char-list">
      ${shown.map((c) => charRow(c, view, selected)).join('') || `<p class="roster-empty">${all.length ? 'No characters match these filters.' : 'No characters saved yet.'}</p>`}
    </div>
    ${g.warband ? `
      <div class="char-row">
        <button class="char-btn warband-btn" style="--cls:var(--accent)" data-id="${WARBAND}" aria-current="${selected === WARBAND && !state.query}">
          <span class="nm cls-ink"><span>Warband bank</span></span>
          <span class="gold-sm">${coins(g.warband.money || 0)}</span>
          <span class="sub">Shared by all characters</span>
        </button>
      </div>` : ''}`;
}

// A game on this PC started saving Rosterbook data but isn't shown (PC only).
function suggestionBanner() {
  const list = state.data?.suggestions || [];
  if (state.session?.role !== 'owner' || !list.length) return '';
  const s = list[0];
  const more = list.length > 1 ? ` <span class="hint">(${list.length - 1} more)</span>` : '';
  const version = s.gameVersion ? ` ${esc(s.gameVersion.split('.').slice(0, 3).join('.'))}` : '';
  const [text, action, label] = {
    game: [`<b>New characters found</b> in ${esc(s.name)}${s.lastSaved ? `, saved ${ago(s.lastSaved)}` : ''}.`, 'add', 'Show them'],
    addon: [`<b>${esc(s.name)}</b> doesn’t have the Rosterbook add-on yet.`, 'install', 'Install add-on'],
    'addon-update': [`<b>${esc(s.name)}</b> was updated${version ? ` to${version}` : ''}. Update the add-on so the game doesn’t switch it off.`, 'install', 'Update add-on'],
  }[s.kind] || [];
  if (!text) return '';
  return `<div class="suggest" role="status">
    <p>${text}${more}</p>
    <div class="suggest-actions">
      <button class="btn small primary" data-suggest="${action}" data-id="${esc(s.id)}">${label}</button>
      <button class="btn small" data-suggest="dismiss" data-id="${esc(s.id)}">Ignore</button>
    </div>
  </div>`;
}

async function answerSuggestion(answer, id) {
  const name = state.data.suggestions.find((s) => s.id === id)?.name || 'that game';
  await api('POST', `/api/setup/suggestions/${encodeURIComponent(id)}/${answer}`);
  await fetchData();
  if (answer === 'install') {
    toast(`Add-on installed in ${name}. If the game is running, restart it to load the add-on.`);
  } else if (answer === 'add') {
    const i = state.data.games.findIndex((g) => g.id === id);
    if (i >= 0) {
      state.game = i;
      save('game', i);
      render();
    }
    toast(`Now showing ${name}`);
  } else {
    toast(`Ignored. You can still do this in Settings → Games.`);
  }
}

function charRow(c, view, selected) {
  const pinned = view.pinned.includes(c.key);
  const hidden = view.hidden.includes(c.key);
  return `
    <div class="char-row${hidden ? ' is-hidden' : ''}" data-key="${esc(c.key)}">
      ${state.arranging ? `<button class="handle" data-handle="${esc(c.key)}" aria-label="Move ${esc(c.firstName)} (use arrow keys)" title="Drag to reorder">${SVG.handle}</button>` : ''}
      <button class="char-btn" style="--cls:${c.classColor}" data-id="${esc(c.key)}" aria-current="${c.key === selected && !state.query}">
        <span class="nm cls-ink">${pinned ? SVG.pinFill : ''}<span>${esc(c.firstName)}</span></span>
        <span class="lvl num">${c.level}</span>
        <span class="sub">${esc(c.race)} ${esc(c.className)}</span>
        <span class="gold-sm">${coins(c.money)}</span>
        ${xpBar(c)}
      </button>
      ${state.arranging ? `<span class="row-actions">
        <button class="icon-btn" data-pin="${esc(c.key)}" aria-pressed="${pinned}" aria-label="${pinned ? 'Unpin' : 'Pin to top'}" title="${pinned ? 'Unpin' : 'Pin to top'}">${SVG.pin}</button>
        <button class="icon-btn" data-hide="${esc(c.key)}" aria-pressed="${hidden}" aria-label="${hidden ? 'Show' : 'Hide'}" title="${hidden ? 'Show' : 'Hide'}">${hidden ? SVG.eyeOff : SVG.eye}</button>
      </span>` : ''}
    </div>`;
}

function filterPanel(g) {
  const view = viewOf(g);
  const sections = FILTER_DIMS.map(([k, title, get]) => {
    const counts = new Map();
    for (const c of g.characters) {
      const v = get(c);
      if (v) counts.set(v, (counts.get(v) || 0) + 1);
    }
    if (counts.size < 2 && !view.filters[k].length) return '';
    const chips = [...counts].sort((a, b) => a[0].localeCompare(b[0])).map(([v, n]) =>
      `<button class="chip" data-filter="${k}" data-value="${esc(v)}" aria-pressed="${view.filters[k].includes(v)}">${esc(v)}<span class="ct">${n}</span></button>`).join('');
    return `<h3>${title}</h3><div class="chips">${chips}</div>`;
  }).join('');
  return `<div class="panel-pop">
    ${sections || '<p class="hint">Filters appear once you have characters of different factions, realms, races or classes.</p>'}
    ${activeFilterCount(view) ? '<div class="pop-foot"><span></span><button class="btn small" data-clear-filters>Clear filters</button></div>' : ''}
  </div>`;
}

function sortPanel(g) {
  const view = viewOf(g);
  return `<div class="panel-pop">
    <h3>Sort by</h3>
    <div class="chips">${SORTS.map(([k, label]) => `<button class="chip" data-sort="${k}" aria-pressed="${view.sort === k}">${label}</button>`).join('')}</div>
    <div class="pop-foot">
      <span class="hint">${state.arranging ? 'Drag to reorder. Pin or hide characters.' : 'Pinned characters stay at the top.'}</span>
      <button class="btn small${state.arranging ? ' primary' : ''}" data-arrange>${state.arranging ? 'Done' : 'Arrange'}</button>
    </div>
  </div>`;
}

roster.addEventListener('click', (e) => {
  const g = cur();
  const view = viewOf(g);
  const t = e.target.closest('button');
  if (!t) return;

  if (t.dataset.suggest) {
    t.disabled = true;
    answerSuggestion(t.dataset.suggest, t.dataset.id).catch((err) => { toast(err.message); t.disabled = false; });
    return;
  }

  if (t.dataset.overview !== undefined) {
    state.overview = true;
    state.highlight = null;
    state.panel = null;
    state.arranging = false;
    render();
    detail.scrollTop = 0;
  } else if (t.dataset.game !== undefined) {
    state.overview = false;
    state.game = Number(t.dataset.game);
    state.highlight = null;
    state.panel = null;
    state.arranging = false;
    save('game', state.game);
    render();
    detail.scrollTop = 0;
  } else if (t.dataset.panel) {
    state.panel = state.panel === t.dataset.panel ? null : t.dataset.panel;
    if (state.panel !== 'sort') state.arranging = false;
    renderRoster();
  } else if (t.dataset.filter) {
    const list = view.filters[t.dataset.filter];
    const v = t.dataset.value;
    view.filters[t.dataset.filter] = list.includes(v) ? list.filter((x) => x !== v) : [...list, v];
    saveView(g);
    render();
  } else if (t.hasAttribute('data-clear-filters')) {
    for (const [k] of FILTER_DIMS) view.filters[k] = [];
    saveView(g);
    render();
  } else if (t.dataset.sort) {
    if (t.dataset.sort === 'custom' && view.sort !== 'custom' && !view.order.length) {
      view.order = sortedChars(g, { includeHidden: true }).map((c) => c.key);
    }
    view.sort = t.dataset.sort;
    saveView(g);
    renderRoster();
  } else if (t.hasAttribute('data-arrange')) {
    state.arranging = !state.arranging;
    renderRoster();
  } else if (t.dataset.pin) {
    const k = t.dataset.pin;
    view.pinned = view.pinned.includes(k) ? view.pinned.filter((x) => x !== k) : [...view.pinned, k];
    saveView(g);
    renderRoster();
  } else if (t.dataset.hide) {
    const k = t.dataset.hide;
    view.hidden = view.hidden.includes(k) ? view.hidden.filter((x) => x !== k) : [...view.hidden, k];
    saveView(g);
    render();
  } else if (t.dataset.id) {
    selectCharacter(t.dataset.id);
  }
});

// ----- drag to reorder (mouse, touch and pen) -----
let drag = null;

function moveKey(g, key, toIndexKey, after) {
  const view = viewOf(g);
  const keys = sortedChars(g, { includeHidden: true }).map((c) => c.key);
  const from = keys.indexOf(key);
  if (from < 0) return;
  keys.splice(from, 1);
  let to = keys.indexOf(toIndexKey);
  if (to < 0) return;
  if (after) to += 1;
  keys.splice(to, 0, key);
  // Keep pinned characters in front by unpinning nothing; order among them is preserved.
  view.order = keys;
  view.sort = 'custom';
  saveView(g);
}

roster.addEventListener('pointerdown', (e) => {
  const h = e.target.closest('[data-handle]');
  if (!h) return;
  e.preventDefault();
  h.setPointerCapture(e.pointerId);
  const row = h.closest('.char-row');
  row.classList.add('dragging');
  drag = { key: h.dataset.handle, row, target: null, after: false };
});

roster.addEventListener('pointermove', (e) => {
  if (!drag) return;
  const el = document.elementFromPoint(e.clientX, e.clientY);
  const row = el?.closest('.char-row[data-key]');
  $$('.drop-before, .drop-after', roster).forEach((r) => r.classList.remove('drop-before', 'drop-after'));
  if (!row || row === drag.row) { drag.target = null; return; }
  const rect = row.getBoundingClientRect();
  drag.after = e.clientY > rect.top + rect.height / 2;
  drag.target = row.dataset.key;
  row.classList.add(drag.after ? 'drop-after' : 'drop-before');
  // Auto-scroll the list near its edges.
  const r = roster.getBoundingClientRect();
  if (e.clientY < r.top + 40) roster.scrollTop -= 12;
  if (e.clientY > r.bottom - 40) roster.scrollTop += 12;
});

function endDrag() {
  if (!drag) return;
  const { key, target, after } = drag;
  drag = null;
  if (target) moveKey(cur(), key, target, after);
  renderRoster();
}
roster.addEventListener('pointerup', endDrag);
roster.addEventListener('pointercancel', () => { drag = null; renderRoster(); });

roster.addEventListener('keydown', (e) => {
  const h = e.target.closest('[data-handle]');
  if (!h || (e.key !== 'ArrowUp' && e.key !== 'ArrowDown')) return;
  e.preventDefault();
  const g = cur();
  const keys = sortedChars(g, { includeHidden: true }).map((c) => c.key);
  const i = keys.indexOf(h.dataset.handle);
  const j = e.key === 'ArrowUp' ? i - 1 : i + 1;
  if (j < 0 || j >= keys.length) return;
  moveKey(g, h.dataset.handle, keys[j], e.key === 'ArrowDown');
  renderRoster();
  $(`[data-handle="${CSS.escape(h.dataset.handle)}"]`, roster)?.focus();
});

function selectCharacter(id, tab, highlight = null) {
  state.picks[cur().id] = id;
  if (tab && id !== WARBAND) state.tab = tab;
  state.highlight = highlight;
  state.query = '';
  search.value = '';
  save('picks', state.picks);
  save('tab', state.tab);
  render();
  detail.scrollTop = 0;
}

// ---------- character detail ----------
const TABS = [['overview', 'Overview'], ['bags', 'Bags'], ['bank', 'Bank'], ['gear', 'Gear'], ['rep', 'Reputation']];

function renderCharacter(c) {
  const [used, total] = slotsUsed(c.bags);
  const bankItems = c.bank.reduce((n, b) => n + b.items.length, 0);
  const counts = { bags: `${used}/${total}`, bank: c.bankVisited ? bankItems : '', rep: c.reputations.length };
  const r = c.xp.rested;

  detail.style.setProperty('--cls', c.classColor);
  detail.innerHTML = `
    <section class="hero">
      <div>
        <h1><span class="cls-ink">${esc(c.firstName)}</span>${c.surname ? ` <span class="surname">${esc(c.surname)}</span>` : ''}</h1>
        <div class="who">Level <b class="num">${c.level}</b> ${esc(c.race)} <b>${esc(c.className)}</b>${c.faction !== 'Unknown' ? `, ${esc(c.faction)}` : ''}${c.realm ? `, ${esc(c.realm)}` : ''}</div>
      </div>
      <div>
        <div class="gold-big">${coins(c.money)}</div>
        <div class="seen">Last seen ${ago(c.lastLogout)}</div>
      </div>
    </section>
    ${c.xp.max ? `<section class="xp-block">
      ${xpBar(c, true)}
      <div class="xp-text">
        <span><b class="num">${fmt(c.xp.current)}</b> / ${fmt(c.xp.max)} XP (${Math.floor((c.xp.current / c.xp.max) * 100)}%)</span>
        <span class="r">${fmt(r.now)} rested${r.now >= r.cap ? ', fully rested' : ` (${Math.round(r.pct)}% of max)`}${c.xp.resting ? ', in an inn' : ''}</span>
      </div>
    </section>` : ''}
    <div class="tabs" role="tablist">
      ${TABS.map(([k, label]) => `<button class="tab" role="tab" data-tab="${k}" aria-selected="${state.tab === k}">${label}${counts[k] !== undefined && counts[k] !== '' ? `<span class="ct num">${counts[k]}</span>` : ''}</button>`).join('')}
    </div>
    <section role="tabpanel">${renderTab(c)}</section>`;
  $('.slot.hit', detail)?.scrollIntoView({ block: 'center' });
}

function renderTab(c) {
  switch (state.tab) {
    case 'bags': return renderBags(c.bags);
    case 'bank': return c.bankVisited
      ? renderBags(c.bank)
      : `<p class="note">No bank saved for ${esc(c.firstName)} yet. Open the bank on this character once and it will appear here.</p>`;
    case 'gear': return renderGear(c);
    case 'rep': return renderReps(c);
    default: return renderOverview(c);
  }
}

function renderOverview(c) {
  const [used, total] = slotsUsed(c.bags);
  const facts = [
    ['Location', c.subZone && c.subZone !== c.zone ? `${c.subZone}, ${c.zone}` : c.zone],
    ['Hearthstone', c.bindLocation],
    ['Time played', c.played ? duration(c.played) : null],
    ['This level', c.xp.max && c.playedThisLevel ? duration(c.playedThisLevel) : null],
    ['Health', c.health ? fmt(c.health) : null],
    [c.power?.type || 'Power', c.power ? fmt(c.power.max) : null],
    ['Average item level', c.itemLevel ? c.itemLevel.toFixed(1) : null],
    ['Bag space', total ? `${total - used} free of ${total}` : null],
    ['Mail', c.mailCount
      ? `${c.mailCount} waiting${c.mailExpires ? `, first expires in ${duration(c.mailExpires - Date.now() / 1000)}` : ''}`
      : 'None'],
  ].filter(([, v]) => v);
  const profs = c.professions.length
    ? `<div class="profs">${c.professions.map((p) => `
        <div class="prof${p.secondary ? ' sec-skill' : ''}">
          <div class="row"><span>${esc(p.name)}</span><span class="num">${p.rank} / ${p.max}</span></div>
          <div class="meter"><i style="width:${p.max ? (p.rank / p.max) * 100 : 0}%"></i></div>
        </div>`).join('')}</div>`
    : '<p class="note">No professions learned yet.</p>';
  return `<dl class="facts">${facts.map(([k, v]) => `<div class="fact"><dt>${esc(k)}</dt><dd class="num">${esc(v)}</dd></div>`).join('')}</dl>
    <h2 class="sec">Professions</h2>${profs}`;
}

function renderBags(bags) {
  if (!bags.length) return '<p class="note">Nothing saved here yet.</p>';
  return bags.map((b) => {
    const bySlot = new Map(b.items.map((i) => [i.slot, i]));
    const size = Math.max(b.size, ...b.items.map((i) => i.slot), 0);
    const cells = [];
    for (let s = 1; s <= size; s++) {
      const it = bySlot.get(s);
      if (!it) { cells.push('<div class="slot free" aria-label="Empty slot"></div>'); continue; }
      const hit = state.highlight && it.id === state.highlight;
      cells.push(`<div class="slot${hit ? ' hit' : ''}" style="--q:var(--q${it.quality ?? 1})" title="${esc(it.name)}${it.count > 1 ? ` x${it.count}` : ''}">
        ${icon(it, it.count)}<span class="n">${esc(it.name)}</span></div>`);
    }
    return `<div class="bag"><div class="bag-head"><h3>${esc(b.name)}</h3><span class="num">${b.size - b.free} / ${b.size}</span></div>
      <div class="slots">${cells.join('')}</div></div>`;
  }).join('');
}

function renderGear(c) {
  if (!c.equipment.length) return '<p class="note">No equipment saved.</p>';
  return `<dl class="gear">${c.equipment.map((e) => `
    <div class="gear-row" style="--q:var(--q${e.quality ?? 1})"><dt>${esc(e.slotName)}</dt><dd>${icon(e)}<span>${esc(e.name)}</span></dd></div>`).join('')}</dl>`;
}

function repRow(r) {
  return `<div class="rep" style="--st:${r.color}">
    <div class="row">
      <span class="nm">${esc(r.name)}</span>
      <span><span class="st" style="color:${r.color}">${esc(r.label)}</span> <span class="num" style="color:var(--muted)">${fmt(r.value)} / ${fmt(r.max)}</span></span>
    </div>
    <div class="meter"><i style="width:${r.pct}%"></i></div>
  </div>`;
}

function renderReps(c) {
  if (!c.reputations.length) return '<p class="note">No reputations saved.</p>';
  if (!c.reputations.some((r) => r.group)) return `<div class="reps">${c.reputations.map(repRow).join('')}</div>`;
  const groups = new Map();
  for (const r of c.reputations) {
    const k = r.group || 'Other';
    if (!groups.has(k)) groups.set(k, []);
    groups.get(k).push(r);
  }
  return [...groups].map(([name, reps]) => `<h2 class="sec rep-group">${esc(name)}</h2><div class="reps">${reps.map(repRow).join('')}</div>`).join('');
}

function renderWarband(w) {
  detail.style.setProperty('--cls', 'var(--accent)');
  detail.innerHTML = `
    <section class="hero">
      <div><h1><span class="cls-ink">Warband bank</span></h1><div class="who">Shared by every character on this account</div></div>
      <div><div class="gold-big">${coins(w.money || 0)}</div><div class="seen">Last seen ${ago(w.scanned)}</div></div>
    </section>
    ${renderBags(w.tabs)}`;
  $('.slot.hit', detail)?.scrollIntoView({ block: 'center' });
}

detail.addEventListener('click', (e) => {
  const tab = e.target.closest('.tab[data-tab]');
  if (tab) {
    state.tab = tab.dataset.tab;
    state.highlight = null;
    save('tab', state.tab);
    render();
    return;
  }
  const card = e.target.closest('.overview-card[data-game]');
  if (card) {
    state.overview = false;
    state.game = Number(card.dataset.game);
    save('game', state.game);
    render();
    detail.scrollTop = 0;
    return;
  }
  const suggest = e.target.closest('[data-suggest]');
  if (suggest) {
    suggest.disabled = true;
    answerSuggestion(suggest.dataset.suggest, suggest.dataset.id).catch((err) => { toast(err.message); suggest.disabled = false; });
    return;
  }
  const where = e.target.closest('.where-btn[data-id]');
  if (where) selectCharacter(where.dataset.id, where.dataset.tab, Number(where.dataset.item));
  if (e.target.closest('[data-open-setup]')) openSettings('games');
});

// ---------- item search ----------
function renderSearch(q) {
  const needle = q.toLowerCase();
  const found = new Map();
  const add = (owner, it, place, tab) => {
    if (!it.name.toLowerCase().includes(needle)) return;
    const key = it.id || it.name;
    if (!found.has(key)) found.set(key, { item: it, total: 0, where: new Map() });
    const f = found.get(key);
    const n = it.count || 1;
    f.total += n;
    const wk = `${owner.key}:${place}`;
    const w = f.where.get(wk) || { owner, place, tab, count: 0 };
    w.count += n;
    f.where.set(wk, w);
  };
  const g = cur();
  for (const c of g.characters) {
    for (const b of c.bags) for (const it of b.items) add(c, it, 'bags', 'bags');
    for (const b of c.bank) for (const it of b.items) add(c, it, 'bank', 'bank');
    for (const e of c.equipment) add(c, { ...e, count: 1 }, 'equipped', 'gear');
  }
  if (g.warband) {
    const wb = { key: WARBAND, firstName: 'Warband', classColor: 'var(--accent)' };
    for (const b of g.warband.tabs) for (const it of b.items) add(wb, it, 'bank', 'bank');
  }
  const items = [...found.values()].sort((a, b) => a.item.name.localeCompare(b.item.name));
  detail.style.removeProperty('--cls');
  detail.innerHTML = `<section class="results">
    <h1>${items.length ? `${plural(items.length, 'item')} matching “${esc(q)}”` : `Nothing matches “${esc(q)}”`}</h1>
    <p class="lead">${items.length ? 'Choose a character to open the bag that holds it.' : `Search covers bags, bank and equipped gear on every ${esc(g.name)} character.`}</p>
    ${items.map((f) => `<div class="hit-item">
      <div class="top"><span class="n" style="--q:var(--q${f.item.quality ?? 1})">${icon(f.item)}${esc(f.item.name)}</span><span class="num">${fmt(f.total)} total</span></div>
      <div class="where">${[...f.where.values()].map((w) => `
        <button class="where-btn" style="--cls:${w.owner.classColor}" data-id="${esc(w.owner.key)}" data-tab="${w.tab}" data-item="${f.item.id || ''}">
          <b class="cls-ink">${esc(w.owner.firstName)}</b> <span>${w.place}</span> <span class="num">${fmt(w.count)}</span>
        </button>`).join('')}</div>
    </div>`).join('')}
  </section>`;
}

let searchTimer;
search.addEventListener('input', () => {
  clearTimeout(searchTimer);
  searchTimer = setTimeout(() => { state.query = search.value.trim(); render(); }, 120);
});
search.addEventListener('keydown', (e) => {
  if (e.key === 'Escape') { search.value = ''; state.query = ''; render(); }
});

// ---------- main render ----------
function render() {
  applyTheme(currentTheme());
  if (state.session?.role === 'unpaired') return renderUnpaired();
  if (!state.data) return;
  const games = state.data.games || [];
  if (!games.length) {
    roster.innerHTML = '';
    detail.innerHTML = `<div class="empty"><h2>Choose your games</h2>
      <p>${state.session?.role === 'owner'
        ? 'Rosterbook needs to know where World of Warcraft keeps its saved data.'
        : 'Rosterbook hasn’t been set up yet. Open it on your PC to choose your games.'}</p>
      ${state.session?.role === 'owner' ? '<button class="btn primary" data-open-setup>Set up games</button>' : ''}</div>`;
    return;
  }
  if (!games[state.game]) state.game = 0;
  if (games.length < 2) state.overview = false;
  renderRoster();
  if (state.overview) return renderOverview();
  const g = cur();
  const shown = sortedChars(g);
  let pick = state.picks[g.id];
  const valid = (pick === WARBAND && g.warband) || g.characters.some((c) => c.key === pick);
  if (!valid) pick = state.picks[g.id] = shown[0]?.key ?? g.characters[0]?.key ?? (g.warband ? WARBAND : null);

  if (state.query.length >= 2) return renderSearch(state.query);
  if (pick == null) {
    detail.innerHTML = `<div class="empty"><h2>No characters yet</h2>
      <p>Log in to a character with the Rosterbook add-on enabled, then type <kbd>/reload</kbd> or log out. This page updates on its own.</p>
      ${g.errors?.length ? `<p class="note">${g.errors.map(esc).join('<br>')}</p>` : ''}
      ${state.session?.role === 'owner' ? '<button class="btn" data-open-setup>Check the add-on</button>' : ''}</div>`;
    return;
  }
  if (pick === WARBAND) renderWarband(g.warband);
  else renderCharacter(g.characters.find((c) => c.key === pick));
}

// All added games at a glance, instead of switching tabs one at a time.
function renderOverview() {
  const games = state.data.games;
  const suggestions = state.session?.role === 'owner' ? state.data.suggestions || [] : [];
  const alerts = suggestions.map((s) => {
    const version = s.gameVersion ? ` ${esc(s.gameVersion.split('.').slice(0, 3).join('.'))}` : '';
    const text = {
      game: `<b>New characters found</b> in ${esc(s.name)}${s.lastSaved ? `, saved ${ago(s.lastSaved)}` : ''}.`,
      addon: `<b>${esc(s.name)}</b> doesn’t have the Rosterbook add-on yet.`,
      'addon-update': `<b>${esc(s.name)}</b> was updated${version ? ` to${version}` : ''}. Update the add-on so the game doesn’t switch it off.`,
    }[s.kind];
    const action = s.kind === 'game' ? 'add' : 'install';
    const label = s.kind === 'game' ? 'Show them' : s.kind === 'addon' ? 'Install add-on' : 'Update add-on';
    return text ? `<div class="suggest" role="status">
      <p>${text}</p>
      <div class="suggest-actions">
        <button class="btn small primary" data-suggest="${action}" data-id="${esc(s.id)}">${label}</button>
        <button class="btn small" data-suggest="dismiss" data-id="${esc(s.id)}">Ignore</button>
      </div>
    </div>` : '';
  }).join('');

  const cards = games.map((g, i) => `
    <button class="overview-card" data-game="${i}">
      <h3>${esc(g.name)}</h3>
      <div class="account-gold">${coins(g.totals.money + (g.warband?.money || 0))}</div>
      <div class="account-meta">${plural(g.totals.characters, 'character')}, ${duration(g.totals.played)} played</div>
    </button>`).join('');

  detail.innerHTML = `
    <div class="overview">
      ${alerts ? `<div class="group">${alerts}</div>` : ''}
      <div class="overview-grid">${cards}</div>
    </div>`;
}

function renderUnpaired() {
  const failed = new URLSearchParams(location.search).get('pair') === 'failed';
  roster.innerHTML = '';
  $('#layout').style.gridTemplateColumns = '1fr';
  detail.innerHTML = `<div class="empty">
    <h2>Connect this device</h2>
    ${failed ? '<p><b>That code didn’t work.</b> Codes expire after 5 minutes and work once. Make a new one on your PC.</p>' : ''}
    <p>On your PC, open Rosterbook and choose <b>Connect a device</b>. Then scan the code with this device’s camera.</p>
  </div>`;
}

// ---------- settings ----------
let settingsTab = 'games';
let setupCache = null;

function openSheet(title, html) {
  $('#sheet-title').textContent = title;
  sheetBody.innerHTML = html;
  if (!sheet.open) {
    sheet.showModal();
    $('#sheet-title').focus();
  }
}
$('#sheet-close').addEventListener('click', () => sheet.close());
sheet.addEventListener('close', () => { pairing = null; clearInterval(pairTimer); clearInterval(queueTimer); clearInterval(realmStatusTimer); });

function settingsTabs() {
  const owner = state.session?.role === 'owner';
  return (owner ? [['games', 'Games'], ['devices', 'Devices'], ['appearance', 'Appearance'], ['about', 'About']]
    : [['appearance', 'Appearance'], ['about', 'About']]);
}

async function openSettings(tab) {
  const tabs = settingsTabs();
  settingsTab = tabs.some(([k]) => k === tab) ? tab : tabs[0][0];
  openSheet('Settings', '<p class="note">Loading…</p>');
  await renderSettings();
}

async function renderSettings() {
  const tabs = settingsTabs();
  let body = '';
  try {
    if (settingsTab === 'games') body = await gamesTab();
    else if (settingsTab === 'devices') body = await devicesTab();
    else if (settingsTab === 'appearance') body = appearanceTab();
    else body = aboutTab();
  } catch (e) {
    body = `<p class="note">${esc(e.message)}</p>`;
  }
  sheetBody.innerHTML = `<div class="tabs" role="tablist">${tabs.map(([k, l]) =>
    `<button class="tab" role="tab" data-stab="${k}" aria-selected="${settingsTab === k}">${l}</button>`).join('')}</div>${body}`;
}

function defaultGameName(flavor, account) {
  return flavor.accounts.length > 1 ? `${flavor.name} (${account.name})` : flavor.name;
}

async function gamesTab() {
  setupCache = await api('GET', '/api/setup');
  const chosen = new Map(setupCache.games.map((g) => [g.id, g]));
  const first = !setupCache.games.length;
  const installs = setupCache.installs.map((inst, ii) => `
    <div class="card">
      <div class="card-head"><h4>World of Warcraft</h4><span class="path">${esc(inst.root)}</span></div>
      ${inst.flavors.map((f, fi) => {
        const addonTag = f.addonVersion && !f.addonOutdated
          ? `<span class="tag ok">Add-on ${esc(f.addonVersion)} installed</span>`
          : f.addonVersion
            ? `<button class="btn small" data-install="${ii}:${fi}">Update add-on${f.addonVersion === setupCache.bundledAddon ? ' for this game version' : ` to ${esc(setupCache.bundledAddon)}`}</button>`
            : `<button class="btn small primary" data-install="${ii}:${fi}">Install add-on</button>`;
        return `<div class="acct" style="grid-template-columns:1fr">
          <div class="card-head"><b>${esc(f.name)}</b>${addonTag}</div>
          ${f.accounts.length ? f.accounts.map((a) => {
            const on = chosen.has(a.gameId) || (first && a.hasData);
            const name = chosen.get(a.gameId)?.name || defaultGameName(f, a);
            return `<div class="acct" data-game-id="${esc(a.gameId)}" data-sv="${esc(a.savedVariablesDir)}" data-flavor="${esc(f.path)}">
              <input type="checkbox" ${on ? 'checked' : ''} aria-label="Show ${esc(name)}">
              <div class="name-row"><input type="text" value="${esc(name)}" aria-label="Name shown in Rosterbook"><span class="hint">Account ${esc(a.name)}</span></div>
              <span class="meta">${a.hasData ? `Data saved ${ago(a.lastSaved)}` : 'No Rosterbook data yet. Install the add-on, then log in to a character.'}</span>
            </div>`;
          }).join('') : '<p class="hint">No accounts yet. Log in to this game once.</p>'}
        </div>`;
      }).join('')}
    </div>`).join('');

  return `
    ${first ? '<p class="note">Choose which games to show. Rosterbook found these on this PC.</p>' : ''}
    ${installs || '<p class="note">No World of Warcraft installs found. Add the folder that contains your WoW install below.</p>'}
    ${pictureSettings()}
    <div class="group">
      <h3>Game installed somewhere else?</h3>
      <p class="note">Add the folder that contains <b>World of Warcraft</b>, or a Wine/Proton prefix.</p>
      <div class="row-flex"><input type="text" id="extra-path" placeholder="/path/to/World of Warcraft"><button class="btn" data-add-path>Search this folder</button></div>
    </div>
    <div class="sticky-foot"><span class="hint">Install or update the add-on with WoW closed, or restart WoW after.</span><button class="btn primary" data-save-games>Save</button></div>`;
}

function pictureSettings() {
  const on = setupCache.gameIcons;
  const status = setupCache.games.map((g) => {
    const st = setupCache.iconStatus?.[g.id] || {};
    const text = st.error ? esc(st.error)
      : st.working ? 'Reading pictures…'
      : st.version ? `${fmt(st.count)} picture${st.count === 1 ? '' : 's'} from version ${esc(st.version)}`
      : 'Waiting for character data';
    return `<li><b>${esc(g.name)}</b>: ${text}</li>`;
  }).join('');
  return `<div class="group">
    <h3>Item pictures</h3>
    <label class="switch"><input type="checkbox" data-game-icons ${on ? 'checked' : ''}> Show item pictures from my WoW install</label>
    <p class="note">Reads the icons for your characters' items from the game's files on this PC. Nothing is downloaded, and the pictures are only shown to you and your connected devices. When off, Rosterbook uses its own drawings.</p>
    ${on && status ? `<ul class="pic-status">${status}</ul>` : ''}
  </div>`;
}

async function devicesTab() {
  const [devices, setup] = await Promise.all([api('GET', '/api/devices'), setupCache ? Promise.resolve(setupCache) : api('GET', '/api/setup')]);
  setupCache = setup;
  return `
    <div class="group">
      <label class="switch"><input type="checkbox" data-lan ${setup.lan ? 'checked' : ''}> Let devices on my home network connect</label>
      <p class="note">Devices must be connected once with a code from this PC. Turn this off to block all of them.</p>
    </div>
    <div class="group">
      <h3>Connected devices</h3>
      ${devices.length ? devices.map((d) => `<div class="device" data-device="${esc(d.id)}">
        <input type="text" value="${esc(d.name)}" aria-label="Device name">
        <span class="meta">Last seen ${ago(d.lastSeen)}</span>
        <button class="btn small danger" data-remove-device="${esc(d.id)}">Remove</button>
      </div>`).join('') : '<p class="note">None yet.</p>'}
      <p style="margin-top:12px"><button class="btn primary" data-connect>Connect a device</button></p>
    </div>`;
}

function appearanceTab() {
  const t = currentTheme();
  return `<div class="group">
    <h3>Theme</h3>
    <p class="note">Applies to every device connected to this PC.</p>
    <div class="seg" role="group" aria-label="Theme">
      ${[['system', 'Match system'], ['light', 'Light'], ['dark', 'Dark']].map(([k, l]) => `<button data-theme-set="${k}" aria-pressed="${t === k}">${l}</button>`).join('')}
    </div>
  </div>`;
}

function aboutTab() {
  const s = state.session || {};
  return `<div class="group">
    <h3>Rosterbook ${esc(s.version || '')}</h3>
    <p class="note">Add-on bundled with this version: ${esc(s.addonVersion || 'unknown')}.</p>
    <p class="note">Everything stays on this PC and your home network. Rosterbook has no accounts and sends nothing to the internet.</p>
    ${setupCache?.settingsPath && s.role === 'owner' ? `<p class="note">Settings file: ${esc(setupCache.settingsPath)}</p>` : ''}
  </div>
  <div class="group">
    <p class="note">Rosterbook is not affiliated with or endorsed by Blizzard Entertainment. World of Warcraft is a trademark of Blizzard Entertainment, Inc.</p>
  </div>`;
}

// ---------- login queue ----------
let queueTimer = null;

function openQueue() {
  openSheet('Login queue', '<p class="note">Loading…</p>');
  renderQueue();
  clearInterval(queueTimer);
  queueTimer = setInterval(() => { if (sheet.open && $('#sheet-title').textContent === 'Login queue') renderQueue(); }, 5000);
}

async function renderQueue() {
  let q;
  try {
    q = await api('GET', '/api/queue');
  } catch (e) {
    sheetBody.innerHTML = `<p class="note">${esc(e.message)}</p>`;
    return;
  }
  if (!q.status) {
    sheetBody.innerHTML = '<p class="note">Login queue tracking isn’t set up on this PC yet.</p>';
    return;
  }
  const latest = q.samples[q.samples.length - 1];
  const cadenceTag = q.status.error
    ? `<span class="tag bad">${esc(q.status.error)}</span>`
    : `<span class="tag ok">${q.status.cadence === 'active' ? 'Watching' : 'Idle'}</span>`;
  const now = latest?.parseOk
    ? `<div class="queue-now"><b>#${fmt(latest.position)}</b><span class="hint">in the queue</span></div>${latest.displayedTimeText ? `<p class="note">“${esc(latest.displayedTimeText)}”</p>` : ''}`
    : '<p class="note">No position read yet.</p>';
  const recent = q.samples.slice(-10).reverse()
    .map((s) => `<li>${ago(s.ts)}: ${s.parseOk ? `#${fmt(s.position)}` : '—'}</li>`).join('');
  sheetBody.innerHTML = `
    <div class="group">
      <div class="card-head"><h4>Right now</h4>${cadenceTag}</div>
      ${now}
      <p class="hint">Last checked ${ago(q.status.updatedAt)}</p>
    </div>
    <div class="group">
      <h3>Recent</h3>
      ${recent ? `<ul class="pic-status">${recent}</ul>` : '<p class="note">No samples yet.</p>'}
    </div>`;
}

// ---------- server status ----------
let realmStatusTimer = null;

function openRealmStatus() {
  openSheet('Server status', '<p class="note">Loading…</p>');
  renderRealmStatus();
  clearInterval(realmStatusTimer);
  realmStatusTimer = setInterval(() => { if (sheet.open && $('#sheet-title').textContent === 'Server status') renderRealmStatus(); }, 30000);
}

async function renderRealmStatus() {
  let regions;
  try {
    regions = await api('GET', '/api/realm-status');
  } catch (e) {
    sheetBody.innerHTML = `<p class="note">${esc(e.message)}</p>`;
    return;
  }
  if (!regions.length) {
    sheetBody.innerHTML = '<p class="note">Checking…</p>';
    return;
  }
  const rows = regions.map((r) => `
    <li>
      <span class="tag ${r.up ? 'ok' : 'bad'}">${r.up ? 'Up' : 'Down'}</span>
      <b>${esc(r.region)}</b>
      <span class="hint">${r.up ? `${fmt(r.latencyMs)} ms` : 'unreachable'} · checked ${ago(r.checkedAt)}</span>
    </li>`).join('');
  sheetBody.innerHTML = `
    <div class="group">
      <h3>Login servers</h3>
      <p class="hint">One server per region covers retail, classic and classic era alike — this can't tell them apart, just whether the region is reachable.</p>
      <ul class="pic-status">${rows}</ul>
    </div>`;
}

sheetBody.addEventListener('click', async (e) => {
  const t = e.target.closest('button');
  if (!t) return;
  try {
    if (t.dataset.stab) {
      settingsTab = t.dataset.stab;
      await renderSettings();
    } else if (t.dataset.install) {
      const [ii, fi] = t.dataset.install.split(':').map(Number);
      const flavor = setupCache.installs[ii].flavors[fi];
      t.disabled = true;
      const r = await api('POST', '/api/setup/install-addon', { flavorDir: flavor.path });
      toast(`Add-on ${r.installed} installed in ${flavor.name}`);
      await renderSettings();
    } else if (t.hasAttribute('data-add-path')) {
      const path = $('#extra-path').value.trim();
      if (!path) return;
      await api('POST', '/api/setup/install-path', { path });
      await renderSettings();
    } else if (t.hasAttribute('data-save-games')) {
      const games = $$('.acct[data-game-id]', sheetBody)
        .filter((row) => $('input[type="checkbox"]', row).checked)
        .map((row) => ({ id: row.dataset.gameId, name: $('input[type="text"]', row).value.trim(), savedVariablesDir: row.dataset.sv, flavorDir: row.dataset.flavor }));
      await api('PUT', '/api/setup/games', games);
      state.session.configured = games.length > 0;
      toast(games.length ? 'Saved' : 'Saved. No games selected.');
      sheet.close();
    } else if (t.dataset.removeDevice) {
      await api('DELETE', `/api/devices/${encodeURIComponent(t.dataset.removeDevice)}`);
      toast('Device removed');
      await renderSettings();
    } else if (t.hasAttribute('data-connect')) {
      openConnect();
    } else if (t.dataset.themeSet) {
      await api('PUT', '/api/theme', { theme: t.dataset.themeSet });
      if (state.data) state.data.theme = t.dataset.themeSet;
      applyTheme(t.dataset.themeSet);
      await renderSettings();
    } else if (t.dataset.altUrl !== undefined) {
      const u = pairing.urls[Number(t.dataset.altUrl)];
      $('#qr-main').innerHTML = u.qr;
      $('#qr-url').textContent = u.base;
    } else if (t.hasAttribute('data-new-code')) {
      openConnect();
    }
  } catch (err) {
    toast(err.message);
    t.disabled = false;
  }
});

sheetBody.addEventListener('change', async (e) => {
  try {
    if (e.target.matches('[data-game-icons]')) {
      await api('PUT', '/api/setup/icons', { enabled: e.target.checked });
      toast(e.target.checked ? 'Item pictures on. They appear as soon as they’re read.' : 'Item pictures off');
      setTimeout(() => { if (sheet.open && settingsTab === 'games') renderSettings(); }, 1500);
    } else if (e.target.matches('[data-lan]')) {
      await api('PUT', '/api/setup/lan', { lan: e.target.checked });
      setupCache.lan = e.target.checked;
      toast(e.target.checked ? 'Network access on' : 'Network access off');
    } else if (e.target.closest('[data-device]') && e.target.matches('input[type="text"]')) {
      await api('PUT', `/api/devices/${encodeURIComponent(e.target.closest('[data-device]').dataset.device)}`, { name: e.target.value });
      toast('Renamed');
    }
  } catch (err) {
    toast(err.message);
  }
});

// ---------- connect a device ----------
let pairing = null;
let pairTimer = null;
let devicesBefore = 0;

async function openConnect() {
  try {
    devicesBefore = (await api('GET', '/api/devices')).length;
    pairing = await api('POST', '/api/pair');
  } catch (e) {
    return toast(e.message);
  }
  const best = pairing.urls[0];
  if (!pairing.lan) {
    return openSheet('Connect a device', `<p class="note">Network access is turned off. Turn on <b>Let devices on my home network connect</b> in Settings, Devices.</p>`);
  }
  if (!best) {
    return openSheet('Connect a device', '<p class="note">This PC doesn’t seem to be on a network right now.</p>');
  }
  const expires = Date.now() + pairing.expiresIn * 1000;
  openSheet('Connect a device', `
    <p class="note">Scan this with your tablet or phone camera. It needs to be on the same Wi-Fi as this PC.</p>
    <div class="qr" id="qr-main">${best.qr}</div>
    <p class="pair-meta num" id="qr-url">${esc(best.base)}</p>
    <p class="pair-code num" aria-label="Code">${esc(pairing.code)}</p>
    <p class="pair-meta" id="pair-left">Works once. Expires in 5:00.</p>
    ${pairing.urls.length > 1 ? `<details class="more"><summary>Didn’t open? Try another address</summary>
      <p class="note">This PC has ${pairing.urls.length} network adapters. Virtual ones (VMware, Docker, VPNs) usually aren’t reachable.</p>
      <div class="chips">${pairing.urls.map((u, i) => `<button class="chip" data-alt-url="${i}">${esc(u.base.replace('http://', ''))} <span class="ct">${esc(u.adapter)}</span></button>`).join('')}</div>
    </details>` : ''}
    <details class="more"><summary>Still can’t connect?</summary>
      <p class="note">Your firewall is probably blocking port ${pairing.port}. Allow it, then scan again:</p>
      <pre class="cmd">sudo ufw allow ${pairing.port}/tcp</pre>
      <pre class="cmd">sudo firewall-cmd --add-port=${pairing.port}/tcp --permanent && sudo firewall-cmd --reload</pre>
      <p class="note">On Windows, allow Rosterbook when Windows Firewall asks, on private networks. Guest Wi-Fi networks usually can’t see other devices.</p>
    </details>`);
  clearInterval(pairTimer);
  pairTimer = setInterval(() => {
    const left = Math.max(0, Math.round((expires - Date.now()) / 1000));
    const el = $('#pair-left');
    if (!el) return clearInterval(pairTimer);
    if (left === 0) {
      clearInterval(pairTimer);
      el.innerHTML = 'This code has expired. <button class="btn small" data-new-code>New code</button>';
    } else {
      el.textContent = `Works once. Expires in ${Math.floor(left / 60)}:${String(left % 60).padStart(2, '0')}.`;
    }
  }, 1000);
}

async function checkPaired() {
  if (!pairing || !sheet.open) return;
  try {
    const devices = await api('GET', '/api/devices');
    if (devices.length > devicesBefore) {
      clearInterval(pairTimer);
      pairing = null;
      const d = devices[devices.length - 1];
      openSheet('Connect a device', `<div class="empty" style="margin:2vh auto"><h2>Connected</h2><p>${esc(d.name)} can now see your characters. You can rename or remove it in Settings, Devices.</p></div>`);
    }
  } catch { /* ignore */ }
}

$('#connect-btn').addEventListener('click', openConnect);
$('#settings-btn').addEventListener('click', () => openSettings(state.session?.role === 'owner' ? 'games' : 'appearance'));
$('#queue-btn').addEventListener('click', openQueue);
$('#realm-status-btn').addEventListener('click', openRealmStatus);

// ---------- data + live updates ----------
const statusEl = $('#status');
const statusText = $('#status-text');
function setStatus(kind, text) {
  statusEl.className = `status ${kind}`;
  statusText.textContent = text;
}
function updatedLabel() {
  const g = state.data && cur();
  return g?.generatedAt ? `Updated ${ago(g.generatedAt)}` : 'Connected';
}

async function fetchSession() {
  state.session = await api('GET', '/api/session');
  const owner = state.session.role === 'owner';
  $('#connect-btn').hidden = !owner;
  $('#settings-btn').hidden = state.session.role === 'unpaired';
  $('#queue-btn').hidden = state.session.role === 'unpaired';
  $('#realm-status-btn').hidden = state.session.role === 'unpaired';
  $('.search').hidden = state.session.role === 'unpaired';
}

async function fetchData() {
  try {
    state.data = await api('GET', '/api/data');
    for (const g of state.data.games) g.iconSet = new Set(g.icons || []);
  } catch (e) {
    if (e.status === 401) { // this device was removed
      await fetchSession();
      state.data = null;
    } else {
      throw e;
    }
  }
  render();
  checkPaired();
}

let es;
function connect() {
  es?.close();
  es = new EventSource('/api/events');
  es.addEventListener('hello', () => fetchData().then(() => setStatus('live', updatedLabel())).catch(() => {}));
  es.addEventListener('update', () => fetchData().then(() => setStatus('live', updatedLabel())).catch(() => {}));
  es.onerror = () => setStatus('down', 'PC offline, retrying');
}

setInterval(() => { if (statusEl.classList.contains('live')) statusText.textContent = updatedLabel(); }, 30000);

async function init() {
  try {
    await fetchSession();
  } catch {
    setStatus('down', 'Can’t reach your PC');
    return;
  }
  applyTheme(state.session.theme);
  const params = new URLSearchParams(location.search);
  if (state.session.role === 'unpaired') {
    setStatus('down', 'Not connected');
    return render();
  }
  if (params.get('paired')) {
    toast('This device is connected');
    history.replaceState(null, '', '/');
  }
  await fetchData().catch(() => setStatus('down', 'Can’t reach your PC'));
  connect();
  if (state.session.role === 'owner' && !state.session.configured) openSettings('games');
  else if (state.session.role === 'owner' && params.get('open') === 'connect') {
    // The desktop app's tray menu opens the window this way.
    history.replaceState(null, '', '/');
    openConnect();
  }
}

init();
