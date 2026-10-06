// Deck explorer (#decks): every deck that ships with the game, by format, from assets/decks.json
// (tools/build-website-decks.py). The data is fetched when the section comes near the screen, so
// the rest of the page never waits for it. Card images are Scryfall's, linked by printing id.
(function () {
  'use strict';
  var root = document.getElementById('dx');
  if (!root) return;

  // Card table columns, as written by the generator (data.cardFields).
  var NAME = 0, COST = 1, MV = 2, CAT = 3, COLORS = 4, RARITY = 5, IMG = 6, TYPE = 7;
  var CAT_PLURAL = ['Creatures', 'Planeswalkers', 'Battles', 'Instants', 'Sorceries', 'Artifacts', 'Enchantments', 'Lands'];
  var LAND = 7;
  var WUBRG = 'WUBRG';
  var COLOR_NAME = { W: 'White', U: 'Blue', B: 'Black', R: 'Red', G: 'Green', C: 'Colourless' };
  var PIP_BG = { W: '#f8f2d6', U: '#4aa3df', B: '#4a403b', R: '#e8664a', G: '#3fa06a', C: '#cac5c0' };
  var RARITY_RANK = { m: 3, r: 2, u: 1, c: 0 };
  var BASICS = /^(Snow-Covered )?(Plains|Island|Swamp|Mountain|Forest|Wastes)$/;
  var PAGE = { tiles: 24, table: 60 };
  var SRC_LABEL = { meta: 'Archetype', event: 'Tournament', starter: 'Starter' };

  var tabs = root.querySelector('.dx-formats');
  var q = document.getElementById('dx-q');
  var srcSeg = document.getElementById('dx-src');
  var sortSel = document.getElementById('dx-sort');
  var viewSeg = document.getElementById('dx-view');
  var blurb = document.getElementById('dx-blurb');
  var results = document.getElementById('dx-results');
  var moreBtn = document.getElementById('dx-more');
  var hoverless = window.matchMedia('(hover: none)');

  var data = null, cards = null, loading = null;
  var state = { format: null, q: '', colors: '', src: 'all', sort: 'pop', view: 'tiles', shown: 0 };
  var filtered = [];
  try {
    var saved = JSON.parse(localStorage.getItem('dx') || '{}');
    if (saved.view === 'tiles' || saved.view === 'table') state.view = saved.view;
    if (typeof saved.format === 'string') state.format = saved.format;
  } catch (e) {}
  function remember() {
    try { localStorage.setItem('dx', JSON.stringify({ view: state.view, format: state.format })); } catch (e) {}
  }

  // --- helpers -----------------------------------------------------------------------------
  function esc(s) {
    return String(s).replace(/[&<>"']/g, function (c) { return '&#' + c.charCodeAt(0) + ';'; });
  }
  function img(id, size) {
    return id ? 'https://cards.scryfall.io/' + size + '/front/' + id[0] + '/' + id[1] + '/' + id + '.jpg' : '';
  }
  function pip(c) { return '<i class="pip pip-' + c + '" title="' + COLOR_NAME[c] + '">' + c + '</i>'; }
  function pips(colors) {
    return '<span class="pips">' + (colors || 'C').split('').map(pip).join('') + '</span>';
  }
  function cost(text) {
    if (!text) return '';
    return '<span class="cost" aria-label="' + esc(text) + '">' + text.split(' // ').map(function (half) {
      return (half.match(/\{[^}]+\}/g) || []).map(function (t) {
        var s = t.slice(1, -1);
        if (s.length === 1 && WUBRG.indexOf(s) >= 0) return '<i class="pip pip-' + s + '">' + s + '</i>';
        var parts = s.split('/');
        var cols = parts.filter(function (p) { return WUBRG.indexOf(p) >= 0; });
        if (cols.length === 2) {
          return '<i class="pip pip-h" style="background:linear-gradient(135deg,' + PIP_BG[cols[0]] + ' 50%,' + PIP_BG[cols[1]] + ' 50%)"></i>';
        }
        if (cols.length === 1) return '<i class="pip pip-' + cols[0] + ' pip-h">' + (parts[1] === 'P' ? 'φ' : parts[0]) + '</i>';
        return '<i class="pip">' + esc(s) + '</i>';
      }).join('');
    }).join('<span aria-hidden="true">&nbsp;//&nbsp;</span>') + '</span>';
  }
  function ordinal(n) {
    var v = n % 100;
    return n + (v >= 11 && v <= 13 ? 'th' : ['th', 'st', 'nd', 'rd'][n % 10] || 'th');
  }
  function niceDate(iso) {
    var m = /^(\d{4})-(\d{2})-(\d{2})$/.exec(iso || '');
    if (!m) return iso || '';
    return new Date(+m[1], m[2] - 1, +m[3]).toLocaleDateString('en-GB', { day: 'numeric', month: 'short', year: 'numeric' });
  }
  // Challenges rank the field ("#11"); Leagues give a record ("5-0").
  function placeLabel(p) {
    var m = /^#?(\d+)$/.exec(p || '');
    return m ? ordinal(+m[1]) + ' place' : p || '';
  }
  function placeNum(d) {
    var m = /^#?(\d+)$/.exec(d.place || '');
    return m ? +m[1] : 99;
  }
  function plural(n, one, many) { return n + ' ' + (n === 1 ? one : many || one + 's'); }

  // Everything the list needs about a deck, worked out once per deck.
  function digest(d) {
    if (d._) return d._;
    var main = d.main || [], all = (d.cmd || []).concat(main);
    var colors = {}, curve = [0, 0, 0, 0, 0, 0, 0, 0], total = 0, lands = 0, creatures = 0, mvSum = 0, spells = 0;
    var names = [d.name.toLowerCase()];
    all.concat(d.side || []).forEach(function (e) { names.push(cards[e[0]][NAME].toLowerCase()); });
    all.forEach(function (e) {
      var c = cards[e[0]], n = e[1];
      if (c[CAT] === LAND) return;
      c[COLORS].split('').forEach(function (k) { colors[k] = 1; });
    });
    main.forEach(function (e) {
      var c = cards[e[0]], n = e[1];
      total += n;
      if (c[CAT] === LAND) { lands += n; return; }
      if (c[CAT] === 0) creatures += n;
      spells += n;
      mvSum += c[MV] * n;
      curve[Math.min(7, c[MV])] += n;
    });
    // The tile shows the card the deck is built around: its commander, else its most-played
    // nonland card, the rarest and most expensive of those.
    var key = (d.cmd && d.cmd[0]) || all.filter(function (e) { return cards[e[0]][CAT] !== LAND; }).sort(function (a, b) {
      var ca = cards[a[0]], cb = cards[b[0]];
      return b[1] - a[1] || RARITY_RANK[cb[RARITY]] - RARITY_RANK[ca[RARITY]] || cb[MV] - ca[MV];
    })[0] || all[0];
    var keys = main.filter(function (e) { return cards[e[0]][CAT] !== LAND; }).sort(function (a, b) {
      return b[1] - a[1] || RARITY_RANK[cards[b[0]][RARITY]] - RARITY_RANK[cards[a[0]][RARITY]] || cards[b[0]][MV] - cards[a[0]][MV];
    }).slice(0, 3).map(function (e) { return cards[e[0]][NAME]; });
    d._ = {
      colors: WUBRG.split('').filter(function (k) { return colors[k]; }).join(''),
      curve: curve, total: total + (d.cmd ? d.cmd.length : 0), lands: lands, creatures: creatures,
      others: spells - creatures, avg: spells ? mvSum / spells : 0,
      key: key ? key[0] : -1, keys: keys, text: names.join('\n')
    };
    return d._;
  }
  function sub(d) {
    if (d.src === 'meta') return 'Built from ' + plural(d.n || 0, 'tournament deck');
    if (d.src === 'event') return [d.event, niceDate(d.date), placeLabel(d.place)].filter(Boolean).join(' · ');
    return d.note || '';
  }
  function badge(d, fmt) {
    if (d.src === 'starter') return fmt.id === 'commander' ? 'Commander' : 'Starter';
    return SRC_LABEL[d.src];
  }
  function fmtById(id) {
    for (var i = 0; i < data.formats.length; i++) if (data.formats[i].id === id) return data.formats[i];
    return null;
  }

  // --- the list --------------------------------------------------------------------------
  function buildTabs() {
    tabs.innerHTML = data.formats.map(function (f) {
      return '<button type="button" role="tab" class="dx-tab" data-f="' + f.id + '" aria-selected="false" tabindex="-1">' +
        esc(f.name) + ' <small>' + f.decks.length + '</small></button>';
    }).join('');
    tabs.addEventListener('click', function (e) {
      var b = e.target.closest('.dx-tab');
      if (b) selectFormat(b.dataset.f, true);
    });
    tabs.addEventListener('keydown', function (e) {
      if (e.key !== 'ArrowLeft' && e.key !== 'ArrowRight') return;
      var all = [].slice.call(tabs.children), i = all.indexOf(document.activeElement);
      if (i < 0) return;
      var next = all[(i + (e.key === 'ArrowRight' ? 1 : -1) + all.length) % all.length];
      next.focus();
      selectFormat(next.dataset.f, true);
    });
  }

  function selectFormat(id, user) {
    var f = fmtById(id) || data.formats[0];
    state.format = f.id;
    [].forEach.call(tabs.children, function (b) {
      var on = b.dataset.f === f.id;
      b.setAttribute('aria-selected', String(on));
      b.tabIndex = on ? 0 : -1;
      if (on && user) b.scrollIntoView({ block: 'nearest', inline: 'nearest' });
    });
    var srcs = {};
    f.decks.forEach(function (d) { srcs[d.src] = 1; });
    srcSeg.hidden = !(srcs.meta && srcs.event);
    if (srcSeg.hidden) state.src = 'all';
    setPressed(srcSeg, state.src);
    sortSel.querySelector('[value="new"]').hidden = !srcs.event;
    if (!srcs.event && state.sort === 'new') sortSel.value = state.sort = 'pop';
    remember();
    refresh();
  }

  function setPressed(seg, v) {
    [].forEach.call(seg.querySelectorAll('button'), function (b) { b.setAttribute('aria-pressed', String(b.dataset.v === v)); });
  }

  function sortKey(a, b) {
    var da = digest(a), db = digest(b);
    switch (state.sort) {
      case 'name': return a.name.localeCompare(b.name);
      case 'mv': return da.avg - db.avg || a.name.localeCompare(b.name);
      case 'colors':
        return da.colors.length - db.colors.length || order(da.colors) - order(db.colors) || a.name.localeCompare(b.name);
      case 'new':
        return (b.date || '').localeCompare(a.date || '') || placeNum(a) - placeNum(b) || (b.n || 0) - (a.n || 0);
      default: {
        var rank = { meta: 0, event: 1, starter: 2 };
        return rank[a.src] - rank[b.src] || (b.n || 0) - (a.n || 0) ||
          (b.date || '').localeCompare(a.date || '') || placeNum(a) - placeNum(b) || a.name.localeCompare(b.name);
      }
    }
  }
  function order(colors) {
    var v = 0;
    for (var i = 0; i < colors.length; i++) v = v * 6 + WUBRG.indexOf(colors[i]) + 1;
    return v;
  }

  function refresh() {
    var f = fmtById(state.format);
    var words = state.q.toLowerCase().split(/\s+/).filter(Boolean);
    filtered = f.decks.filter(function (d) {
      var g = digest(d);
      if (state.src !== 'all' && d.src !== state.src) return false;
      for (var i = 0; i < state.colors.length; i++) {
        var c = state.colors[i];
        if (c === 'C' ? g.colors !== '' : g.colors.indexOf(c) < 0) return false;
      }
      for (var j = 0; j < words.length; j++) if (g.text.indexOf(words[j]) < 0) return false;
      return true;
    }).sort(sortKey);
    var count = filtered.length === f.decks.length ? plural(f.decks.length, 'deck')
      : '<b>' + filtered.length + '</b> of ' + plural(f.decks.length, 'deck');
    blurb.innerHTML = esc(f.blurb) + ' · ' + count;
    state.shown = 0;
    results.innerHTML = '';
    if (!filtered.length) {
      results.innerHTML = '<p class="dx-empty">No ' + esc(f.name) + ' deck matches. Try fewer colours or another word.</p>';
      moreBtn.hidden = true;
      return;
    }
    more();
  }

  // When the search names a card rather than a deck, the tile says how many copies it plays.
  function hit(d) {
    var w = state.q.trim().toLowerCase();
    if (w.length < 3 || d.name.toLowerCase().indexOf(w) >= 0) return '';
    var lists = (d.cmd || []).concat(d.main || [], d.side || []);
    for (var i = 0; i < lists.length; i++) {
      var c = cards[lists[i][0]];
      if (c[NAME].toLowerCase().indexOf(w) >= 0) {
        var side = d.side && d.side.indexOf(lists[i]) >= 0;
        return '<span class="dx-hit">Plays ' + lists[i][1] + ' ' + esc(c[NAME]) + (side ? ' (sideboard)' : '') + '</span>';
      }
    }
    return '';
  }

  function more() {
    var f = fmtById(state.format);
    var from = state.shown, to = Math.min(filtered.length, from + PAGE[state.view]);
    var slice = filtered.slice(from, to);
    var html;
    if (state.view === 'tiles') {
      html = slice.map(function (d, i) {
        var g = digest(d), max = Math.max.apply(null, g.curve) || 1;
        var key = cards[g.key];
        return '<button type="button" class="dx-tile" data-i="' + (from + i) + '">' +
          '<span class="dx-art"><img src="' + img(key && key[IMG], 'art_crop') + '" alt="" loading="lazy" decoding="async">' +
          pips(g.colors) + '<span class="dx-badge">' + badge(d, f) + '</span></span>' +
          '<span class="dx-info"><b>' + esc(d.name) + '</b><span class="dx-sub">' + esc(sub(d)) + '</span>' + hit(d) +
          '<span class="dx-foot"><span class="dx-counts">' + g.creatures + ' creatures · ' + g.others + ' spells · ' + g.lands + ' lands</span>' +
          '<span class="curve-mini" title="Mana curve, 0 to 7+">' + g.curve.map(function (n) {
            return '<i style="height:' + Math.round(n / max * 100) + '%"></i>';
          }).join('') + '</span></span></span></button>';
      }).join('');
      var grid = results.querySelector('.dx-grid');
      if (!grid) { results.innerHTML = '<div class="dx-grid"></div>'; grid = results.firstChild; }
      grid.insertAdjacentHTML('beforeend', html);
    } else {
      html = slice.map(function (d, i) {
        var g = digest(d), key = cards[g.key], max = Math.max.apply(null, g.curve) || 1;
        return '<tr data-i="' + (from + i) + '">' +
          '<td class="hide-sm"><img class="dx-thumb" src="' + img(key && key[IMG], 'art_crop') + '" alt="" loading="lazy"></td>' +
          '<td class="dx-name"><button type="button">' + esc(d.name) + '</button><div class="dx-sub">' + esc(sub(d)) + '</div>' + hit(d) + '</td>' +
          '<td>' + pips(g.colors) + '</td>' +
          '<td class="num">' + g.avg.toFixed(2) + '</td>' +
          '<td class="hide-sm"><span class="curve-mini">' + g.curve.map(function (n) {
            return '<i style="height:' + Math.round(n / max * 100) + '%"></i>';
          }).join('') + '</span></td>' +
          '<td class="num hide-sm">' + g.creatures + ' / ' + g.others + ' / ' + g.lands + '</td>' +
          '<td class="dx-keys">' + g.keys.map(esc).join(', ') + '</td></tr>';
      }).join('');
      var body = results.querySelector('tbody');
      if (!body) {
        results.innerHTML = '<div class="dx-tablewrap"><table class="dx-table"><thead><tr>' +
          '<th class="hide-sm"><span class="visually-hidden">Art</span></th><th>Deck</th><th>Colours</th><th class="num">Avg MV</th>' +
          '<th class="hide-sm">Curve</th><th class="num hide-sm" title="Creatures / other spells / lands">Cre / Spl / Lnd</th><th>Key cards</th>' +
          '</tr></thead><tbody></tbody></table></div>';
        body = results.querySelector('tbody');
      }
      body.insertAdjacentHTML('beforeend', html);
    }
    state.shown = to;
    moreBtn.hidden = to >= filtered.length;
    moreBtn.textContent = 'Show more (' + (filtered.length - to) + ' left)';
  }

  results.addEventListener('click', function (e) {
    var el = e.target.closest('[data-i]');
    if (el) openDeck(+el.dataset.i, el);
  });
  moreBtn.addEventListener('click', more);

  var typing;
  q.addEventListener('input', function () {
    clearTimeout(typing);
    typing = setTimeout(function () { state.q = q.value; if (data) refresh(); }, 140);
  });
  root.querySelector('.dx-colors').addEventListener('click', function (e) {
    var b = e.target.closest('.pip-btn');
    if (!b) return;
    var on = b.getAttribute('aria-pressed') !== 'true';
    b.setAttribute('aria-pressed', String(on));
    state.colors = [].map.call(root.querySelectorAll('.pip-btn[aria-pressed="true"]'), function (x) { return x.dataset.c; }).join('');
    if (data) refresh();
  });
  srcSeg.addEventListener('click', function (e) {
    var b = e.target.closest('button');
    if (!b) return;
    state.src = b.dataset.v;
    setPressed(srcSeg, state.src);
    if (data) refresh();
  });
  sortSel.addEventListener('change', function () { state.sort = sortSel.value; if (data) refresh(); });
  viewSeg.addEventListener('click', function (e) {
    var b = e.target.closest('button');
    if (!b) return;
    state.view = b.dataset.v;
    setPressed(viewSeg, state.view);
    remember();
    if (data) refresh();
  });
  setPressed(viewSeg, state.view);

  // --- the viewer ------------------------------------------------------------------------
  var dlg = document.getElementById('dx-viewer');
  var pane = document.getElementById('dxv-pane');
  var preview = document.getElementById('dxv-preview');
  var body = dlg.querySelector('.dxv-body');
  var vtabs = document.getElementById('dxv-tabs');
  var pop = document.getElementById('dx-pop');
  var cur = { list: [], i: 0, deck: null, view: 'list', opener: null, library: [], hand: [], mull: 0 };

  function openDeck(i, opener) {
    cur.list = filtered.slice();
    cur.opener = opener || null;
    show(i);
    if (!dlg.open) {
      document.documentElement.classList.add('lb-lock');
      if (dlg.showModal) dlg.showModal(); else dlg.setAttribute('open', '');
    }
  }

  function show(i) {
    var f = fmtById(state.format);
    cur.i = (i + cur.list.length) % cur.list.length;
    var d = cur.deck = cur.list[cur.i], g = digest(d), key = cards[g.key];
    document.getElementById('dxv-format').textContent = badge(d, f) === f.name ? f.name : f.name + ' · ' + badge(d, f);
    document.getElementById('dxv-name').textContent = d.name;
    document.getElementById('dxv-meta').innerHTML = pips(g.colors) + '<span>' + esc(sub(d)) + '</span>' +
      '<span>' + plural(g.total, 'card') + (d.side ? ' + ' + d.side.reduce(function (s, e) { return s + e[1]; }, 0) + ' sideboard' : '') + '</span>';
    document.getElementById('dxv-art').style.backgroundImage = key && key[IMG] ? 'url("' + img(key[IMG], 'art_crop') + '")' : '';
    setPreview(g.key);
    var many = cur.list.length > 1;
    document.getElementById('dxv-prev').hidden = document.getElementById('dxv-next').hidden = !many;
    try { history.replaceState(null, '', '#decks/' + f.id + '/' + d.id); } catch (e) {}
    cur.library = [];
    render();
    pane.scrollTop = 0;
  }

  function setPreview(ci) {
    var c = cards[ci];
    if (!c) return;
    preview.src = img(c[IMG], 'normal');
    preview.alt = c[NAME];
  }

  function zoom(ci) {
    var c = cards[ci];
    if (!c || !c[IMG]) return;
    var im = pop.querySelector('img');
    im.src = img(c[IMG], 'large');
    im.alt = c[NAME];
    pop.hidden = false;
  }
  pop.addEventListener('click', function () { pop.hidden = true; });

  function sections(d) {
    var out = [];
    if (d.cmd) out.push({ title: d.cmd.length > 1 ? 'Commanders' : 'Commander', rows: d.cmd });
    CAT_PLURAL.forEach(function (title, cat) {
      var rows = (d.main || []).filter(function (e) { return cards[e[0]][CAT] === cat; });
      if (rows.length) out.push({ title: title, rows: rows, cat: cat });
    });
    if (d.side) out.push({ title: 'Sideboard', rows: d.side, side: true });
    out.forEach(function (s) {
      s.rows = s.rows.slice().sort(function (a, b) {
        var ca = cards[a[0]], cb = cards[b[0]];
        if (s.cat === LAND) return BASICS.test(ca[NAME]) - BASICS.test(cb[NAME]) || ca[NAME].localeCompare(cb[NAME]);
        return ca[MV] - cb[MV] || ca[NAME].localeCompare(cb[NAME]);
      });
      s.count = s.rows.reduce(function (n, e) { return n + e[1]; }, 0);
    });
    return out;
  }

  function cardButton(e, cls) {
    var c = cards[e[0]];
    return '<button type="button" class="dv-card' + (cls || '') + '" data-c="' + e[0] + '" aria-label="' + esc((e[1] > 1 ? e[1] + ' ' : '') + c[NAME]) + '">' +
      '<img src="' + img(c[IMG], 'normal') + '" alt="" loading="lazy" decoding="async" width="488" height="680">' +
      (e[1] > 1 ? '<span class="qty">' + e[1] + '</span>' : '') + '</button>';
  }

  function render() {
    var d = cur.deck, g = digest(d);
    [].forEach.call(vtabs.querySelectorAll('button'), function (b) { b.setAttribute('aria-selected', String(b.dataset.v === cur.view)); });
    body.classList.toggle('no-preview', cur.view !== 'list');
    if (cur.view === 'list') {
      pane.innerHTML = '<div class="dl-cols">' + sections(d).map(function (s) {
        return '<section class="dl-group' + (s.side ? ' dl-side' : '') + '"><h4>' + s.title + ' <span>' + s.count + '</span></h4>' +
          s.rows.map(function (e) {
            var c = cards[e[0]];
            return '<button type="button" class="dl-row" data-c="' + e[0] + '" title="' + esc(c[TYPE]) + '"><span class="n">' + e[1] + '</span>' +
              '<span class="nm">' + esc(c[NAME]) + '</span>' + cost(c[COST]) + '</button>';
          }).join('') + '</section>';
      }).join('') + '</div>';
    } else if (cur.view === 'visual') {
      pane.innerHTML = '<div class="dv-cols">' + sections(d).map(function (s) {
        return '<div class="dv-col"><h4>' + s.title + ' <span>' + s.count + '</span></h4><div class="dv-stack">' +
          s.rows.map(function (e) { return cardButton(e); }).join('') + '</div></div>';
      }).join('') + '</div>';
    } else if (cur.view === 'stats') {
      pane.innerHTML = stats(d, g);
    } else {
      if (!cur.library.length) newHand(0);
      pane.innerHTML = handView();
    }
  }

  function stats(d, g) {
    var max = Math.max.apply(null, g.curve) || 1;
    var labels = ['0', '1', '2', '3', '4', '5', '6', '7+'];
    var curve = '<div class="curve" role="img" aria-label="Mana curve: ' + g.curve.map(function (n, i) { return n + ' at ' + labels[i]; }).join(', ') + '">' +
      g.curve.map(function (n, i) {
        return '<div class="curve-bar" title="' + plural(n, 'card') + ' with mana value ' + labels[i] + '">' + (n ? '<span>' + n + '</span>' : '') +
          '<i style="height:' + (n / max * 85) + '%"></i></div>';
      }).join('') + '</div><div class="curve-x">' + labels.map(function (l) { return '<span>' + l + '</span>'; }).join('') + '</div>';

    var types = [0, 0, 0, 0, 0, 0, 0, 0];
    (d.main || []).forEach(function (e) { types[cards[e[0]][CAT]] += e[1]; });
    var tmax = Math.max.apply(null, types) || 1;
    var typeRows = types.map(function (n, i) {
      return n ? '<div class="ds-row"><span class="lbl">' + CAT_PLURAL[i] + '</span><span class="track"><span class="fill" style="width:' + (n / tmax * 100) + '%"></span></span><span class="v">' + n + '</span></div>' : '';
    }).join('');

    // Coloured mana symbols in the costs: how hard each colour pulls on the mana base.
    var sym = { W: 0, U: 0, B: 0, R: 0, G: 0 };
    (d.cmd || []).concat(d.main || []).forEach(function (e) {
      (cards[e[0]][COST].match(/\{[^}]+\}/g) || []).forEach(function (t) {
        for (var k in sym) if (t.indexOf(k) >= 0) sym[k] += e[1];
      });
    });
    var smax = Math.max(sym.W, sym.U, sym.B, sym.R, sym.G) || 1;
    var symRows = WUBRG.split('').map(function (k) {
      return sym[k] ? '<div class="ds-row"><span class="lbl">' + pip(k) + COLOR_NAME[k] + '</span><span class="track"><span class="fill" style="width:' + (sym[k] / smax * 100) + '%;background:' + PIP_BG[k] + '"></span></span><span class="v">' + sym[k] + '</span></div>' : '';
    }).join('') || '<p class="ds-note">No coloured mana symbols.</p>';

    var landShare = g.total ? Math.round(g.lands / (d.main || []).reduce(function (s, e) { return s + e[1]; }, 0) * 100) : 0;
    return '<div class="ds">' +
      '<div class="ds-facts">' +
      '<div class="ds-fact"><b>' + g.total + '</b><span>Cards</span></div>' +
      '<div class="ds-fact"><b>' + g.avg.toFixed(2) + '</b><span>Avg mana value</span></div>' +
      '<div class="ds-fact"><b>' + g.lands + '</b><span>Lands · ' + landShare + '%</span></div>' +
      '<div class="ds-fact"><b>' + g.creatures + '</b><span>Creatures</span></div></div>' +
      '<div><h4>Mana curve</h4>' + curve + '<p class="ds-note">Nonland cards in the main deck, by mana value.</p></div>' +
      '<div><h4>Card types</h4><div class="ds-rows">' + typeRows + '</div>' +
      '<h4 style="margin-top:26px">Mana symbols</h4><div class="ds-rows">' + symRows + '</div></div>' +
      '</div>';
  }

  // Sample hand: shuffle the main deck and draw seven, the way a game starts. Mulligans follow
  // the London rule: draw seven again, then put one card per mulligan on the bottom.
  function newHand(mull) {
    var lib = [];
    (cur.deck.main || []).forEach(function (e) { for (var k = 0; k < e[1]; k++) lib.push(e[0]); });
    for (var i = lib.length - 1; i > 0; i--) {
      var j = Math.floor(Math.random() * (i + 1)), t = lib[i]; lib[i] = lib[j]; lib[j] = t;
    }
    cur.library = lib;
    cur.hand = lib.splice(0, 7);
    cur.mull = mull;
  }
  function handView() {
    var lands = cur.hand.filter(function (ci) { return cards[ci][CAT] === LAND; }).length;
    var keep = 7 - cur.mull;
    return '<div class="dh-bar">' +
      '<button type="button" class="pill pill-play" data-act="new">New hand</button>' +
      '<button type="button" class="pill pill-secondary" data-act="mull"' + (keep <= 1 ? ' disabled' : '') + '>Mulligan</button>' +
      '<button type="button" class="pill pill-secondary" data-act="draw"' + (cur.library.length ? '' : ' disabled') + '>Draw a card</button>' +
      '<span>' + plural(lands, 'land') + ' of ' + cur.hand.length + ' · ' + cur.library.length + ' left in library' +
      (cur.mull ? ' · mulligan to ' + keep + ': put ' + plural(cur.mull, 'card') + ' on the bottom' : '') + '</span></div>' +
      '<div class="dh-hand">' + cur.hand.map(function (ci, i) {
        return cardButton([ci, 1]).replace('class="dv-card"', 'class="dv-card" style="animation-delay:' + (i < 7 ? i * 40 : 0) + 'ms"');
      }).join('') + '</div>';
  }

  pane.addEventListener('click', function (e) {
    var act = e.target.closest('[data-act]');
    if (act) {
      if (act.dataset.act === 'new') newHand(0);
      else if (act.dataset.act === 'mull') newHand(cur.mull + 1);
      else if (cur.library.length) cur.hand.push(cur.library.shift());
      pane.innerHTML = handView();
      return;
    }
    var c = e.target.closest('[data-c]');
    if (!c) return;
    // With a preview pane, a click on a row pins it there; otherwise it shows the card big.
    if (c.classList.contains('dl-row') && !hoverless.matches && getComputedStyle(preview.parentNode).display !== 'none') {
      setPreview(+c.dataset.c);
      return;
    }
    zoom(+c.dataset.c);
  });
  function hoverRow(e) {
    var r = e.target.closest && e.target.closest('.dl-row');
    if (r) setPreview(+r.dataset.c);
  }
  pane.addEventListener('mouseover', hoverRow);
  pane.addEventListener('focusin', hoverRow);

  vtabs.addEventListener('click', function (e) {
    var b = e.target.closest('button');
    if (!b) return;
    cur.view = b.dataset.v;
    render();
  });
  document.getElementById('dxv-prev').addEventListener('click', function () { show(cur.i - 1); });
  document.getElementById('dxv-next').addEventListener('click', function () { show(cur.i + 1); });
  document.getElementById('dxv-close').addEventListener('click', function () { closeDeck(); });
  dlg.addEventListener('click', function (e) { if (e.target === dlg) closeDeck(); });
  dlg.addEventListener('keydown', function (e) {
    if (!pop.hidden && e.key === 'Escape') { e.preventDefault(); pop.hidden = true; return; }
    if (e.key === 'ArrowLeft' && cur.list.length > 1) show(cur.i - 1);
    else if (e.key === 'ArrowRight' && cur.list.length > 1) show(cur.i + 1);
  });
  // Esc closes a modal dialog natively; this tidies up however it was closed.
  dlg.addEventListener('close', function () {
    pop.hidden = true;
    document.documentElement.classList.remove('lb-lock');
    try { history.replaceState(null, '', '#decks'); } catch (e) {}
    if (cur.opener && document.contains(cur.opener)) cur.opener.focus({ preventScroll: true });
  });
  function closeDeck() {
    if (dlg.close) dlg.close(); else { dlg.removeAttribute('open'); dlg.dispatchEvent(new Event('close')); }
  }

  document.getElementById('dxv-copy').addEventListener('click', function () {
    var btn = this, label = btn.querySelector('span');
    var lines = function (rows) { return rows.map(function (e) { return e[1] + ' ' + cards[e[0]][NAME]; }).join('\n'); };
    var d = cur.deck, text = '';
    if (d.cmd) text += 'Commander\n' + lines(d.cmd) + '\n\n';
    text += 'Deck\n' + lines(d.main || []);
    if (d.side) text += '\n\nSideboard\n' + lines(d.side);
    var done = function (t) { label.textContent = t; setTimeout(function () { label.textContent = 'Copy list'; }, 1600); };
    if (navigator.clipboard) navigator.clipboard.writeText(text).then(function () { done('Copied'); }, function () { done('Copy failed'); });
    else done('Copy failed');
  });

  // --- loading -----------------------------------------------------------------------------
  function load() {
    if (loading) return loading;
    loading = fetch('assets/decks.json').then(function (r) {
      if (!r.ok) throw new Error(r.status);
      return r.json();
    }).then(function (json) {
      data = json;
      cards = json.cards;
      root.setAttribute('aria-busy', 'false');
      buildTabs();
      selectFormat(state.format);
    }).catch(function () {
      loading = null;
      results.innerHTML = '<p class="dx-empty">The decks could not be loaded. <button type="button" class="pill pill-secondary" id="dx-retry">Try again</button></p>';
      document.getElementById('dx-retry').addEventListener('click', load);
    });
    return loading;
  }

  // A link like #decks/modern/boros-burn opens that deck.
  function fromHash() {
    var m = /^#decks\/([\w-]+)(?:\/([\w-]+))?$/.exec(location.hash);
    if (!m) return;
    load().then(function () {
      if (!data) return;
      if (fmtById(m[1]) && state.format !== m[1]) selectFormat(m[1], true);
      document.getElementById('decks').scrollIntoView();
      if (!m[2]) return;
      var at = -1;
      filtered.forEach(function (d, i) { if (d.id === m[2]) at = i; });
      if (at >= 0) openDeck(at);
    });
  }
  window.addEventListener('hashchange', fromHash);
  if (/^#decks\//.test(location.hash)) fromHash();
  else if ('IntersectionObserver' in window) {
    var io = new IntersectionObserver(function (entries) {
      if (entries.some(function (e) { return e.isIntersecting; })) { io.disconnect(); load(); }
    }, { rootMargin: '800px 0px' });
    io.observe(root);
  } else load();
})();
