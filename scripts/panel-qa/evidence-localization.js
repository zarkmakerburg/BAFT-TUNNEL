// Static contract test: every evidence code emitted by the Agent must be translated in fa/en.
const fs = require('node:fs');
const vm = require('node:vm');
const window = {};
vm.runInNewContext(fs.readFileSync('panel/i18n.js', 'utf8'), { window });
const codes = [
  'pfc.planner','pfc.direction_support','pfc.directory','pfc.setup_helper','pfc.existing_pairing','pfc.pki_helper','pfc.pki_files','pfc.node_key','pfc.directory_writable','pfc.service_port','pfc.tunnel_port','pfc.service_unit','pfc.peer_reachability','pfc.peer_identity','pfc.firewall',
  'pfr.planner','pfr.direction_support','pfr.directory','pfr.setup_helper','pfr.pki_helper','pfr.directory_writable','pfr.existing_pairing','pfr.pki_files','pfr.node_key','pfr.service_port','pfr.tunnel_port','pfr.service_unit','pfr.peer_reachability','pfr.peer_identity','pfr.firewall',
  'pf.planner.pass','pf.planner.blocked','pf.direction_support.pass','pf.direction_support.blocked',
  'pf.directory.pass','pf.directory.blocked','pf.directory.warning','pf.setup_helper.pass','pf.setup_helper.blocked',
  'pf.existing_pairing.pass','pf.existing_pairing.blocked','pf.pki_helper.pass','pf.pki_helper.blocked',
  'pf.pki_files.pass','pf.pki_files.warning','pf.pki_files.blocked','pf.node_key.pass','pf.node_key.blocked',
  'pf.directory_writable.pass','pf.directory_writable.blocked','pf.directory_writable.unknown',
  'pf.service_port.serve_accepting','pf.service_port.serve_waiting','pf.service_port.forward_busy','pf.service_port.forward_free',
  'pf.tunnel_port.pass','pf.tunnel_port.warning','pf.service_unit.pass','pf.service_unit.warning',
  'pf.peer_reachability.pass','pf.peer_reachability.warning','pf.peer_reachability.blocked','pf.peer_reachability.error','pf.peer_reachability.unknown',
  'pf.peer_identity.unknown','pf.firewall.unknown',
  'diag.source.unit_session','diag.source.active_tcp_probe','diag.source.last_failure','diag.source.config','diag.source.none','diag.source.unit_counters',
  'diag.tcp_reachability.pass','diag.tcp_reachability.fail','diag.tcp_reachability.unknown','diag.tcp_reachability.not_applicable',
  'diag.secure_channel.pass','diag.secure_channel.fail','diag.secure_channel.unknown',
  'diag.identity.pass','diag.identity.fail','diag.identity.unknown',
  'diag.authorization.pass','diag.authorization.fail','diag.authorization.unknown',
  'diag.session.pass','diag.session.fail','diag.session.unknown',
  'diag.data_path.pass','diag.data_path.unknown','diag.integrity.unknown',
  'diag.next.tcp_reachability.fail','diag.next.tcp_reachability.unknown','diag.next.secure_channel.fail',
  'diag.next.identity.fail','diag.next.authorization.fail','diag.next.session.fail','diag.next.data_path.unknown',
  'diag.next.start','diag.next.check','diag.next.none','diag.next.rerun_tier','diag.next.rerun'
];
let failed = 0;
for (const lang of ['fa', 'en']) {
  for (const code of codes) {
    const value = window.BAFT_I18N[lang][code];
    if (typeof value !== 'string' || !value.trim() || value === code) {
      console.error(`FAIL ${lang} missing ${code}`); failed++;
    }
  }
  const sessionTemplate = window.BAFT_I18N[lang]['diag.session.pass'];
  const sessionRendered = sessionTemplate.replace(/\{(\w+)\}/g, (_, key) => ({ sessions: 1, peer_id: 'test-peer' }[key] ?? ''));
  if (!sessionRendered.includes('test-peer') || /\{\w+\}/.test(sessionRendered)) {
    console.error(`FAIL ${lang} diag.session.pass peer_id interpolation`); failed++;
  }
}
if (failed) process.exit(1);
console.log(`EVIDENCE_LOCALIZATION=PASS (${codes.length} codes × fa/en)`);
