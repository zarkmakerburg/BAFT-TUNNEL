import { useEffect, useId, useRef, useState, type ReactNode } from 'react';
import { useApp } from '../app';
import type { Kind } from '../derive';

export const GLYPH: Record<Kind | 'off', string> = { ok: '✓', warn: '!', bad: '✕', unknown: '?', info: 'i', off: '○' };

/** Identifiers (IDs, addresses, ports, hashes, commands, versions) stay left-to-right in both languages. */
export function Ltr({ children }: { children: ReactNode }) {
  return <bdi className="ltr" dir="ltr">{children ?? '—'}</bdi>;
}

/** Status is never colour-only: every pill carries a glyph and a text label. */
export function Pill({ kind, children }: { kind: Kind; children: ReactNode }) {
  return (
    <span className={`pill ${kind}`}>
      <span className="glyph" aria-hidden="true">{GLYPH[kind]}</span>
      <span>{children}</span>
    </span>
  );
}

export function Card({ title, action, children, className }: { title?: ReactNode; action?: ReactNode; children: ReactNode; className?: string }) {
  return (
    <section className={`card ${className ?? ''}`}>
      {(title || action) && (
        <header className="card-head">
          {title && <h2>{title}</h2>}
          {action}
        </header>
      )}
      {children}
    </section>
  );
}

export function Kpi({ label, value, hint, state }: { label: string; value: ReactNode; hint?: ReactNode; state?: { k: Kind; l: string } | null }) {
  return (
    <div className="kpi">
      <div className="kpi-label">{label}</div>
      <div className="kpi-value">{value}</div>
      {hint && <div className="kpi-hint">{hint}</div>}
      {state && <Pill kind={state.k}>{state.l}</Pill>}
    </div>
  );
}

export function KV({ rows }: { rows: [string, ReactNode][] }) {
  return (
    <dl className="kv">
      {rows.filter((r) => r[1] !== null && r[1] !== undefined).map(([k, v]) => (
        <div key={k} className="kv-row">
          <dt>{k}</dt>
          <dd>{v}</dd>
        </div>
      ))}
    </dl>
  );
}

/** Empty, unavailable and error are different states and look different. */
export function Empty({ kind = 'empty', children, action }: { kind?: 'empty' | 'unavailable' | 'error'; children: ReactNode; action?: ReactNode }) {
  return (
    <div className={`empty ${kind}`} role={kind === 'error' ? 'alert' : 'status'}>
      <span className="glyph" aria-hidden="true">{kind === 'error' ? '✕' : kind === 'unavailable' ? '—' : '○'}</span>
      <p>{children}</p>
      {action}
    </div>
  );
}

export function Banner({ kind, title, children, action }: { kind: 'info' | 'bad' | 'partial' | 'warn'; title?: string; children?: ReactNode; action?: ReactNode }) {
  return (
    <div className={`banner ${kind}`} role={kind === 'bad' ? 'alert' : 'status'}>
      {title && <h2>{title}</h2>}
      {children}
      {action}
    </div>
  );
}

export function Skeleton() {
  const { t } = useApp();
  return (
    <div className="skeleton" role="status" aria-busy="true" aria-label={t('rx.loading')}>
      <span className="sk w60" /><span className="sk w90" /><span className="sk w75" /><span className="sk w90" />
    </div>
  );
}

export function Segmented<T extends string>({ value, options, onChange, label }: { value: T; options: [T, string][]; onChange: (v: T) => void; label: string }) {
  return (
    <div className="segmented" role="group" aria-label={label}>
      {options.map(([k, l]) => (
        <button key={k} type="button" aria-pressed={value === k} onClick={() => onChange(k)}>{l}</button>
      ))}
    </div>
  );
}

export function Field({ label, error, hint, children }: { label: string; error?: string; hint?: string; children: (p: { id: string; 'aria-invalid'?: true; 'aria-describedby'?: string }) => ReactNode }) {
  const id = useId();
  const dId = `${id}-d`;
  return (
    <div className="field">
      <label htmlFor={id}>{label}</label>
      {children({ id, ...(error ? { 'aria-invalid': true as const } : {}), ...(error || hint ? { 'aria-describedby': dId } : {}) })}
      {(error || hint) && <p id={dId} className={error ? 'fielderr' : 'sub'} role={error ? 'alert' : undefined}>{error ?? hint}</p>}
    </div>
  );
}

/** Dependency-free tooltip: shown on hover and keyboard focus, described via aria-describedby. */
export function Tip({ text, children }: { text: string; children: ReactNode }) {
  const id = useId();
  return (
    <span className="tip" aria-describedby={id}>
      {children}
      <span role="tooltip" id={id} className="tip-box">{text}</span>
    </span>
  );
}

/** Native <dialog>: focus trap, Esc and focus restore come from the platform. */
export function Dialog({ open, onClose, label, kind = 'dialog', children }: { open: boolean; onClose: () => void; label: string; kind?: 'dialog' | 'drawer' | 'palette' | 'popover'; children: ReactNode }) {
  const ref = useRef<HTMLDialogElement>(null);
  const opener = useRef<Element | null>(null);
  useEffect(() => {
    const d = ref.current;
    if (!d) return;
    if (open && !d.open) {
      opener.current = document.activeElement;
      d.showModal();
      document.documentElement.classList.add('lock');
      (window as any).__baftHold = ((window as any).__baftHold ?? 0) + 1;
    } else if (!open && d.open) d.close();
    return () => {
      if (d.open) d.close();
    };
  }, [open]);
  return (
    <dialog
      ref={ref}
      className={`ov ov-${kind}`}
      aria-label={label}
      onClose={() => {
        document.documentElement.classList.remove('lock');
        (window as any).__baftHold = Math.max(0, ((window as any).__baftHold ?? 1) - 1);
        if (opener.current instanceof HTMLElement && opener.current.isConnected) opener.current.focus();
        onClose();
      }}
      onClick={(e) => { if (e.target === ref.current) ref.current?.close(); }}
    >
      {open && children}
    </dialog>
  );
}

/** State-changing actions name the action and its consequence before anything is sent. */
export function Confirm({ open, title, body, okLabel, danger, onResult }: { open: boolean; title: string; body: string; okLabel: string; danger?: boolean; onResult: (ok: boolean) => void }) {
  const { t } = useApp();
  const answered = useRef(false);
  useEffect(() => { if (open) answered.current = false; }, [open]);
  const finish = (ok: boolean) => {
    if (answered.current) return;
    answered.current = true;
    onResult(ok);
  };
  return (
    <Dialog open={open} label={title} onClose={() => finish(false)}>
      <div className="dlg">
        <h2>{title}</h2>
        <p>{body}</p>
        <div className="row">
          <button type="button" className={danger ? 'danger' : 'primary'} onClick={() => finish(true)} autoFocus>{okLabel}</button>
          <button type="button" onClick={() => finish(false)}>{t('ui.cancel')}</button>
        </div>
      </div>
    </Dialog>
  );
}

export function Toasts() {
  const { toasts } = useApp();
  return (
    <div className="toasts" role="status" aria-live="polite">
      {toasts.map((x) => <div key={x.id} className={`toast ${x.kind}`}>{x.text}</div>)}
    </div>
  );
}

export function CopyButton({ text, label }: { text: string; label?: string }) {
  const { t, toast } = useApp();
  const [done, setDone] = useState(false);
  return (
    <button type="button" className="ghost" onClick={() => {
      void navigator.clipboard?.writeText(text).then(() => { setDone(true); toast(t('rx.copied'), 'ok'); window.setTimeout(() => setDone(false), 1500); }, () => toast(t('fail'), 'bad'));
    }}>{done ? t('rx.copied') : label ?? t('rx.copy')}</button>
  );
}

/** Simple data table; on narrow screens each row becomes a labelled card (CSS reads data-label). */
export function Table({ cols, rows, caption }: { cols: string[]; rows: ReactNode[][]; caption: string }) {
  return (
    <div className="table-wrap">
      <table className="cards">
        <caption className="sr">{caption}</caption>
        <thead><tr>{cols.map((c) => <th key={c} scope="col">{c}</th>)}</tr></thead>
        <tbody>
          {rows.map((r, i) => (
            <tr key={i}>{r.map((cell, j) => <td key={j} data-label={cols[j]}>{cell}</td>)}</tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}
