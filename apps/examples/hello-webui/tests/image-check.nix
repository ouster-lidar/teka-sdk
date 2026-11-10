{ pkgs, imageVersion }:

''
  dump=${pkgs.erofs-utils}/bin/dump.erofs
  "$dump" --offset="$root_offset" --cat --path=usr/lib/teka/mission-control/manifest.json "$raw" >manifest.json
  ${pkgs.jq}/bin/jq -e '
    .id == "hello-webui" and .version == "${imageVersion}" and
    .ui.mode == "static" and .ui.template == "demo.html" and .ui.styles == ["demo.css"] and
    .nav.panels == [{"label":"Demo","panel":"demo"}] and .backend.mode == "none"
  ' manifest.json >/dev/null
  "$dump" --offset="$root_offset" --ls --path=usr/lib/teka/mission-control/web "$raw" >web-files
  test "$(awk 'NF == 3 && $2 == 1 { print $3 }' web-files)" = "$(printf '%s\n' app-api.js demo.css demo.html demo.js)"
  "$dump" --offset="$root_offset" --cat --path=usr/lib/teka/mission-control/web/demo.html "$raw" >demo.html
  test -s demo.html
  "$dump" --offset="$root_offset" --cat --path=usr/lib/teka/mission-control/web/demo.js "$raw" >demo.js
  ${pkgs.nodejs}/bin/node - demo.js <<'JS'
  const assert = require('node:assert/strict');
  const fs = require('node:fs');
  const vm = require('node:vm');
  const source = fs.readFileSync(process.argv[2], 'utf8');
  const html = fs.readFileSync('demo.html', 'utf8');
  const cards = [...html.matchAll(/<article\b[^>]*>/g)].map(function ([tag]) {
    return {
      dataset: Object.fromEntries([...tag.matchAll(/data-([a-z]+)="([^"]*)"/g)].map(([, key, value]) => [key, value])),
      output: {},
      line: { updates: 0, setAttribute(name, value) { assert.equal(name, 'points'); this.points = value; this.updates++; } },
      querySelector(selector) { return selector === 'output' ? this.output : this.line; }
    };
  });
  let refresh;
  const updated = {};
  const dashboard = {
    querySelectorAll() { return cards; },
    querySelector(selector) {
      return selector === 'button' ? { addEventListener(event, callback) { assert.equal(event, 'click'); refresh = callback; } } : updated;
    }
  };
  vm.runInNewContext(source, { window: {}, document: { getElementById: () => dashboard } });
  assert.equal(cards.length, 3);
  function check(updates) {
    for (const card of cards) {
      const value = parseFloat(card.output.textContent);
      assert(Number.isFinite(value) && value >= 0 && value <= Number(card.dataset.max || Infinity));
      assert(card.output.textContent.endsWith(card.dataset.unit));
      assert.equal(card.line.updates, updates);
      const points = card.line.points.split(' ').map(point => point.split(',').map(Number));
      assert.equal(points.length, 24);
      assert(points.every(([x, y]) => x >= 0 && x <= 240 && y >= 0 && y <= 72));
    }
    assert(updated.textContent.startsWith('Updated '));
  }
  check(1);
  refresh();
  check(2);
  vm.runInNewContext(source, { window: {}, document: { getElementById: () => null } });
  JS
''
