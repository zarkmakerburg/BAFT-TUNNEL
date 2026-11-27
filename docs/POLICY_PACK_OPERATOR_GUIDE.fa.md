# راهنمای اپراتور بستهٔ سیاست ترافیک (فارسی)

> **وضعیت: پیش‌نمایش، به‌صورت پیش‌فرض خاموش، هیچ پروفایلی اندازه‌گیری‌شده نیست.** بسته پشت سه گیت
> است که اپراتور باید تنظیم کند: feature کامپایل `policy-pack`، سوئیچ واحد
> `BAFT_TUNNEL_POLICY_PACK` و (اختیاری) `policy.profile`. همهٔ رکوردهای پروفایل در این مخزن شاهد
> هارنس در سطح **`simulated`** هستند، هیچ tuple از نوع (engine, carrier, profile) `measured` نیست و
> هیچ‌چیز اینجا ادعایی دربارهٔ تأخیر مسیر اینترنت یا آمادگی انتشار پایدار نیست. گیت `v0.4.0` هنوز به
> شاهد دو‌میزبانهٔ LAB برای هر tuple (TP-3.x) و پنجرهٔ دو‌میزبانهٔ TP-4.1/TP-4.2 نیاز دارد؛ این صفحه
> فقط موجود را مستند می‌کند و آن‌ها را نمی‌بندد.
>
> این صفحه **سمت خواندن** را توصیف می‌کند: عضو `policy` در `GET /v1/status` چه دارد، CLI و پنل آن را
> چطور نشان می‌دهند، و وقتی مقداری `unknown` یا در دسترس نیست خواننده چه باید بکند. طراحی در
> [ADR-0031](ADR-0031-adaptive-traffic-policy-pack.md) (پروفایل‌ها)،
> [ADR-0047](ADR-0047-policy-status-contract.md) (همین قرارداد وضعیت)،
> [ADR-0056](ADR-0056-adaptive-optimization-controller.md) (کنترلر تطبیقی) و
> [ADR-0057](ADR-0057-policy-operator-surface.md) (سطح نوشتن) است. نسخهٔ انگلیسی:
> [POLICY_PACK_OPERATOR_GUIDE.md](POLICY_PACK_OPERATOR_GUIDE.md).

## ۱. بسته چیست و گیت‌هایش کدام‌اند

بسته یک افزونهٔ اختیاری است که زمان‌بندی ترافیک TCP رله‌شده را در موتورهای Rust
(`tcp-tls` و `noise-tcp` روی TCP/Yamux) تغییر می‌دهد: برای هر جهت یک جفت زمان‌بند — پنجرهٔ
stream در Yamux و quantum/cap در deficit round-robin — و پنج ردیف پروفایل نسخه‌دار:
`balanced/1`، `gaming/1`، `trading/1`، `streaming/1` و `bulk/1`. موتور `baft` و حامل UDP خارج از
دامنه‌اند و در کارت و شیء وضعیت `unsupported` گزارش می‌شوند، هرگز با نتیجهٔ موتور Rust یکی نمی‌شوند.

فعال‌سازی به هر سه گیت نیاز دارد و گیت چهارمی وجود ندارد:

1. باینری با `--features policy-pack` کامپایل شده باشد؛
2. واحد سوئیچ `BAFT_TUNNEL_POLICY_PACK` را تنظیم کند (drop-in مدیریت‌شدهٔ
   `/etc/systemd/system/baft-tunnel@<instance>.service.d/50-policy-pack.conf` با
   `Environment=BAFT_TUNNEL_POLICY_PACK=sched`)؛
3. کانفیگ تونل اختیاری `policy.profile` را نام ببرد؛ نبودش یعنی `balanced/1`.

با سوئیچ تنظیم‌نشده یا feature کامپایل‌نشده، رله مسیر قبلی را می‌رود و مسیر داده بایت‌به‌بایت همان
امروز است. `POST /v1/policy/apply` (enable/disable/select/reset) سطح نوشتن است؛ رسیدش `configured`
و `restart_required` می‌گوید، هرگز «active» — تنها خواندن زیر می‌تواند این را بگوید.

## ۲. از کجا بخوانیم

هر دو مسیر واقعی همان شیء را می‌خوانند و هیچ‌کدام فیلدی اضافه نمی‌کند.

### ۲.۱ CLI

```console
baft-tunnel policy status --instance edge
baft-tunnel policy status --instance edge --json
```

- فرمان اول نمونهٔ متصل ایجنت را بررسی می‌کند، سپس `GET /v1/status` را می‌خواند و عضو `policy` آن را
  چاپ می‌کند (متن به‌صورت پیش‌فرض، شیء خام با `--json`).
- کد خروج **۰** هر وقت ایجنت پاسخ داده باشد — حتی برای خواندن `unknown`. کد خروج **۲** وقتی ایجنت در
  دسترس نیست، توکن را رد می‌کند (401) یا به نمونهٔ دیگری وصل است. خواندنِ شکست‌خورده هرگز به‌شکل
  وضعیت سالم چاپ نمی‌شود.
- `--agent` و `--token-file` مثل بقیهٔ فرمان‌های `policy` هستند؛ URL باید origin لوپ‌بک باشد.

### ۲.۲ پنل

- **پنل پیش‌فرض React** در `/` (صفحهٔ Agent، کارت «بستهٔ سیاست ترافیک») همان شیء را فارسی و انگلیسی
  با RTL نشان می‌دهد (از PR #472).
- پنل کلاسیک در `/classic` عمداً **هیچ** کارت policy ندارد. این پنل همان fallback یک‌نسخه‌ای
  [ADR-0054](ADR-0054-react-local-agent-panel.md) است؛ برای این خواندن از CLI (`policy status`) یا
  پنل پیش‌فرض استفاده کنید.
- کارت فقط مقدارها را نشان می‌دهد؛ متریکی که مشاهده نشده «در دسترس نیست (دلیل)» است، هرگز `0`؛ و
  عضو غایب یا خواندن شکست‌خورده/401 به‌شکل «در دسترس نیست با دلیل» رندر می‌شود.

### ۲.۳ مسیر خام

```console
curl -s -H "Authorization: Bearer $TOKEN" http://127.0.0.1:18080/v1/status
```

عضو `policy` روی ایجنتی که این خواندن را دارد همیشه حاضر است و `interface_version` آن `1` است.
ایجنت قدیمی‌تر از این خواندن به‌کل عضو `policy` را ندارد — و این با `off` یکی نیست.

## ۳. شیء، فیلد به فیلد

| فیلد | مقدارها | معنا |
|---|---|---|
| `interface_version` | `1` | نسخهٔ interface سیاست (ADR-0044) که این شیء از آن پیروی می‌کند. |
| `feature_compiled` | `true`/`false` | آیا **همین باینری ایجنت** با `policy-pack` ساخته شده است. با `false` هیچ سوئیچی پک را اجرا نمی‌کند. |
| `state` | `off` `active` `fallback` `unknown` | دقیقاً یکی از این چهار حالت بسته؛ §۳.۱. |
| `reason` | مجموعهٔ بسته، یا `null` | چرا حالت `active` نیست؛ `null` فقط برای `active`. §۳.۱. |
| `profile.configured` | شناسهٔ نسخه‌دار، یا `null` | آنچه **کانفیگ** تونل می‌خواهد؛ نبودش `null` است (و مؤثر `balanced/1` گزارش می‌شود). |
| `profile.configured_effective` | `balanced/1` … `bulk/1` | شناسهٔ پیکربندی‌شده پس از قاعدهٔ «نبود = Balanced». |
| `profile.running` | شناسهٔ نسخه‌دار، یا `null` | ردیفی که **واحد در حال اجرا** منتشر کرده است. بدون شاهد تازه `null` است و هرگز از کانفیگ کپی نمی‌شود. |
| `profile.class` | مثل `running`، یا `null` | کلاس ترافیک نشست در حال اجرا؛ یک کلاس برای هر نشست. |
| `switch.state` | `on` `off` `absent` `unknown` | نتیجهٔ اسکن فقط‌خواندنی `Environment=` در واحد و drop-in‌ها. |
| `switch.value` | رشتهٔ سوئیچ، یا `null` | مقدار یافت‌شده، مثل `sched` یا `off`. |
| `switch.sources` | فهرست مسیرها | هر فایل واحد/drop-in که سوئیچ را تنظیم می‌کند. بیش از یکی، حالت را `unknown` می‌کند. |
| `switch.scan` | `null`، `set_in_multiple_sources`، `unreadable` | چرا خواندن سوئیچ on/off ساده نیست. |
| `adaptive.configured` | `true`/`false` | opt-in `policy.adaptive` از کانفیگ. |
| `adaptive.effective` | `true`/`false`/`null` | فقط با شاهد تازه و حالت `active`/`fallback` مؤثر است؛ وگرنه `null`. |
| `fallback` | `null` یا `{cause, since_unix}` | آخرین fallback ثبت‌شدهٔ منتشرکننده، با علت و زمان شروع. |
| `tuples[]` | یک ردیف برای هر `(engine, carrier, profile)` | `claim` یکی از `unsupported`/`unknown`/`measured` است؛ `evidence` نام رکورد یا `null`. §۳.۲. |
| `capabilities` | پنج نام → `supported`/`unsupported`/`unknown` | اعلام موتور؛ این سطح هیچ‌چیز را فرض نمی‌کند و تا اعلام زندهٔ موتور همه `unknown` است. |
| `metrics` | چهار نام → شیء متریک | §۳.۳. |
| `evidence.running` | `{pid, sampled_unix, age_s, fresh}` یا `null` | همان snapshot منتشرشده‌ای که فیلدهای running از آن آمده‌اند. |

مقدار مشاهده‌نشده `null` با دلیل است؛ هرگز `0` ساختگی، رشتهٔ خالی یا پروفایل پیش‌فرض به‌عنوان
«در حال اجرا» نیست.

### ۳.۱ حالت‌ها و دلیل‌ها

| `state` | معنا | `reason` ممکن |
|---|---|---|
| `active` | شاهد تازهٔ اجرا می‌گوید پک با پروفایل resolve‌شده در حال اجراست. | `null` |
| `fallback` | منتشرکننده بازگشت به ردیف پیکربندی‌شده را ثبت کرده است. | علت fallback (مثل `guard_violation`) |
| `off` | پک قرار نیست اجرا شود. | `feature_not_compiled`، `switch_absent`، `switch_off` |
| `unknown` | خواندن نمی‌تواند اثبات کند چه چیزی اجرا می‌شود. | `no_instance`، `no_running_evidence`، `stale_running_evidence`، `unit_not_running`، `config_unreadable`، `switch_set_in_multiple_sources`، `unrecognized_state`* |

\* `unrecognized_state` و `reason_not_reported` را **مدل نمایش پنل** اضافه می‌کند، نه ایجنت، وقتی
payload بدشکل یا بیرون از قرارداد باشد. خود ایجنت این‌ها را نمی‌فرستد.

`active` به همهٔ این‌ها نیاز دارد: سوئیچ روشن، feature کامپایل‌شده، snapshotی از `policy.json` که
بیش از سه بازهٔ نمونه‌برداری (هر بازه ۱ ثانیه) کهنه نباشد، و — وقتی systemd مقدار می‌دهد —
`MainPID`ی که با `pid` همان snapshot یکی باشد. پس پروفایلی که پیکربندی شده ولی واحد از آن زمان
restart نشده `unknown` است؛ همان واقعیتی که `restart_required` در رسید نوشتن می‌گوید.

### ۳.۲ ادعای tuple

هر ادعا به یک `(engine, carrier, profile)` دقیق تعلق دارد و فقط وقتی معتبر است که رکورد همان موتور را
اجرا کرده باشد. امروز:

- `tcp-tls` و `noise-tcp` هر پنج پروفایل را با `claim: "unknown"` و `evidence: null` فهرست می‌کنند،
  چون هر رکورد مخزن یک اجرای هارنس لوپ‌بک در سطح `simulated` است و خط هارنس tuple نیست؛
- `baft` یک ردیف با `claim: "unsupported"` دارد (ADR-0031 تصمیم ۱۷) و هرگز ردیف موتور Rust را به
  ارث نمی‌برد؛
- هیچ ردیفی `measured` نیست. یک رکورد `simulated` هرگز این جدول را راضی نمی‌کند.

### ۳.۳ متریک‌ها

هر متریک `value`، `unit`، `interval_seconds`، `provenance`، `fresh`، `available` و `reason` دارد.
`value` فقط وقتی عدد است که `available` برابر `true` باشد؛ وگرنه `null` است و `reason` می‌گوید چرا.

| متریک | واحد | منشأ | امروز |
|---|---|---|---|
| `queued_bytes` | بایت | measured | `available: false`، `reason: "not_sampled"` |
| `overflow_refusals` | شمارش | measured | `available: false`، `reason: "not_sampled"` |
| `guard_violations` | شمارش | measured | `available: false`، `reason: "not_instrumented"` — مسیر زمان‌بند `Guarded` ندارد، پس شمارنده‌اش ثابت است نه اندازه‌گیری |
| `queueing_delay_estimate_ms` | میلی‌ثانیه | estimated | `available: false`، `reason: "signal_blind"` — صف arbiter یک rendezvous است، پس انتظار سمت نوشتن قابل مشاهده نیست |

مقدار غایب را صفر نخوانید. `0` در این جدول یعنی «مشاهده شد و صفر بود»؛ `null` یعنی «مشاهده نشد».

## ۴. وقتی خواندن `active` نیست چه کنیم

| خواندن | اقدام اپراتور |
|---|---|
| `off` / `feature_not_compiled` | این باینری پک را اجرا نمی‌کند. پیش از انتظار هر حالتی از بیلد دارای `policy-pack` استفاده کنید؛ مشکل دیگری نیست. |
| `off` / `switch_absent` | سوئیچ تنظیم نشده است. `policy enable` همان drop-in مدیریت‌شده را می‌نویسد؛ پروفایل لازم نیست (`balanced/1` ردیف identity است). |
| `off` / `switch_off` | کسی عمداً سوئیچ را خاموش کرده است. فقط اگر همان قصد است دوباره روشن کنید. |
| `unknown` / `no_instance` | ایجنت به نمونهٔ تونل وصل نیست؛ `--instance` بدهید و از ایجنت نصب‌شده برای همان واحد استفاده کنید. |
| `unknown` / `no_running_evidence` | کانفیگ می‌گوید پک روشن است ولی واحد snapshot تازه منتشر نکرده؛ واحد را restart کنید (همان `restart_required` رسید) و دوباره بخوانید. |
| `unknown` / `stale_running_evidence` | snapshot از سه بازه کهنه‌تر است؛ واحد انتشار را متوقف کرده. پیش از اعتماد به هر فیلد running، زنده بودن واحد را بررسی کنید. |
| `unknown` / `unit_not_running` | واحد تونل را start کنید؛ تا اجرا نشود چیزی برای توصیف نیست. |
| `unknown` / `config_unreadable` | ایجنت نتوانست واحد یا drop-in را بخواند (یا `EnvironmentFile=` دارد). فایل‌ها را با root بررسی کنید؛ خواندن حدس نمی‌زند. |
| `unknown` / `switch_set_in_multiple_sources` | بیش از یک واحد/drop-in سوئیچ را تنظیم می‌کند. دقیقاً یکی را نگه دارید؛ `policy disable` فایلی را که مالکش نیست برنمی‌دارد. |
| در دسترس نیست / `no_policy_member` (پنل) | ایجنت پیش از این خواندن است. ایجنت را ارتقا دهید؛ CLI روی همان میزبان هم همین‌طور شکست می‌خورد. |
| در دسترس نیست / `status_error` (پنل)، خروج ۲ (CLI) | مسیر وضعیت در دسترس نیست یا توکن رد شد. اتصال/اعتبارنامه را درست کنید؛ هیچ حالتی گزارش نمی‌شود. |

## ۵. پنج پروفایل

همهٔ ردیف‌ها مقادیر طراحی پذیرفته‌شده‌اند (ADR-0031، نسخهٔ ۱). ستون شاهد همان برچسب مخزن است:
شاهد هارنس `simulated`، هرگز `measured`؛ معیار دو‌میزبانهٔ LAB برای هر چهار ردیف غیر Balanced باز
است.

| پروفایل | پنجره | Quantum | Cap | کاربرد | شاهد (همه `simulated`) |
|---|---|---|---|---|---|
| `balanced/1` | 256 KiB | 16 KiB | 64 KiB | پیش‌فرض و fail-safe؛ identity با بازوی زمان‌بند اندازه‌گیری‌شده | TP-2.2: هم‌سطح بازوی پنجره؛ بدون سود قابل‌تکرار |
| `gaming/1` | 256 KiB | 4 KiB | 16 KiB | grant و بافر کوچک‌تر تا انفجار bulk سهم کمتری از صف بگیرد | رکورد TP-3.2؛ loss/jitter/RSS/۶۰ دقیقه و tupleهای موتور واقعی اندازه‌گیری نشده |
| `trading/1` | 256 KiB | 2 KiB | 8 KiB | ریزترین درهم‌بافی ممکن؛ توان‌گذر عمداً فدا می‌شود | TP-3.3: سودی بر Balanced نشان نداد (p99 بین ۱.۵ تا ۲۱.۲٪ بالاتر؛ CPU حدود ۴.۴ برابر) |
| `streaming/1` | 512 KiB | 32 KiB | 128 KiB | تحویل پیوسته با نرخ متوسط و بافر ایستای بزرگ‌تر ولی کران‌دار | TP-3.4: p50/p95/p99 زیر بار ۲.۱ تا ۲.۲ برابر Balanced؛ CPU کمتر ولی تکرارها هم‌پوشانی ندارند؛ goodput تحویل‌شده اندازه‌گیری نشده |
| `bulk/1` | 2 MiB | 64 KiB | 256 KiB | توان‌گذر برای انتقال انبوه، تأخیر پذیرفته‌شده؛ نصف پنجرهٔ ۴ MiB مدیریت‌نشده | TP-3.5: p50/p95/p99 زیر بار ۸.۳ تا ۸.۹ برابر Balanced؛ CPU ۴۸.۹٪ کمتر؛ **goodput تحویل‌شده اندازه‌گیری نشده** |

هیچ پروفایلی «توصیه‌شده»، «پذیرفته‌شده به‌عنوان measured» یا «آماده» نیست. هر پروفایل را فقط به‌عنوان
قصد پیکربندی و پس از خواندن رکوردش انتخاب کنید و تا restart واحد انتظار `unknown` داشته باشید.
رکوردها: [TP-3.2](POLICY_PACK_TP-3.2_GAMING_EVIDENCE.md)،
[TP-3.3](POLICY_PACK_TP-3.3_TRADING_EVIDENCE.md)،
[TP-3.4](POLICY_PACK_TP-3.4_STREAMING_EVIDENCE.md) و
[TP-3.5](POLICY_PACK_TP-3.5_BULK_EVIDENCE.md).

## ۶. کنترل تطبیقی

`policy.adaptive` کنترلر ADR-0056 را فعال می‌کند. هر ثانیه شمارنده‌های arbiter خودِ نشست را نمونه
می‌گیرد، حداکثر یک پله بین پنج ردیف پذیرفته‌شده جابه‌جا می‌شود (با هیسترزیس، تداوم، dwell و محدودیت
نرخ)، هر تغییر را تأیید می‌کند و در شکست به ردیف پیکربندی‌شده بازمی‌گردد یا fail-safe می‌شود. بدون
پروفایل پیکربندی‌شده بی‌اثر است، پیش‌فرض خاموش است و این خواندن فقط آن را گزارش می‌کند — هرگز روشنش
نمی‌کند.

رکورد هارنسش ([TP-4.1](POLICY_PACK_TP-4.1_ADAPTIVE_EVIDENCE.md)) در سطح `simulated` است: حلقه
**۰ تغییر** داد، چون صف rendezvous سیگنال تأخیر را کور می‌کند (تخمین `0 ms` در هر نمونه)، پس هم‌سطح
ردیف ثابت ماند. ماتریس fail-safe ([TP-4.2](POLICY_PACK_TP-4.2_FAILSAFE_EVIDENCE.md)) هم `simulated`،
اجباری و قطعی است. هر دو منتظر همان پنجرهٔ دو‌میزبانهٔ LAB هستند که معلق است.

## ۷. کاستی‌های صادقانه

- **هیچ ادعای تأخیر مسیر یا فاصله نیست.** همهٔ اندازه‌گیری‌ها اجرای هارنس لوپ‌بک تک‌میزبانه در سطح
  `simulated`‌اند و دربارهٔ مسیر اینترنت، ISP یا چیدمان واقعی دو‌میزبانه چیزی نمی‌گویند.
- **فقط حامل TCP.** سود Gaming/Trading که به پرهیز از head-of-line blocking در TCP نیاز دارد در
  دسترس نیست؛ حامل UDP (#244) پیش‌نیازی جدا با ADR، audit و LAB خودش است.
- **موتور `baft` پوشش ندارد.** `unsupported` گزارش می‌شود و ردیف موتور Rust را به ارث نمی‌برد.
- **دو متریک ابزار اندازه‌گیری ندارند.** `guard_violations` روی مسیر زمان‌بند ثابت است و
  `queueing_delay_estimate_ms` انتظار سمت نوشتن را نمی‌بیند؛ شیء همین را می‌گوید و آن ثابت‌ها را
  به‌عنوان داده نشان نمی‌دهد.
- **متریک‌های صف** در این سطح امروز نمونه‌برداری نمی‌شوند (`not_sampled`).
- **اندازه‌گیری‌نشده‌ها:** loss، jitter/impairment، RSS، اجرای ۶۰ دقیقه‌ای، tupleهای موتور واقعی،
  goodput تحویل‌شده برای Streaming/Bulk و هر اثری بر جریان‌های تعاملی هم‌زمان.
- **پک پیش‌فرض خاموش است و هیچ پروفایلی measured پذیرفته نشده.** گیت `v0.4.0` شاهد دو‌میزبانهٔ هر
  tuple در TP-3.x به‌علاوهٔ پنجرهٔ دو‌میزبانهٔ TP-4.1/TP-4.2 است، هر دو معلق؛ این صفحه و رکوردهای
  هارنس جای آن‌ها را نمی‌گیرند.

## ۸. اسناد مرتبط

- [ADAPTIVE_TRAFFIC_POLICY_PACK.md](ADAPTIVE_TRAFFIC_POLICY_PACK.md) — نقشه و وضعیت آیتم‌به‌آیتم.
- [ADR-0031](ADR-0031-adaptive-traffic-policy-pack.md) — پروفایل‌ها، دامنهٔ موتور و تصمیم‌ها.
- [ADR-0047](ADR-0047-policy-status-contract.md) — قرارداد وضعیتی که این صفحه مستند می‌کند.
- [ADR-0057](ADR-0057-policy-operator-surface.md) — enable/disable/select/reset و رسیدهایش.
- [ADR-0056](ADR-0056-adaptive-optimization-controller.md) — کنترلر تطبیقی.
- [ADR-0054](ADR-0054-react-local-agent-panel.md) — پنل پیش‌فرض React و fallback کلاسیک که کارت
  policy ندارد.
