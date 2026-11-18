// Strings come from the agent's own /i18n.js (loaded as a classic script before this bundle) so the
// classic panel and this one never drift. Only strings that exist solely in the React panel live here.
declare global {
  interface Window {
    BAFT_I18N?: Record<string, Record<string, string>>;
    BAFT_LANGS?: Record<string, { label: string; dir: 'rtl' | 'ltr' }>;
  }
}

export type Lang = 'fa' | 'en';
export const LANGS: Record<Lang, { label: string; dir: 'rtl' | 'ltr' }> = {
  fa: { label: 'فارسی', dir: 'rtl' },
  en: { label: 'English', dir: 'ltr' },
};

const EXTRA: Record<Lang, Record<string, string>> = {
  fa: {
    'rx.classic': 'پنل کلاسیک',
    'rx.classic.note': 'پنل کلاسیک تا تأیید نهایی در دسترس می‌ماند.',
    'rx.wiz.n1': 'نصب نود',
    'rx.wiz.n2': 'اتصال',
    'rx.wiz.n3': 'ساخت تونل',
    'rx.wiz.n1.d': 'این نود باید نصب و فعال باشد. نسخه و وضعیت آن از خود Agent خوانده می‌شود.',
    'rx.wiz.n2.d': 'هویت این نود و نود مقابل را مشخص کنید. چیزی از سرور مقابل دیده نمی‌شود مگر اینکه واقعاً اندازه‌گیری شده باشد.',
    'rx.wiz.n3.d': 'پورت‌ها و نام تونل را وارد کنید، بررسی پیش‌نیازها و پیش‌نمایش را ببینید. هیچ چیزی تا مرحله اعمال تغییر نمی‌کند.',
    'rx.wiz.check': 'بررسی پیش‌نیازها',
    'rx.wiz.plan': 'پیش‌نمایش برنامه',
    'rx.wiz.next': 'بعدی',
    'rx.wiz.back': 'قبلی',
    'rx.wiz.apply': 'اعمال و جفت‌سازی در پنل کلاسیک',
    'rx.wiz.apply.note': 'اعمال تغییرات روی میزبان و جفت‌سازی هنوز در پنل کلاسیک انجام می‌شود تا برابری کامل تأیید شود.',
    'rx.wiz.err.required': 'این فیلد الزامی است.',
    'rx.wiz.err.port': 'پورت باید بین ۱ تا ۶۵۵۳۵ باشد.',
    'rx.wiz.installed': 'نسخهٔ نصب‌شده',
    'rx.wiz.mode': 'نقش پیکربندی‌شده',
    'rx.wiz.none': 'هنوز پیکربندی نشده',
    'rx.search': 'جستجو',
    'rx.copy': 'کپی',
    'rx.copied': 'کپی شد',
    'rx.copy.details': 'کپی جزئیات فنی',
    'rx.confirm': 'تأیید',
    'rx.loading': 'در حال بارگذاری',
    'rx.mode.preview': 'پیش‌نمایش',
    'rx.sev.all': 'همهٔ سطوح',
    'rx.reverse.hidden': 'حالت معکوس فقط وقتی Agent آن را اعلام کند نمایش داده می‌شود.',
    'rx.cfg.summary': 'خلاصهٔ پیکربندی',
    'rx.runtime': 'زمان اجرا',
    'rx.service': 'سرویس',
    'rx.settings.sensitive': 'خروج از نشست فعلی',
    'rx.settings.logout.body': 'توکن از این مرورگر حذف می‌شود و باید دوباره وارد شوید.',
  },
  en: {
    'rx.classic': 'Classic panel',
    'rx.classic.note': 'The classic panel stays available until the new one is accepted.',
    'rx.wiz.n1': 'Install node',
    'rx.wiz.n2': 'Connect',
    'rx.wiz.n3': 'Create tunnel',
    'rx.wiz.n1.d': 'This node must be installed and running. Its version and state are read from the Agent itself.',
    'rx.wiz.n2.d': 'Name this node and the peer. Nothing is shown about the other server unless it was actually measured.',
    'rx.wiz.n3.d': 'Enter the ports and tunnel name, then review the preflight checks and the preview. Nothing changes until you apply.',
    'rx.wiz.check': 'Run preflight',
    'rx.wiz.plan': 'Preview plan',
    'rx.wiz.next': 'Next',
    'rx.wiz.back': 'Back',
    'rx.wiz.apply': 'Apply and pair in the classic panel',
    'rx.wiz.apply.note': 'Applying changes to the host and pairing still happen in the classic panel until full parity is verified.',
    'rx.wiz.err.required': 'This field is required.',
    'rx.wiz.err.port': 'Port must be between 1 and 65535.',
    'rx.wiz.installed': 'Installed version',
    'rx.wiz.mode': 'Configured role',
    'rx.wiz.none': 'Not configured yet',
    'rx.search': 'Search',
    'rx.copy': 'Copy',
    'rx.copied': 'Copied',
    'rx.copy.details': 'Copy technical details',
    'rx.confirm': 'Confirm',
    'rx.loading': 'Loading',
    'rx.mode.preview': 'Preview',
    'rx.sev.all': 'All severities',
    'rx.reverse.hidden': 'Reverse mode is shown only when the Agent reports it.',
    'rx.cfg.summary': 'Configuration summary',
    'rx.runtime': 'Runtime',
    'rx.service': 'Service',
    'rx.settings.sensitive': 'End this session',
    'rx.settings.logout.body': 'The token is removed from this browser and you will have to sign in again.',
  },
};

export type Vars = Record<string, string | number | undefined>;

export function translate(lang: Lang, key: string, vars?: Vars): string {
  const base = window.BAFT_I18N ?? {};
  const s = EXTRA[lang][key] ?? base[lang]?.[key] ?? EXTRA.en[key] ?? base['en']?.[key];
  if (s === undefined) return key;
  return s.replace(/\{(\w+)\}/g, (_m, k: string) => (vars && vars[k] !== undefined ? String(vars[k]) : ''));
}

export function hasKey(lang: Lang, key: string): boolean {
  return translate(lang, key) !== key;
}
