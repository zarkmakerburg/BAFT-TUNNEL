# ─── PROJECT ROADMAP ───

**Project:** BAFT-TUNNEL  
**Repository:** [zarkmakerburg/BAFT-TUNNEL](https://github.com/zarkmakerburg/BAFT-TUNNEL) — Private  
**HQ / Project Board:** [Issue #2](https://github.com/zarkmakerburg/BAFT-TUNNEL/issues/2)  
**Phase 1 PR:** [PR #3](https://github.com/zarkmakerburg/BAFT-TUNNEL/pull/3) — ادغام‌شده در Main  
**Current phase:** Phase 1 — Rust tunnel runtime, PKI, installer and LAB validation  
**Latest documented change:** Main `e913f34`-based head after PR #13/#14/#15 (آخرین SHA ادغام‌شده: `8bcafc9ff2f1c5f67d0f394f82bb5ffb8067cfa1`)  
**PR status:** PR #3 Merged (owner-approved)؛ PR #9/#10/#13/#14/#15 Merged؛ PR #16 (ADR-0017) باز و منتظر تصمیم مالک؛ PR #17 (Web Panel mockup) باز  
**Revised phased product plan (MVP, engines, Phase 2–6):** [PRODUCT_ROADMAP.md](PRODUCT_ROADMAP.md) — ADR-0017 و ADR-0018 توسط مالک تأیید شد.  
**Production status:** Not production-ready — فقط Pre-release `v0.1.0-phase1-pre` منتشر شده (با Workflow امضای Cosign در `release.yml`)

## Phase 0 — Foundation and Architecture
**هدف:** ایجاد زیرساخت مستقل، معماری امنیتی، مستندات، محیط LAB و قواعد توسعه.

- ایجاد مخزن خصوصی و مستقل از BAFT
- تعیین معماری Rust و مرزبندی اجزا
- تدوین Threat Model و Engineering Guardrails
- ایجاد محیط آزمایشگاهی دو سرور ایران و خارج
- تعریف معیارهای پذیرش و مدیریت شواهد
- تثبیت تصمیم‌های اولیه معماری و محصول

**وضعیت: تکمیل‌شده — PR #1 در Main ادغام شده است.**

## Phase 1 — Core Tunnel Runtime
**هدف:** ساخت هسته واقعی تونل با ارتباط امن، احراز هویت، کنترل دسترسی و نصب قابل‌اعتماد روی Linux.

- Rust CLI و Runtime
- TCP Transport با TLS 1.3 و Mutual TLS
- دو مسیر Direct و Reverse
- Multiplexing با Yamux
- احراز هویت هویت Node و مجوز Tunnel
- Offline PKI و Certificate Revocation
- Installer، systemd Service و Rollback
- پشتیبانی پیش‌فرض Ubuntu 22.04 / systemd 249
- تست واقعی دو سرور ایران و خارج
- تکمیل چرخه عملیاتی سرویس، تست پایداری و مدیریت CRL

**وضعیت: Phase 1 با استثناهای مستند بسته شد (P1-004، SHA `e4453bc`) — Pre-release منتشر شده؛ آماده Production نیست. استثناها: Soak چندساعته، اجرای واقعی `push-crl.sh` روی میزبان‌ها، بازبینی مستقل (Waived).**

## Phase 2 — Control Plane & Agent
**هدف:** ایجاد مدیریت متمرکز Nodeها، تنظیمات و وضعیت مطلوب سیستم.

- Agent و ارتباط امن با Control Plane
- مدیریت Desired State
- Reconciliation و تشخیص انحراف وضعیت
- API و مدیریت Nodeها و Tunnelها
- Signed Configuration و Update
- ثبت عملیات، Audit و Receipt
- Enrollment آنلاین فقط در صورت تصمیم معماری و مجوز جداگانه؛ در Phase 1 عمداً به‌صورت Offline باقی می‌ماند.

**وضعیت: برنامه‌ریزی‌شده؛ شروع پس از پذیرش Phase 1.**

## Phase 3 — Web Panel & Setup Wizard
**هدف:** ارائه محصولی قابل‌استفاده برای کاربر عادی، بدون نیاز به دانش تخصصی شبکه.

- Dashboard و Overview
- مدیریت Nodeها و Tunnelها
- Wizard نصب و راه‌اندازی
- Preflight و Configuration Preview
- Apply، Verify و Rollback
- Monitoring، Alert و Diagnostics
- مدیریت کاربران و سطح دسترسی
- طراحی Responsive و پشتیبانی فارسی RTL و انگلیسی LTR

**وضعیت: برنامه‌ریزی‌شده؛ شروع پس از تثبیت قراردادهای Phase 2.**

## Phase 4 — Advanced Transport & Reliability
**هدف:** توسعه قابلیت‌های پیشرفته انتقال و افزایش تاب‌آوری ارتباط.

- UDP و QUIC به‌عنوان Transportهای تکمیلی
- انتخاب مسیر و مدیریت اتصال
- Failover و Recovery
- مدیریت پروفایل‌ها
- بهبود عملکرد و مقیاس‌پذیری
- آزمون رفتار در شرایط اختلال شبکه

**وضعیت: برنامه‌ریزی‌شده.**

## Phase 5 — Hardening & Release
**هدف:** تکمیل الزامات عملیاتی، امنیتی و انتشار نسخه پایدار.

- بستن تمام Gateهای پذیرش
- بررسی امنیتی و وابستگی‌ها
- تست نصب، ارتقا، بازیابی و حذف
- تست پایداری طولانی‌مدت
- مستندات نهایی و Quickstart
- آماده‌سازی Artifact و امضای Release
- انتشار نسخه پایدار

**وضعیت: برنامه‌ریزی‌شده؛ وابسته به تکمیل فازهای قبلی.**

---

# ─── COMPREHENSIVE PROJECT BOARD ───

## 📋 BACKLOG (کارهای آینده و توسعه‌های بعدی)

- **[BL-001] Control Plane API:** ایجاد API متمرکز برای مدیریت Nodeها، Tunnelها و سیاست‌های عملیاتی.
- **[BL-002] Agent Reconciliation:** پیاده‌سازی Agent برای دریافت و تطبیق وضعیت مطلوب با وضعیت واقعی Node.
- **[BL-003] Signed Desired State:** طراحی و پیاده‌سازی تنظیمات امضاشده و قابل‌اعتبارسنجی.
- **[BL-004] Web Dashboard:** ساخت داشبورد جامع برای وضعیت سرورها، ارتباط‌ها، ترافیک و رخدادها.
- **[BL-005] Setup Wizard:** طراحی فرایند هدایت‌شده نصب، اتصال Nodeها و ساخت Tunnel.
- **[BL-006] User & Role Management:** مدیریت کاربران، نقش‌ها و مجوزهای عملیاتی.
- **[BL-007] Monitoring & Alerts:** پایش وضعیت، کیفیت ارتباط، هشدارها و تاریخچه رخدادها.
- **[BL-008] UDP Transport:** افزودن مسیر انتقال UDP پس از تثبیت قرارداد Transport.
- **[BL-009] QUIC Transport:** افزودن QUIC به‌عنوان Transport اختیاری.
- **[BL-010] Path Selection & Failover:** انتخاب مسیر مناسب و بازیابی اتصال در شرایط اختلال.
- **[BL-011] Backup & Restore:** طراحی نسخه پشتیبان و بازیابی امن تنظیمات و وضعیت.
- **[BL-012] Update Manager:** مدیریت نسخه‌ها، بررسی امضا، ارتقا و بازگشت نسخه.
- **[BL-013] Release Publishing:** امکان آماده‌سازی و انتشار GitHub Release از همین محیط ChatGPT.
- **[BL-014] Product UX Validation:** ارزیابی تجربه نصب و مدیریت با معیار سادگی C-Tunnel، بدون انتقال کد یا وابستگی آن.
- **[BL-015] Persian / English UX:** پشتیبانی کامل از رابط فارسی RTL و انگلیسی LTR.
- **[BL-016] Scale & Performance:** ارزیابی ظرفیت، مصرف منابع و رفتار در تعداد بالاتر Node و Tunnel.

## ⏳ TO DO (کارهای بعدی که باید انجام شوند)

- **[P1-002b] پیاده‌سازی تصمیم ADR-0017:** پس از تأیید مالک؛ A1 (توزیع CRL با Push اپراتور) و B1 (چرخش CA/Pin با دوره Overlap). اولویت: بالا
- **[P1-004] Phase 1 Acceptance Closure:** تطبیق شواهد تمام Gateها با آخرین SHA و بستن موارد باز؛ مستقل‌بودن بازبینی Waived است. اولویت: بالا
- **[P2-001] Dependency & Security Maintenance.** اولویت: متوسط
- **[P2-002] Operator Documentation.** اولویت: متوسط
- **[P2-003] Phase 2 Contracts.** اولویت: متوسط
- **[OPS-002] Import کنترل‌شده از BAFT:** طبق قاعده جدید مالک، ورود کد BAFT فقط پس از Health Check (امنیت، مجوز، وابستگی، تست) و با ثبت در HQ مجاز است.

## ⚡ IN PROGRESS (تسک‌های فعلی و در حال انجام)

- **[P1-001] Real-Host Soak & Network Impairment:** گزینه‌های `soak_seconds` و `impair` به Workflow دو-میزبان اضافه و در Main ادغام شد (PR #15). اجرای واقعی LAB با Soak ۱۸۰۰ ثانیه + Impairment **PASS** شد ([run 42398701622](https://github.com/zarkmakerburg/BAFT-TUNNEL/actions/runs/42398701622)، `8bcafc9`؛ ۱۲۸ دور، بدون رشد fd، RSS ثابت). توجه: Impairment فعلی در سطح Application است (تأخیر/Jitter و Restart پراکسی)، **نه Packet Loss واقعی TCP/NetEm** روی کارت شبکه؛ شبیه‌سازی Packet Loss واقعی هنوز انجام نشده.
- **[P1-002] CRL Distribution & Trust Rollover:** پیاده‌سازی شد؛ توالی Rollover روی LAB واقعی PASS. اجرای واقعی `push-crl.sh` روی میزبان‌های LAB انجام نشده.
- **[P1-004] Phase 1 Acceptance:** بسته با استثناهای مستند (Soak چندساعته، `push-crl.sh` واقعی، بازبینی مستقل Waived).
- **[P3-PREVIEW] Web Panel Mockup:** PR #17 (پیش‌نمایش UX؛ بدون Backend).
- **[OPS-001] LAB Readiness:** هر دو میزبان Ubuntu 22.04 / systemd 249؛ مجوز دائمی مالک برای آزمون‌های ایزوله و پاک‌سازی‌شده.

## ✅ DONE (کارهای تکمیل شده تا این لحظه)

### زیرساخت و معماری
- ایجاد مخزن خصوصی و مستقل `zarkmakerburg/BAFT-TUNNEL`.
- ایجاد HQ مرکزی در Issue #2 برای تصمیم‌ها، شواهد، عملیات و تحویل کار.
- ادغام Phase 0 از طریق PR #1 در Main.
- ثبت Engineering Guardrails و Threat Model.
- ثبت تصمیم Rust به‌عنوان زبان هسته و عدم استفاده از Go در Core.
- ثبت معماری Transport و مسیرهای Direct و Reverse.
- ثبت معیارهای تجربه کاربری و نصب در `docs/PRODUCT_UX_ACCEPTANCE.md`.
- ثبت تصمیم استفاده از C-Tunnel فقط به‌عنوان مرجع UX و نصب، بدون انتقال کد.

### هسته و انتقال داده
- ایجاد Rust CLI و Runtime.
- پیاده‌سازی نقش‌های Serve و Forward.
- پیاده‌سازی TCP Transport.
- پیاده‌سازی TLS 1.3 و Mutual TLS.
- پیاده‌سازی Yamux Multiplexing.
- پیاده‌سازی احراز هویت Node، Peer و Tunnel.
- محدودسازی مقصد Forwarding به IP و Port مشخص.
- پیاده‌سازی کنترل ظرفیت Session و Stream و محدودیت Frame.
- پیاده‌سازی Reconnect با Backoff و Jitter.
- پیاده‌سازی Shutdown کنترل‌شده با SIGTERM و Ctrl-C.
- افزودن آزمون‌های منفی برای Frameهای ناقص، بزرگ، تکراری و نامعتبر.

### امنیت و PKI
- پیاده‌سازی Offline CA و ابزار PKI در `scripts/pki/baft-pki.sh`.
- تولید کلید و CSR محلی برای Node.
- صدور Certificate با هویت URI SAN مشخص.
- اعتبارسنجی Certificate، CA و Fingerprint.
- الزام CRL برای هر دو نقش TLS Client و Server.
- رد Certificateهای Revoked و CRLهای منقضی یا نامعتبر.
- ادغام `key_credential` در Runtime و اعتبارسنجی Configuration.
- رد Credential مفقود، خالی، بیش‌ازحد بزرگ، Symlink و مسیر نامعتبر.
- ثبت ADR-0013 برای Node Key Custody.
- ثبت ADR-0014 و تصمیم نگه‌داشتن Enrollment آنلاین خارج از Phase 1.
- ثبت ADR-0015 برای اعتبارسنجی و فعال‌سازی کنترل‌شده CRL.
- پیاده‌سازی `scripts/pki/apply-crl.sh` برای بررسی و فعال‌سازی اتمیک CRL محلی همراه با Rollback.
- آزمون موفق فعال‌سازی CRL و بازیابی پس از Restart ناموفق در محیط systemd ایزوله.

### نصب و Systemd
- پیاده‌سازی `scripts/install.sh`.
- پیاده‌سازی `scripts/rollback.sh`.
- پیاده‌سازی `scripts/uninstall.sh`.
- ایجاد Unit Template در `systemd/baft-tunnel@.service`.
- پیاده‌سازی Hardening پایه سرویس.
- آزمون Staged Install، Backup، Rollback و Uninstall.
- آزمون Hosted Linux با systemd 255 برای `LoadCredentialEncrypted=`.
- آزمون Restart و تحویل مجدد Credential در اجرای Systemd.
- آزمون چرخه نصب و حذف ایزوله در CI.

### LAB و CI
- ایجاد LAB دو سرور ایران و خارج.
- آزمون واقعی Direct و Reverse با انتقال 2,097,289 بایت در هر جهت و تطبیق SHA-256.
- آزمون بازیابی Direct پس از Restart Listener.
- آزمون رد Certificate کلاینت و سرور Revoked.
- پاک‌سازی موفق منابع موقت آزمایش.
- تکمیل CI شامل Rust Tests، Clippy، Formatting، Release Build، Audit و License Checks در SHAهای ثبت‌شده.
- اجرای موفق آزمون‌های systemd و CRL در Hosted Ubuntu 24.04 / systemd 255.
- ثبت مالکیت و تأیید معافیت از بازبینی مستقل جداگانه برای پذیرش Phase 1 در HQ.
- به‌روزرسانی محافظه‌کارانه هر دو LAB Host بدون Distribution Upgrade و بدون Reboot.
- تأیید باقی‌ماندن هر دو میزبان روی Ubuntu 22.04 / systemd 249.

### تصمیم Ubuntu 22.04
- ثبت ADR-0016 برای پشتیبانی پیش‌فرض Ubuntu 22.04 / systemd 249.
- تعیین `LoadCredential=` به‌عنوان روش پایه.
- تعیین `LoadCredentialEncrypted=` به‌عنوان قابلیت اختیاری systemd 250+.
- ثبت شفاف اینکه روش پایه، رمزنگاری در حالت ذخیره‌شده نیست و بر کنترل دسترسی فایل متکی است.
- هماهنگ‌سازی راهنمای اپراتور، ADR-0013 و سند وضعیت Phase 1.

### به‌روزرسانی ۱۷ آبان ۱۴۰۵ / 2026-11-07
- **[P1-000]** مسیر Credential پایه `LoadCredential=` روی Ubuntu 22.04 / systemd 249: PR #9 ادغام شد (`f481000`)؛ CI میزبانی‌شده 22.04 سبز؛ آزمون واقعی روی هر دو میزبان LAB PASS ([run 42390940225](https://github.com/zarkmakerburg/BAFT-TUNNEL/actions/runs/42390940225), commit `263d237`). یافته: در systemd 249 مسیر `/run/credentials` برای `ExecStartPre=` در دسترس نیست و `config validate` پیش از شروع از Unit حذف شد.
- **PR #3** با تأیید صریح مالک در Main ادغام شد؛ Workflowهای موقت Secret/SSH در PR #10 حذف شدند.
- **Pre-release** `v0.1.0-phase1-pre` منتشر شد (غیر Production)، با `release.yml` و امضای Cosign.
- **یافته‌های بازبینی ۱، ۲، ۳، ۴، ۷** (PR #13، `21b9f92`): محدودیت Handshakeهای در انتظار (کل ۳۲، هر IP دو)، خطای Accept غیرکشنده با Backoff، Idle Timeout ۹۰۰ ثانیه برای Streamها، Listener فوروارد پیش‌فرض Loopback، هشدار انقضای CRL، و محدودیت‌های Unit (`TasksMax`, `MemoryMax`, `ProtectProc`, `ProcSubset`). CI سبز روی 24.04 و 22.04/249.
- **[P1-003]** پوشش Handshake خصمانه تکمیل شد (PR #14): Nonceهای مخدوش و فیلدهای ناشناخته؛ سایر موارد پیش‌تر پوشش داشتند.
- **قواعد جدید مالک:** مجوز Merge/Tag/Release/Pull؛ مجوز دائمی آزمون‌های LAB (ایزوله، پاک‌سازی‌شده، بدون Secret، بدون Production)؛ امکان ورود کد BAFT فقط پس از Health Check؛ بازبینی مستقل Waived.

---

# ─── DRILL-DOWN TASK DETAILS ───

## [P1-000] Ubuntu 22.04 Baseline Credential & Persistent Lifecycle

**وضعیت: تکمیل‌شده (CI 22.04 + LAB واقعی).**

- **Objective:**
  پشتیبانی عملیاتی و پیش‌فرض BAFT-TUNNEL روی Ubuntu 22.04 با systemd 249؛ بدون اجبار به ارتقای سیستم‌عامل یا systemd. مسیر استاندارد باید از `LoadCredential=` استفاده کند و کلید خصوصی را از مسیر Credential مخصوص سرویس به Runtime تحویل دهد.

- **Files Affected:**
  - `scripts/install.sh`
  - `scripts/rollback.sh`
  - `scripts/uninstall.sh`
  - `systemd/baft-tunnel@.service`
  - `src/key_custody.rs`
  - `src/config.rs`
  - `scripts/pki/test-systemd-credential-runtime.sh`
  - `scripts/pki/test-systemd-service-lifecycle.sh`
  - `scripts/test-install.sh`
  - `.github/workflows/phase1-rust.yml`
  - `docs/OPERATOR_GUIDE.md`
  - `docs/ADR-0013-node-key-custody.md`
  - `docs/ADR-0016-ubuntu-2204-systemd-compatibility.md`
  - `docs/PHASE1_IMPLEMENTATION_STATUS.md`

- **Step-by-Step Logic:**
  1. نسخه systemd و قابلیت‌های Credential را در مسیر نصب تشخیص بده؛ systemd 249 باید مسیر پایه و systemd 250+ امکان مسیر رمزنگاری‌شده اختیاری داشته باشد.
  2. مسیر Provisioning پایه را پیاده‌سازی کن: ساخت `/etc/baft-tunnel/credentials/INSTANCE` با مالکیت `root:root` و مجوز `0700`.
  3. کلید خصوصی را در فایل Credential با مالکیت `root:root` و مجوز `0600` نصب کن؛ هیچ Secretای در Environment، آرگومان CLI یا JSON قرار نگیرد.
  4. برای Instance انتخاب‌شده، Drop-in شامل `LoadCredential=node-key:/etc/baft-tunnel/credentials/INSTANCE/node-key` بساز.
  5. در Configuration فقط `key_credential: node-key` قرار بگیرد؛ فیلد `key` و `key_credential` هم‌زمان مجاز نباشند.
  6. Runtime از `CREDENTIALS_DIRECTORY` مسیر را دریافت کند و اعتبارسنجی فعلی را حفظ کند؛ نبود Credential باید Fail Closed شود.
  7. نصب و ارتقا باید Backup و Rollback امن داشته باشند؛ Uninstall نباید بدون درخواست صریح Credentialها و Configuration را حذف کند.
  8. تست Integration واقعی systemd 249 را اضافه کن؛ تست systemd 255 رمزنگاری‌شده را به‌عنوان قابلیت اختیاری حفظ کن.
  9. روی LAB با یک Instance ایزوله و منابع موقت، Start، Restart، Stop، Credential Permissions و Cleanup را آزمایش کن؛ سرویس و Tunnel فعال موجود نباید دست‌کاری شوند.
  10. نتایج دقیق و SHA تست‌ها را در HQ و `docs/PHASE1_IMPLEMENTATION_STATUS.md` ثبت کن.

- **Verification:**
  - روی Ubuntu 22.04 / systemd 249، Credential با `LoadCredential=` به سرویس تحویل شود.
  - Runtime بتواند کلید را از `CREDENTIALS_DIRECTORY` بخواند و Configuration را Validate کند.
  - فایل منبع فقط در اختیار Root باشد و مجوزهای دایرکتوری و فایل دقیقاً بررسی شوند.
  - حذف Credential، خراب‌بودن فایل یا نبود `CREDENTIALS_DIRECTORY` باعث شکست امن شود.
  - سرویس Start، Restart و Stop موفق داشته باشد.
  - Rollback و Uninstall منابع متعلق به تست را به‌درستی پاک کنند و داده‌های نامرتبط حفظ شوند.
  - CI روی هر دو مسیر systemd 249 و systemd 255 نتیجه جداگانه داشته باشد.
  - هیچ تستی صرفاً به‌دلیل موفقیت Hosted Ubuntu 24.04 به‌عنوان اثبات Ubuntu 22.04 ثبت نشود.

## [P1-001] Real-Host Soak & Network Impairment

- **Objective:**
  اثبات پایداری واقعی Direct و Reverse در بازه طولانی و هنگام افزایش Latency، Packet Loss، قطع ارتباط و بازیابی؛ با معیارهای پذیرش عددی و قابل تکرار.

- **Files Affected:**
  - `lab/real-hosts/phase1-two-host-e2e.sh`
  - `lab/real-hosts/test-phase1-two-host-e2e.sh`
  - `lab/real-hosts/preflight-two-hosts.sh`
  - `lab/real-hosts/.env.example`
  - `.github/workflows/phase1-two-host-e2e.yml`
  - `docs/REAL_HOSTS_LAB.md`
  - `docs/PHASE1_IMPLEMENTATION_STATUS.md`

- **Step-by-Step Logic:**
  1. پروفایل Soak را با مدت اجرا، نرخ انتقال، تعداد اتصال، Latency و Packet Loss تعریف کن.
  2. معیارهای موفقیت را پیش از اجرا مشخص کن: Integrity، نرخ خطا، Recovery، مصرف منابع و رفتار Session.
  3. اجرای Direct و Reverse را روی همان دو میزبان Ubuntu 22.04 انجام بده.
  4. انتقال داده در هر دو جهت را با Hash بررسی کن؛ صرفاً موفقیت اتصال را معیار سلامت قرار نده.
  5. اختلال کنترل‌شده را فقط در محدوده منابع و مسیرهای تست اعمال کن و قبل و بعد از آزمون وضعیت شبکه را ثبت کن.
  6. قطع و بازگشت ارتباط، Restart Listener و بازیابی Tunnel را آزمایش کن.
  7. پس از پایان، تمام Ruleها و منابع موقت متعلق به تست را پاک‌سازی و پاک‌شدن آن‌ها را تأیید کن.
  8. نتایج واقعی را از Simulation و NetEm جدا نگه دار و در HQ ثبت کن.

- **Verification:**
  - Hash داده در هر دو مسیر بدون تغییر باشد.
  - معیارهای از پیش تعیین‌شده برای Recovery و پایداری رعایت شوند.
  - هیچ Session یا Process آزمایشی پس از Cleanup باقی نماند.
  - وضعیت سرویس‌های نامرتبط و تنظیمات دائمی میزبان تغییر نکرده باشد.
  - خروجی شامل SHA، مدت اجرا، شرایط شبکه و نتیجه هر سناریو باشد.

## [P1-002] CRL Distribution & Trust Rollover

- **Objective:**
  تبدیل مدیریت فعلی CRL از انتقال و فعال‌سازی دستی به چرخه‌ای امن، قابل‌ردیابی و قابل‌بازیابی؛ همراه با طراحی چرخش CA و Certificate Pin بدون ایجاد شکاف اعتماد.

- **Files Affected:**
  - `scripts/pki/baft-pki.sh`
  - `scripts/pki/apply-crl.sh`
  - `scripts/pki/test-apply-crl.sh`
  - `scripts/pki/test-crl-activation-systemd.sh`
  - `src/tls.rs`
  - `src/config.rs`
  - `src/authorization.rs`
  - `docs/ADR-0015-crl-update-and-rollover.md`
  - `docs/SECURITY_GUIDE.md`
  - `docs/OPERATOR_GUIDE.md`
  - `docs/PHASE1_IMPLEMENTATION_STATUS.md`

- **Step-by-Step Logic:**
  1. قرارداد انتشار CRL را با نسخه، شماره یکتا، زمان اعتبار، صادرکننده و امضای قابل‌اعتبارسنجی تعریف کن.
  2. روش انتقال احراز‌شده CRL به Nodeها را طراحی کن؛ انتشار نباید به CA خصوصی آنلاین یا قرارگرفتن کلید CA روی Node نیاز داشته باشد.
  3. دریافت CRL را با بررسی امضا، CA، شماره نسخه و زمان اعتبار انجام بده.
  4. تازه‌سازی CRL را اتمیک انجام بده و در صورت داده نامعتبر یا منقضی‌شده Fail Closed باقی بمان.
  5. وضعیت تازه‌بودن CRL و نتیجه فعال‌سازی را قابل مشاهده و ثبت کن.
  6. سناریوی CA و Pin Rollover را با دوره Overlap، اعتبارسنجی دوگانه و مسیر Rollback طراحی کن.
  7. رفتار Rollback را از نظر امنیتی بررسی کن؛ بازگرداندن CRL قدیمی ممکن است Certificate قبلاً Revoked را دوباره معتبر کند.
  8. تست‌های Replay، CRL قدیمی، CRL جعلی، قطع ارتباط، شکست Restart و Rollback را اضافه کن.
  9. هر تغییر اعتماد را به‌صورت مرحله‌ای و قابل‌ردیابی روی LAB آزمایش کن.

- **Verification:**
  - CRL با امضای اشتباه یا CA نامعتبر پذیرفته نشود.
  - CRL قدیمی، تکراری یا منقضی‌شده فعال نشود.
  - شماره CRL در هر به‌روزرسانی معتبر افزایش یابد.
  - شکست فعال‌سازی باعث باقی‌ماندن فایل ناقص یا وضعیت مبهم نشود.
  - Certificate لغوشده در هر دو سمت TLS رد شود.
  - CA و Pin Rollover در حالت Overlap و Rollback با شواهد قابل تکرار آزمایش شود.
  - هیچ کلید خصوصی CA به Node یا CI منتقل نشود.

## [P1-003] Protocol Adversarial Coverage

**وضعیت: تکمیل‌شده (PR #13 و #14).**

- **Objective:**
  کاهش ریسک خطاهای پروتکل، Replay، مصرف بی‌رویه منابع و شکست‌های هم‌زمان در مرزهای امنیتی و عملیاتی Runtime.

- **Files Affected:**
  - `src/handshake.rs`
  - `src/authorization.rs`
  - `src/tunnel.rs`
  - `src/tls.rs`
  - `src/config.rs`
  - `src/key_custody.rs`
  - `Cargo.toml`
  - `Cargo.lock`
  - `.github/workflows/phase1-rust.yml`
  - `docs/PHASE1_IMPLEMENTATION_STATUS.md`

- **Step-by-Step Logic:**
  1. مرزهای ورودی و نقاط تخصیص منابع را بر اساس کد موجود مشخص کن.
  2. پوشش فعلی Frame، Identity، Certificate، CRL و Authorization را بررسی کن تا تست تکراری ساخته نشود.
  3. فقط برای نقاط فاقد پوشش، تست Fuzz یا Property-based هدفمند اضافه کن.
  4. سناریوهای Replay، ورودی ناقص، فیلدهای تکراری، Session Exhaustion و اتصال هم‌زمان را بررسی کن.
  5. رفتار قطع ناگهانی Carrier و بسته‌شدن Streamها را بررسی کن.
  6. هر نقص قابل‌بازتولید را ابتدا با تست شکست‌خورده ثبت و سپس اصلاح کن.
  7. تست‌های ضروری را در CI موجود ادغام کن؛ از ساخت Workflowهای تکراری و غیرضروری اجتناب کن.

- **Verification:**
  - ورودی نامعتبر باعث Crash یا Downgrade امنیتی نشود.
  - محدودیت حافظه، Frame، Stream و Connection رعایت شود.
  - Replay یا هویت و مجوز نامعتبر پذیرفته نشود.
  - پس از قطع ارتباط، منابع آزاد و وضعیت Session قابل‌بازیابی باشد.
  - CI و گزارش تست به SHA دقیق متصل باشند.

---

# ─── CURRENT ACCEPTANCE GATES ───

| Gate | وضعیت | توضیح |
|---|---|---|
| Phase 0 Foundation | تکمیل | PR #1 |
| Rust TCP Runtime / TLS 1.3 mTLS / Offline PKI | پیاده‌سازی‌شده | Direct و Reverse |
| Two-host E2E | PASS | شواهد واقعی Direct/Reverse و Revocation |
| Hosted CI | PASS | Ubuntu 24.04 و Ubuntu 22.04 / systemd 249 |
| Ubuntu 22.04 Credential (P1-000) | تکمیل | CI + LAB واقعی |
| Protocol adversarial coverage (P1-003) | تکمیل | PR #13/#14 |
| Soak ۳۰ دقیقه‌ای (P1-001) | PASS | run 42398701622؛ Soak طولانی‌تر (ساعت‌ها) هنوز انجام نشده |
| Controlled impairment (P1-001) | **انجام شد** | Packet Loss واقعی ۲٪ با nftables PASS (run 42412456810) و Soak ۳۶۰۰ ثانیه PASS (run 42412781202) |
| Remote CRL distribution (P1-002) | پیاده‌سازی شد (تست Mock) | `push-crl.sh` + تست؛ اجرای واقعی LAB هنوز انجام نشده |
| CA / Pin rollover (P1-002) | **انجام شد** | توالی کامل ADR-0017 B1 روی LAB واقعی PASS (run 42447019410): Overlap، گواهی CA جدید، حذف CA قدیمی و رد کلاینت قدیمی |
| Independent review | Waived | معافیت مالک؛ انجام نشده |
| Noise+TCP (P2-004) | PASS (Direct) | run 42445109749؛ Reverse و Loss روی Noise تست نشده |
| Phase 1 acceptance (P1-004) | **بسته با استثنا** | شواهد با SHA `e4453bc`؛ استثناهای مستند در Status |
| PR #3 | Merged | با تأیید مالک |
| Pre-release | منتشر شده | `v0.1.0-phase1-pre` |
| Production Release | انجام نشده | آماده Production نیست |

## شواهد و مراجع اصلی

- [HQ Control Board — Issue #2](https://github.com/zarkmakerburg/BAFT-TUNNEL/issues/2)
- [Phase 1 PR #3](https://github.com/zarkmakerburg/BAFT-TUNNEL/pull/3)
- [Ubuntu 22.04 Decision — ADR-0016](https://github.com/zarkmakerburg/BAFT-TUNNEL/blob/phase1/rust-tcp-transport/docs/ADR-0016-ubuntu-2204-systemd-compatibility.md)
- [Phase 1 Implementation Status](https://github.com/zarkmakerburg/BAFT-TUNNEL/blob/phase1/rust-tcp-transport/docs/PHASE1_IMPLEMENTATION_STATUS.md)
- [Operator Guide](https://github.com/zarkmakerburg/BAFT-TUNNEL/blob/phase1/rust-tcp-transport/docs/OPERATOR_GUIDE.md)
- [CRL Lifecycle Decision — ADR-0015](https://github.com/zarkmakerburg/BAFT-TUNNEL/blob/phase1/rust-tcp-transport/docs/ADR-0015-crl-update-and-rollover.md)
- [Product UX Acceptance](https://github.com/zarkmakerburg/BAFT-TUNNEL/blob/phase1/rust-tcp-transport/docs/PRODUCT_UX_ACCEPTANCE.md)
- [Two-host E2E Run](https://github.com/zarkmakerburg/BAFT-TUNNEL/actions/runs/42358624201)
- [Latest recorded LAB upgrade](https://github.com/zarkmakerburg/BAFT-TUNNEL/actions/runs/42382280375)
- [Owner waiver for independent review](https://github.com/zarkmakerburg/BAFT-TUNNEL/issues/2#issuecomment-6708145366)
- [Ubuntu 22.04 baseline decision and implementation status update](https://github.com/zarkmakerburg/BAFT-TUNNEL/issues/2#issuecomment-6708562233)

## قواعد ثابت اجرای پروژه

1. پروژه فعال فقط `BAFT-TUNNEL` است. ورود هر کد از BAFT فقط پس از Health Check و ثبت در HQ مجاز است؛ هیچ Secret یا تاریخچه‌ای منتقل نشود.
2. Ubuntu 22.04 / systemd 249 مبنای پشتیبانی است؛ ارتقای سیستم‌عامل یا systemd مجاز نیست.
3. مجوز Merge، Tag، Release و Pull از مالک صادر شده است؛ انتشار فقط به‌صورت Pre-release و بدون ادعای Production.
4. مجوز دائمی آزمون‌های LAB: ایزوله، پاک‌سازی‌شده، بدون Secret، بدون دست‌زدن به Production یا سرویس‌های نامرتبط.
5. هر نتیجه به Commit SHA و لینک CI یا LAB متصل باشد؛ Simulation، Hosted CI و Real-host سه نوع مدرک جداگانه‌اند.
6. هیچ ادعای تکمیل بدون شواهد؛ بازبینی مستقل Waived است، نه انجام‌شده.
7. تغییر وضعیت در HQ Issue #2 و `docs/PHASE1_IMPLEMENTATION_STATUS.md` ثبت شود.
8. Sonnet 5.5، بدون اسکن کامل مخزن، پاسخ فارسی و کوتاه.

**گام بعدی:** نتیجه Soak/Impairment، تصمیم مالک درباره ADR-0017، سپس P1-004.
