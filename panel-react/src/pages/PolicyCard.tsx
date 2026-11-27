import type { ReactNode } from 'react';
import { useApp } from '../app';
import { useData } from '../feed-context';
import { ago, fmtNum, type Kind, type TFn } from '../derive';
import { Card, Empty, KV, Ltr, Pill, Table } from '../components/ui';
import { policyView, type PolicyMetric, type PolicyState } from '../policy-view.mjs';

// ADR-0047's read-only object, rendered as-is. This card adds no field and derives no state: a
// missing status read, a 401/unreachable endpoint, an agent without the member and an `unknown`
// reading each render as unavailable/unknown with the reason the payload gives. Metrics that were
// not observed show their provenance reason, never 0, and configured profile is never shown as
// running.

const STATE_KIND: Record<PolicyState, Kind> = { active: 'ok', fallback: 'warn', off: 'info', unknown: 'unknown' };
const CLAIM_KIND: Record<string, Kind> = { measured: 'ok', unknown: 'unknown', unsupported: 'info' };

/** An i18n key with a readable fallback: a new reason from the Agent never renders as a raw key. */
function label(t: TFn, key: string, fallback?: string): string {
  const value = t(key);
  return value === key ? (fallback ?? key) : value;
}

function reasonLabel(t: TFn, reason: string | null): string {
  if (!reason) return '—';
  return label(t, 'pv.reason.' + reason, reason);
}

function metricValue(t: TFn, lang: 'fa' | 'en', metric: PolicyMetric) {
  if (!metric.available || metric.value == null) {
    return <span className="muted">{t('pv.metric.unavailable', { reason: reasonLabel(t, metric.reason) })}</span>;
  }
  return (
    <Ltr>
      {fmtNum(lang, metric.value)}
      {metric.unit ? ` ${metric.unit}` : ''}
    </Ltr>
  );
}

export function PolicyCard() {
  const { t, lang } = useApp();
  const { data: d } = useData();
  const view = policyView(d?.status ?? null, d?.meta.status);
  const unavailable = view.unavailability;
  const action = view.available ? (
    <Pill kind={STATE_KIND[view.state]}>{t('pv.state.' + view.state)}</Pill>
  ) : (
    <Pill kind={unavailable?.kind === 'error' ? 'bad' : 'unknown'}>{t('pv.unavailable')}</Pill>
  );

  if (!view.available || unavailable) {
    const reasonKey = 'pv.unavailable.' + (unavailable?.reason ?? 'no_policy_member');
    return (
      <Card title={t('pv.title')} action={action}>
        <p className="sub">{t('pv.note')}</p>
        <Empty kind={unavailable?.kind ?? 'unavailable'}>
          {label(t, reasonKey, t('pv.unavailable.generic', { reason: unavailable?.reason ?? 'unknown' }))}
        </Empty>
      </Card>
    );
  }

  const onOff = (value: boolean | null) =>
    value == null ? t('pv.unknown') : value ? t('pv.on') : t('pv.off');
  const switchState = label(t, 'pv.switch.' + view.switchState.state, view.switchState.state);
  const switchCell = (
    <>
      <Pill kind={view.switchState.state === 'on' ? 'ok' : view.switchState.state === 'unknown' ? 'unknown' : 'info'}>{switchState}</Pill>
      {view.switchState.value && <> <Ltr>{view.switchState.value}</Ltr></>}
    </>
  );

  const rows: [string, ReactNode][] = [
    [t('pv.state'), <Pill key="state" kind={STATE_KIND[view.state]}>{t('pv.state.' + view.state)}</Pill>],
          [t('pv.reason'), view.state === 'active' ? t('pv.reason.none') : reasonLabel(t, view.reason)],
          [t('pv.feature'), view.featureCompiled == null ? '—' : view.featureCompiled ? t('pv.compiled.yes') : t('pv.compiled.no')],
          [t('pv.switch'), switchCell],
          [
            t('pv.switch.sources'),
            view.switchState.sources.length ? (
              <ul className="list" key="sources">
                {view.switchState.sources.map((source) => (
                  <li key={source}><Ltr>{source}</Ltr></li>
                ))}
              </ul>
            ) : (
              t('pv.switch.nosources')
            ),
          ],
          [t('pv.profile.configured'), view.profile.configured ? <Ltr key="c">{view.profile.configured}</Ltr> : t('pv.notobserved')],
          [t('pv.profile.effective'), view.profile.configuredEffective ? <Ltr key="e">{view.profile.configuredEffective}</Ltr> : t('pv.notobserved')],
          [t('pv.profile.running'), view.profile.running ? <Ltr key="r">{view.profile.running}</Ltr> : t('pv.notobserved')],
          [
            t('pv.adaptive'),
            t('pv.adaptive.value', { configured: onOff(view.adaptive.configured), effective: onOff(view.adaptive.effective) }),
          ],
    [
      t('pv.fallback'),
      view.fallback
        ? `${reasonLabel(t, view.fallback.cause)}${view.fallback.since ? ' · ' + ago(lang, t, view.fallback.since) : ''}`
        : t('pv.fallback.none'),
    ],
  ];
  if (view.evidence) {
    rows.push([
      t('pv.evidence'),
      t('pv.evidence.value', {
        pid: view.evidence.pid == null ? t('pv.unknown') : String(view.evidence.pid),
        age: view.evidence.ageSeconds == null ? t('pv.unknown') : String(view.evidence.ageSeconds),
      }),
    ]);
  }

  return (
    <Card title={t('pv.title')} action={action}>
      <p className="sub">{t('pv.note')}</p>
      <KV rows={rows} />

      <h3>{t('pv.tuples')}</h3>
      {view.tuples.length ? (
        <Table
          caption={t('pv.tuples')}
          cols={[t('pv.tuple.engine'), t('pv.tuple.carrier'), t('pv.tuple.profile'), t('pv.tuple.claim'), t('pv.tuple.evidence')]}
          rows={view.tuples.map((tuple) => [
            tuple.engine ? <Ltr key="e">{tuple.engine}</Ltr> : '—',
            tuple.carrier ? <Ltr key="c">{tuple.carrier}</Ltr> : '—',
            tuple.profile ? <Ltr key="p">{tuple.profile}</Ltr> : '—',
            <Pill key="cl" kind={CLAIM_KIND[tuple.claim] ?? 'unknown'}>{label(t, 'pv.claim.' + tuple.claim, tuple.claim)}</Pill>,
            tuple.evidence ? <Ltr key="ev">{tuple.evidence}</Ltr> : '—',
          ])}
        />
      ) : (
        <Empty kind="unavailable">{t('pv.tuples.none')}</Empty>
      )}
      <p className="sub">{t('pv.tuples.note')}</p>

      <h3>{t('pv.metrics')}</h3>
      {view.metrics.length ? (
        <Table
          caption={t('pv.metrics')}
          cols={[t('pv.metric.name'), t('pv.metric.value'), t('pv.metric.provenance'), t('pv.metric.freshness')]}
          rows={view.metrics.map((metric) => [
            label(t, 'pv.metric.' + metric.name, metric.name),
            <span key="v">{metricValue(t, lang, metric)}</span>,
            metric.provenance ? label(t, 'pv.prov.' + metric.provenance, metric.provenance) : '—',
            metric.available ? (
              <Pill key="f" kind={metric.fresh === false ? 'warn' : 'ok'}>{metric.fresh === false ? t('pv.stale') : t('pv.fresh')}</Pill>
            ) : (
              '—'
            ),
          ])}
        />
      ) : (
        <Empty kind="unavailable">{t('pv.metrics.none')}</Empty>
      )}
      <p className="sub">{t('pv.note.honest')}</p>
    </Card>
  );
}
