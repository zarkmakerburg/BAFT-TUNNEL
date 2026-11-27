// Checks for the panel defects the review enumerated. Logic lives in plain `.mjs` helpers so it is
// testable without a bundler or DOM; the purely presentational fixes are pinned by reading the
// component template and the stylesheet, so the old rule or hardcoded value cannot come back.
import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';

import { totalOrNull } from '../src/series.mjs';
import { longHistoryNotice } from '../src/history-view.mjs';
import { cellGeometry } from '../src/health-strip.mjs';

const source = (path) => fs.readFileSync(new URL(`../src/${path}`, import.meta.url), 'utf8');

// 1. Capability rows must come from the Agent, never from a literal.
test('no capability pill is hardcoded to "unknown" in the new panel', () => {
  for (const page of ['pages/Agent.tsx', 'pages/Servers.tsx']) {
    const text = source(page);
    assert.doesNotMatch(text, /CapPill state=\{null\}/, `${page} invents a capability`);
    assert.match(text, /cap\.setup\.note/, `${page} must say where plan/apply live`);
  }
});

// 2. A missing half of a series is absent, not 0.
test('a series total is null unless both sides were observed', () => {
  assert.equal(totalOrNull(120, 30), 150);
  assert.equal(totalOrNull(0, 0), 0, 'a measured zero stays zero');
  assert.equal(totalOrNull(null, 30), null);
  assert.equal(totalOrNull(120, undefined), null);
  assert.equal(totalOrNull(undefined, undefined), null);
});

test('no component turns a missing traffic counter into 0', () => {
  const traffic = source('pages/parts.tsx');
  assert.match(traffic, /totalOrNull\(x\.from_peer_bps, x\.to_peer_bps\)/);
  assert.doesNotMatch(traffic, /from_peer_bps \?\? 0/, 'traffic history still defaults to 0');
  const long = source('pages/Monitoring.tsx');
  assert.match(long, /totalOrNull\(x\.from_peer_bytes, x\.to_peer_bytes\)/);
  assert.doesNotMatch(long, /from_peer_bytes \?\? 0/, '24 h bytes still default to 0');
  assert.doesNotMatch(source('pages/Servers.tsx'), /st\.avail \?\? 0/, 'availability still defaults to 0');
});

// 3. Unavailable history is not an empty range.
test('the 24 h range distinguishes unavailable history from an empty one', () => {
  assert.equal(longHistoryNotice('unavailable', 0), 'unavailable');
  assert.equal(longHistoryNotice(undefined, 0), 'unavailable');
  assert.equal(longHistoryNotice('error', 0), 'error');
  assert.equal(longHistoryNotice('ok', 0), 'empty');
  assert.equal(longHistoryNotice('ok', 5), 'ok');
});

test('Monitoring asks the helper before choosing the empty message', () => {
  assert.match(source('pages/Monitoring.tsx'), /longHistoryNotice\(/);
});

// 4. Up/down never relies on colour alone.
test('a failed probe cell is shorter than a successful one', () => {
  const up = cellGeometry(false, 28);
  const down = cellGeometry(true, 28);
  assert.deepEqual(up, { y: 0, height: 28 });
  assert.ok(down.y > 0 && down.height < up.height, 'down must differ in shape, not only colour');
  assert.equal(down.height + down.y, up.height, 'the cell stays inside the strip');
});

test('the health strip uses the geometry helper and names both states in text', () => {
  const chart = source('components/charts.tsx');
  assert.match(chart, /cellGeometry\(/);
  assert.match(chart, /strip\.up/);
  assert.match(chart, /strip\.down/);
});

// 5. Map labels are not scaled by the viewBox.
test('the geo map keeps its endpoint names out of the scaled SVG', () => {
  const viz = source('components/viz.tsx');
  assert.doesNotMatch(viz, /fontSize=\{viewW/, 'label size must not follow the zoom window');
  assert.doesNotMatch(viz, /pin-label/, 'the scaled label class must be gone');
  assert.match(viz, /className="geo-pins"/, 'endpoint names live in a normal-sized caption');
  const css = source('styles.css');
  assert.match(css, /\.geo-pins\{[^}]*font-size:var\(--fs-1\)/, 'the caption is sized in CSS px');
  assert.doesNotMatch(css, /\.pin-label/, 'the old scaled-label rule must be gone');
});

// 6. The rail icon mirrors in RTL.
test('the collapse icon opts into the RTL flip that the stylesheet defines', () => {
  assert.match(source('shell/Shell.tsx'), /<Icon name="panel" className="flip"/);
  assert.match(source('styles.css'), /\[dir=rtl\] \.flip\{transform:scaleX\(-1\)\}/);
});

// 7. Stale data is signalled in text, not by dimming the page.
test('stale state no longer dims the view and is announced in text', () => {
  const css = source('styles.css');
  assert.doesNotMatch(css, /is-stale/, 'the dim rule must be gone');
  const shell = source('shell/Shell.tsx');
  assert.doesNotMatch(shell, /is-stale/, 'the shell must not toggle the dim class');
  assert.match(shell, /stale && <Pill kind="warn">\{t\('stale'\)\}<\/Pill>/, 'stale must be visible as text');
});
