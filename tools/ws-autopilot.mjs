// Throwaway protocol exerciser: plays a match over the WebSocket protocol with a "first option / OK" policy.
// usage: node tools/ws-autopilot.mjs [humanDeckRef] [aiDeckRef] [--verbose] [--cards]   (--cards: click cards in hand on priority)
const BASE = process.env.ENGINE || 'http://localhost:8080';
const args = process.argv.slice(2);
const verbose = args.includes('--verbose');
const clickCards = args.includes('--cards');
const pos = args.filter(a => !a.startsWith('--'));
const decks = await (await fetch(`${BASE}/api/decks`)).json();
const pick = (spec, dflt) => spec === 'random' ? decks[Math.floor(Math.random() * decks.length)].ref : spec ? decks.find(d => d.name.toLowerCase().includes(spec.toLowerCase()))?.ref : dflt;
const human = pick(pos[0], decks[0].ref);
const ai = pick(pos[1], decks[1].ref);
const m = await (await fetch(`${BASE}/api/match`, { method: 'POST', headers: { 'content-type': 'application/json' }, body: JSON.stringify({ humanDeck: human, aiDeck: ai, humanName: 'Tester' }) })).json();
console.log('match', m);

const ws = new WebSocket(`${BASE.replace('http', 'ws')}/ws/match/${m.id}`);
const hist = {};
const count = (k) => (hist[k] = (hist[k] || 0) + 1);
let state = null, frames = 0, lastInputKey = '', clicks = 0, t0 = Date.now();
const send = (o) => ws.send(JSON.stringify(o));
const myId = () => state?.you;
const me = () => state?.game?.players?.find(p => p.id === myId());

ws.onmessage = (ev) => {
  const f = JSON.parse(ev.data); frames++;
  if (f.t === 'state') { state = f; if (verbose) console.log(`state seq=${f.seq} turn=${f.game.turn} phase=${f.game.phase} stack=${f.game.stack.length}`); return; }
  if (f.t === 'log') { if (verbose) for (const e of f.entries) console.log('   log:', e.type, e.text); return; }
  if (f.t === 'hello' || f.t === 'pong' || f.t === 'alert') return;
  if (f.t === 'flash') { count('flash'); if (lastInput) setTimeout(() => decide(lastInput, true), 5); return; }
  if (f.t === 'message') { count('message'); console.log('  message:', f.title, f.text); return; }
  if (f.t === 'error') { console.log('  ERROR frame:', f.text); return; }
  if (f.t === 'gameOver') {
    console.log(`\nGAME OVER winner=${f.winner} youWon=${f.youWon} in ${Date.now() - t0}ms, frames=${frames}, clicks=${clicks}`);
    console.log('prompt/input histogram:', hist);
    const p = me(); if (p) console.log('you:', { life: p.life, hand: p.zones.hand.length, bf: p.zones.battlefield.length });
    ws.close(); process.exit(0);
  }
  if (f.t === 'prompt') {
    count('prompt:' + f.kind);
    if (verbose) console.log('  PROMPT', f.kind, JSON.stringify(f).slice(0, 200));
    let value = null;
    switch (f.kind) {
      case 'chooseAbility': value = 0; break;
      case 'confirm': value = f.defaultYes; break;
      case 'option': value = f.default; break;
      case 'text': value = f.initial ?? (f.numeric ? '0' : ''); break;
      case 'choices': case 'chooseEntity': { const n = Math.max(f.min, Math.min(1, f.max)); value = f.options.slice(0, n).map(o => o.i); break; }
      case 'order': { const n = f.options.length; const keep = Math.min(Math.max(f.remainMax, 0), n); value = { order: f.options.slice(keep).map(o => o.i), remember: false }; break; }
      case 'assignCombatDamage': { const v = {}; if (f.blockers.length) v[String(f.blockers[0].id)] = f.damage; else if (f.defender) v.defender = f.damage; value = v; break; } // null = 'assign another attacker first' (would loop)
      case 'assignAmount': value = f.options.length ? { [f.options[0].i]: f.amount } : {}; break;
      case 'manipulateCards': value = f.cards.map(c => c.id); break;
      default: console.log('  unknown prompt kind', f.kind);
    }
    send({ t: 'answer', id: f.id, value });
    return;
  }
  if (f.t === 'input') {
    lastInput = f;
    const key = JSON.stringify([f.seq, f.message, f.ok, f.cancel, f.selectable, f.highlighted, f.selectablePlayers, f.selectMin, f.selectMax]);
    if (key === lastInputKey) return; // unchanged repaint
    lastInputKey = key;
    if (verbose) console.log('  INPUT', f.message.replace(/\n/g, ' ').slice(0, 80), '| ok:', f.ok.label, f.ok.enabled, '| cancel:', f.cancel.label, f.cancel.enabled, '| sel:', f.selectable.length);
    setTimeout(() => decide(f, false), 5);
  }
};
function decide(f, noCards) {
  clicks++;
  const msg = f.message || '';
  if (!noCards && clickCards && f.ok.enabled && f.ok.label === 'OK' && state) {
    const p = me();
    // our main phase: try to play a random card from hand (lands and spells alike; rejected plays flash and we re-decide)
    if (/Main phase/.test(msg) && /\(Tester\)/.test(msg) && p?.zones.hand.length && Math.random() < 0.6) {
      const c = p.zones.hand[Math.floor(Math.random() * p.zones.hand.length)];
      count('click:card-from-hand'); send({ t: 'click', action: 'card', id: c.id }); return;
    }
    // declare attackers: alpha strike half the time
    if (/attack/i.test(msg) && Math.random() < 0.5) { count('click:alphaStrike'); send({ t: 'click', action: 'alphaStrike' }); setTimeout(() => send({ t: 'click', action: 'ok' }), 50); return; }
  }
  if (f.ok.enabled) { count('click:' + f.ok.label); send({ t: 'click', action: 'ok' }); }
  else if (f.selectable.some(id => !f.highlighted.includes(id))) { count('click:selectCard'); send({ t: 'click', action: 'card', id: f.selectable.find(id => !f.highlighted.includes(id)) }); }
  else if (f.selectablePlayers?.length) { count('click:selectPlayer'); send({ t: 'click', action: 'player', id: f.selectablePlayers[0] }); }
  else if (f.cancel.enabled) { count('click:' + f.cancel.label); send({ t: 'click', action: 'cancel' }); }
  else clicks--;
}
ws.onerror = (e) => { console.log('ws error', e.message); process.exit(1); };
let lastInput=null;
setTimeout(() => { console.log('TIMEOUT; hist', hist, 'frames', frames, 'last state turn', state?.game?.turn, '\nlast input:', JSON.stringify(lastInput)); process.exit(2); }, Number(process.env.TIMEOUT_MS||180000));
