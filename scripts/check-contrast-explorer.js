// Run contrast-explorer.html's script against a stub DOM and assert the new
// full-page section actually renders. Catches a syntax error or a thrown
// exception, neither of which is visible from reading the file.
const fs = require('fs');
const html = fs.readFileSync(process.argv[2], 'utf8');
const script = html.slice(html.indexOf('<script>') + 8, html.lastIndexOf('</script>'));

const store = {};
const mk = id => (store[id] = store[id] || {
  id, innerHTML: '', value: '', textContent: '', style: {}, checked: false,
  addEventListener() {}, appendChild() {}, querySelectorAll: () => [],
});
global.document = {
  getElementById: mk,
  querySelectorAll: () => [],
  querySelector: () => null,
  addEventListener() {},
  createElement: () => mk('tmp'),
};
global.window = { addEventListener() {}, matchMedia: () => ({ matches: false, addEventListener() {} }) };

// Exercise every preset, not just the default: op1/op2/op3 carry their own
// surface ladder, and a renderer that only works for the shipped one is the
// bug this harness exists to catch.
const probe = script + `
  globalThis.__probe = [];
  for (const k of Object.keys(PRESETS)) {
    cur = withLadder(PRESETS[k]);
    render();
    const h = document.getElementById('fp').innerHTML;
    globalThis.__probe.push([k, h.length, h.includes('$' + '{'), h.includes('undefined'), h]);
  }

  // The bug this file exists to catch from now on: editing a pane must reach
  // every example, not just the preview at the top. Change one rung to a
  // colour that appears nowhere else and look for it downstream.
  cur = withLadder(PRESETS.cur);
  render();
  const before = { fp: document.getElementById('fp').innerHTML,
                   uc: document.getElementById('uc').innerHTML,
                   al: document.getElementById('allsurf').innerHTML };
  const MARK = '#ff00ff';
  globalThis.__pane = {};
  for (const k of LKEYS) {
    cur = withLadder(PRESETS.cur);
    cur.ladder[k] = MARK;
    render();
    globalThis.__pane[k] = {
      fp: document.getElementById('fp').innerHTML.includes(MARK),
      uc: document.getElementById('uc').innerHTML.includes(MARK),
      al: document.getElementById('allsurf').innerHTML.includes(MARK),
    };
  }
  // And a text colour must too -- that half already worked, so this is a
  // regression guard rather than a fix.
  cur = withLadder(PRESETS.cur);
  cur.main = MARK;
  render();
  globalThis.__ink = document.getElementById('fp').innerHTML.includes(MARK);

  // overlay0 is an ink like any other: editing it must reach the examples and
  // both tables. It was absent from this tool entirely until 2026-09-09.
  cur = withLadder(PRESETS.cur);
  cur.ovl = MARK;
  render();
  globalThis.__ovl = {
    fp: document.getElementById('fp').innerHTML.includes(MARK),
    tbl: document.getElementById('tbl').innerHTML.includes('overlay0'),
    sep: document.getElementById('sep').innerHTML.includes('overlay0'),
    strip: document.getElementById('allsurf').innerHTML.includes(MARK),
  };
  // The border role must reach the examples: in the border theme it carries the
  // structure the fills used to, so it is the single most load-bearing value.
  cur = withLadder(PRESETS.brd);
  cur.bord = MARK;
  render();
  globalThis.__bord = document.getElementById('fp').innerHTML.includes(MARK);

  // The decided preset must exist and must be flat -- nothing shaded.
  const b = PRESETS.brd;
  globalThis.__brd = !!b && b.main === '#000000' && b.sec1 === b.acc &&
                     b.ladder.s0 === b.ladder.s1;

  // There must be a worked link example. The palette gives a link and a caption
  // one colour, so the underline is the only thing telling them apart -- a claim
  // that cannot be judged if nothing on the page is actually a link.
  cur = withLadder(PRESETS.brd);
  render();
  globalThis.__link = /text-decoration:\s*underline/.test(document.getElementById('fp').innerHTML);

  // The plan's role table must be live, not a hardcoded copy that drifts away
  // from the controls -- it is the first thing read on the page.
  cur = withLadder(PRESETS.brd);
  cur.bord = MARK;
  render();
  globalThis.__plan = {
    live: document.getElementById('plan_roles').innerHTML.includes(MARK.toUpperCase()),
    rows: (document.getElementById('plan_roles').innerHTML.match(/<tr>/g) || []).length,
  };

  // The pane-vs-pane table must exist and must react to a pane change.
  cur = withLadder(PRESETS.cur); render();
  const psBefore = document.getElementById('panesep').innerHTML;
  cur = withLadder(PRESETS.cur); cur.ladder.s1 = MARK; render();
  globalThis.__panesep = {
    rows: (psBefore.match(/<tr>/g) || []).length,
    reacts: document.getElementById('panesep').innerHTML !== psBefore,
  };

  // Labels on and off.
  cur = withLadder(PRESETS.cur);
  showTags = true;  render();
  globalThis.__tagsOn = document.getElementById('fp').innerHTML;
  globalThis.__ucTagsOn = document.getElementById('uc').innerHTML;
  showTags = false; render();
  globalThis.__tagsOff = document.getElementById('fp').innerHTML;
  globalThis.__ucTagsOff = document.getElementById('uc').innerHTML;

  showTags = true;
  cur = withLadder(PRESETS.cur); render();
`;
try {
  new Function(probe)();
} catch (e) {
  console.log('THREW: ' + e.message);
  process.exit(1);
}

const fp = store['fp'] ? store['fp'].innerHTML : '';
const note = store['fp_note'] ? store['fp_note'].innerHTML : '';
const checks = [
  ['full-page section rendered', fp.length > 2000],
  ['three windows, drawn twice each', (fp.match(/class="win"/g) || []).length === 6],
  ['bordered column present', fp.includes('borders (the default)')],
  ['cards column present', fp.includes('cards (optional theme)')],
  // Borders are the decision as of 2026-09-11, so they lead. If the columns
  // ever swap back, the page is showing the optional theme as the primary one.
  ['borders lead the cards', fp.indexOf('borders (the default)') < fp.indexOf('cards (optional theme)')],
  ['a context menu is drawn', fp.includes('class="menu"')],
  ['no unresolved template literal', !fp.includes('${')],
  ['no literal "undefined" leaked into markup', !fp.includes('undefined')],
  ['legend explains the rungs', note.includes('crust') && note.includes('surface0')],
  ['use-case strip still renders', (store['uc'] ? store['uc'].innerHTML.length : 0) > 500],
  // Named exactly, not with a fallback between two candidate ids. A check that
  // accepts either would keep passing if the element it is really guarding were
  // renamed, which is the one thing it exists to notice.
  ['surface strip still renders', (store['allsurf'] ? store['allsurf'].innerHTML.length : 0) > 200],
  ['contrast table still renders', (store['tbl'] ? store['tbl'].innerHTML.length : 0) > 200],
  ['ink-vs-ink table still renders', (store['sep'] ? store['sep'].innerHTML.length : 0) > 200],
];
let bad = 0;
for (const [what, ok] of checks) {
  console.log((ok ? 'ok    ' : 'FAIL  ') + what);
  if (!ok) bad++;
}
// Show which element ids the page actually populated, to catch a renamed target.
for (const [k, len, tpl, undef, html] of (global.__probe || [])) {
  const ok = len > 2000 && !tpl && !undef;
  console.log((ok ? 'ok    ' : 'FAIL  ') + 'preset ' + k + ' renders (' + len + ' chars)');
  if (!ok) bad++;
}
// A preset that carries its own surface ladder must visibly change the
// mockups. Equal lengths prove nothing -- every colour is 7 characters -- so
// compare the markup itself.
const byKey = Object.fromEntries((global.__probe || []).map(r => [r[0], r[4]]));
for (const k of ['op1', 'op2', 'op3']) {
  const differs = byKey[k] && byKey.cur && byKey[k] !== byKey.cur;
  console.log((differs ? 'ok    ' : 'FAIL  ') + k + ' changes the windows vs the shipped ladder');
  if (!differs) bad++;
}
for (const k of ['mantle', 'crust', 's0', 's1', 's2']) {
  const r = (global.__pane || {})[k] || {};
  const ok = r.fp && r.al;
  console.log((ok ? 'ok    ' : 'FAIL  ') + 'pane ' + k + ' reaches the examples' +
              (ok ? '' : ' (windows:' + !!r.fp + ' strip:' + !!r.al + ')'));
  if (!ok) bad++;
}
console.log((global.__ink ? 'ok    ' : 'FAIL  ') + 'a text colour reaches the examples');
if (!global.__ink) bad++;
{
  const on = global.__tagsOn || '', off = global.__tagsOff || '';
  const onHas = (on.match(/class="tag"/g) || []).length;
  const offHas = (off.match(/class="tag"/g) || []).length;
  const namesShown = ['mantle', 'crust', 'surface0', 'surface1', 'surface2'].every(n => on.includes(n));
  console.log((onHas > 10 ? 'ok    ' : 'FAIL  ') + 'shade labels appear when on (' + onHas + ' chips)');
  console.log((offHas === 0 ? 'ok    ' : 'FAIL  ') + 'shade labels disappear when off');
  console.log((namesShown ? 'ok    ' : 'FAIL  ') + 'every pane name is named somewhere in the windows');
  if (!(onHas > 10)) bad++;
  if (offHas !== 0) bad++;
  if (!namesShown) bad++;
}
{
  const on = (global.__ucTagsOn || '').match(/class="tag"/g) || [];
  const off = (global.__ucTagsOff || '').match(/class="tag"/g) || [];
  const ok = on.length > 5 && off.length === 0;
  console.log((ok ? 'ok    ' : 'FAIL  ') + 'use-case strip is labelled too (' + on.length + ' on, ' +
              off.length + ' off)');
  if (!ok) bad++;
}
{
  const o = global.__ovl || {};
  for (const [what, ok] of [['examples', o.fp], ['contrast table', o.tbl],
                            ['ink-vs-ink table', o.sep], ['surface strip', o.strip]]) {
    console.log((ok ? 'ok    ' : 'FAIL  ') + 'overlay0 reaches the ' + what);
    if (!ok) bad++;
  }
  const ps = global.__panesep || {};
  const psOk = ps.rows === 5 && ps.reacts;
  console.log((psOk ? 'ok    ' : 'FAIL  ') + 'pane-vs-pane table has 5 steps and reacts (' +
              ps.rows + ' rows, reacts:' + !!ps.reacts + ')');
  if (!psOk) bad++;
}
console.log((global.__bord ? 'ok    ' : 'FAIL  ') + 'the border colour reaches the examples');
if (!global.__bord) bad++;
console.log((global.__brd ? 'ok    ' : 'FAIL  ') + 'the decided preset is present and flat');
if (!global.__brd) bad++;
{
  const pl = global.__plan || {};
  const ok = pl.live && pl.rows === 6;
  console.log((ok ? 'ok    ' : 'FAIL  ') + 'the plan role table is live (' + pl.rows + ' rows, live:' +
              !!pl.live + ')');
  if (!ok) bad++;
}
console.log((global.__link ? 'ok    ' : 'FAIL  ') + 'a real link is drawn in the examples');
if (!global.__link) bad++;
console.log('populated ids: ' + Object.keys(store).filter(k => store[k].innerHTML).join(', '));
process.exit(bad ? 1 : 0);
