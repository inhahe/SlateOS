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
    cur = { ...PRESETS[k] };
    render();
    const h = document.getElementById('fp').innerHTML;
    globalThis.__probe.push([k, h.length, h.includes('$' + '{'), h.includes('undefined'), h]);
  }
  cur = { ...PRESETS.cur }; render();
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
  ['shaded column present', fp.includes('shaded (today)')],
  ['bordered column present', fp.includes('with borders')],
  ['a context menu is drawn', fp.includes('class="menu"')],
  ['no unresolved template literal', !fp.includes('${')],
  ['no literal "undefined" leaked into markup', !fp.includes('undefined')],
  ['legend explains the rungs', note.includes('crust') && note.includes('surface0')],
  ['use-case strip still renders', (store['uc'] ? store['uc'].innerHTML.length : 0) > 500],
  ['surface strip still renders', (store['surfaces'] ? store['surfaces'].innerHTML.length : 0) > 200
    || (store['allsurf'] ? store['allsurf'].innerHTML.length : 0) > 200],
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
console.log('populated ids: ' + Object.keys(store).filter(k => store[k].innerHTML).join(', '));
process.exit(bad ? 1 : 0);
