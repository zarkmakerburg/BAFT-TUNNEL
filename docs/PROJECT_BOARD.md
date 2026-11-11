# برد پروژه BAFT-TUNNEL — نمای کوتاه فارسی

> این فایل فقط نقطه ورود و خلاصه مدیریتی است. مرجع واحد فازها، اولویت‌ها و گام بعدی: [PRODUCT_ROADMAP.md](PRODUCT_ROADMAP.md). ارزیابی فنی تطبیق BAFT / Hashem / BCC با کد فعلی و دستورکار اجرایی Agent: [CROSS_PROJECT_CAPABILITY_ALIGNMENT.md](CROSS_PROJECT_CAPABILITY_ALIGNMENT.md). شواهد تاریخی دقیق و شناسه اجراها: [PHASE1_IMPLEMENTATION_STATUS.md](PHASE1_IMPLEMENTATION_STATUS.md). برد تصمیمات و گزارش عملیات: [HQ Issue #2](https://github.com/zarkmakerburg/BAFT-TUNNEL/issues/2).

## وضعیت در 2026-11-11

- **Baseline بررسی‌شده:** `main` در SHA `52a2825254f8254386f71af7220c7cb244200fc2` (ادغام U8 مستندات PR #165؛ U7 روی والد محصول `1219f547` اجرا شد).
- **U7:** PASS — هر سه Job در اجرای trusted [43063199428](https://github.com/zarkmakerburg/BAFT-TUNNEL/actions/runs/43063199428) موفق شدند؛ تست واقعی دو‌سروره روی RC.7 امضاشده تکمیل شد.
- **U8:** PASS — [PR #165](https://github.com/zarkmakerburg/BAFT-TUNNEL/pull/165) با CI کامل ادغام شد؛ راهنمای فارسی/انگلیسی و مسیر واقعی tcp-tls از Wizard مستند شدند.
- **وضعیت محصول:** توسعه فعال / Pre-release؛ **آماده Production نیست**. نسخهٔ امضاشدهٔ `v0.1.0-rc.7` منتشر شده؛ Release پایدار یا Deploy عملیاتی انجام نشده است.
- **گیت‌های P0:** هر پنج گیت با SHA و شناسه اجرا بسته شده‌اند؛ شواهد کامل در [HQ Issue #2](https://github.com/zarkmakerburg/BAFT-TUNNEL/issues/2) و [Roadmap](PRODUCT_ROADMAP.md).
  - **P0-1** (PR #88، helper سیستم‌دی): PR #88 ادغام شده؛ تست واقعی systemd 249 در اجرای 42523536944 (SHA `22e94f1`).
  - **P0-2:** رسید Apply با نشانگر Session مجاز (اجرای 42524638625) و اثبات دو‌میزبانه در P0-5؛ `ready` عمداً Unknown می‌ماند.
  - **P0-3** (Engine Store ← Launcher برای BAFT): اجراهای 42510577167 و 42522105698 با Asset امضاشده rc.2؛ از PR #133 ‏(`c7dbd15`) BAFT در Wizard به‌صورت Preview و فقط Direct دیده می‌شود، ولی Wizard هنوز نمی‌تواند پیکربندی آن را بسازد.
  - **P0-4** (Reverse R3): اجرای 42525790470 روی `5a72a25`؛ Reverse همچنان **Preview**.
  - **P0-5** (MVP یکپارچه): اجرای 42538458094 روی `92c4e70` با Prerelease امضاشده `v0.1.0-rc.4` روی هر دو میزبان LAB؛ یکپارچگی 64 KiB و 2 MiB تأیید شد.
- **حفاظت `main` (2026-11-09):** PR الزامی، ۰ تأییدیه، چک‌های الزامی `rust` و `ubuntu-2204-baseline` و `lab-config`، بدون force push و حذف؛ Bypass ادمین مجاز است.
- **نسخهٔ فعلی:** `v0.1.0-rc.7`؛ `rc.6` به‌دلیل هویت امضای نامعتبر تاریخی است و نباید استفاده شود. `rc.3` نیز به‌دلیل نیاز به glibc 2.39 روی Ubuntu 22.04 قابل استفاده نیست.
- **D2:** PR #150 ادغام شده؛ اجرای trusted LAB شمارهٔ [42996224181](https://github.com/zarkmakerburg/BAFT-TUNNEL/actions/runs/42996224181) چرخهٔ پنل تا PKI آفلاین و Start را روی دو میزبان با موفقیت تکرار کرد. Drift قبلی در این اجرا بازتولید نشد (`host_drift=none`)، اما منبع اصلی آن هنوز نامشخص است و در S10 می‌ماند.
- **TCP-TLS Direct:** مسیر پشتیبانی‌شده با شواهد واقعی LAB. **Reverse:** فقط Preview. **Noise-TCP:** Direct آزمایشی؛ Reverse اثبات‌نشده و غیرقابل انتخاب.
- **پنل:** رابط محلی سبک، فارسی RTL و انگلیسی LTR؛ Fleet مرکزی، آمار کامل ترافیک و Firewall inspection هنوز موجود نیست.

## مسیر اجرایی بعدی

موارد قبلی این بخش (Diagnostics و وضعیت End-to-End و شمارنده و Audit، ارزیابی امنیتی، Backup/Update، پذیرش Reverse و BAFT) انجام شده‌اند. ترتیب جدید (ارزیابی 2026-11-09؛ جزئیات در [REMAINING_WORK_PLAN.md](REMAINING_WORK_PLAN.md)):

۱. **Q1/Q2/Q3/Q4:** انجام شده‌اند؛ U8 در PR #165 ادغام شد.
۲. **U1:** انجام شده؛ اجرای [42656507521](https://github.com/zarkmakerburg/BAFT-TUNNEL/actions/runs/42656507521) تمدید CRL سی‌روزه، Push روی هر دو میزبان، افزایش شماره CRL و بازیابی تونل را ثابت کرد.
۳. **D2 / U5b:** انجام شده؛ PR #150 ادغام و اجرای تکراری trusted LAB شمارهٔ 42996224181 موفق شد.
۴. **U2/U3/U5a/U4/U6:** پیاده‌سازی شده‌اند؛ شواهد و جزئیات در Roadmap.
۵. **U7:** با اجرای [43063199428](https://github.com/zarkmakerburg/BAFT-TUNNEL/actions/runs/43063199428) پذیرفته شد؛ Soak دقیقاً ۳۶۰۰ ثانیه، ۲۶۴ دور و صفر Probe ناموفق بود. Backup/Restore، Update/Rollback، Revocation، Uninstall و پاک‌سازی نیز موفق شدند.
۶. **U8:** تکمیل و ادغام شد؛ [PR #165](https://github.com/zarkmakerburg/BAFT-TUNNEL/pull/165)، Commit ادغام `52a2825`؛ تمام CIها PASS.
۷. **B1:** کدهای ساختاریافته و ترجمه در [PR #168](https://github.com/zarkmakerburg/BAFT-TUNNEL/pull/168)؛ تمام چهار گروه CI روی Head `1272dcac` PASS شدند. فقط Merge پس از کنترل آخرین وضعیت باقی مانده است.
۸. **B2–B6:** پس از U8 و طبق Roadmap؛ بدون افزودن پروتکل یا Fleet قبل از تکمیل این گیت‌ها.

**موارد باز:** منشأ Drift شش‌خطی Address Set روی میزبان خارج در U7 (S10؛ هیچ قانون BAFT-TUNNEL وجود نداشت)، آزمون پنل روی گوشی واقعی، B1–B6 و گیت‌های انتشار. بازبینی امنیتی مستقل فعلاً به درخواست مالک متوقف است. Soak بیش از یک ساعت برنامه‌ریزی نمی‌شود. (بررسی حقوقی AGPL موتور BAFT حذف شد: BAFT متعلق به مالک است و استفاده از آن به‌صورت باینری جدا طبق ADR-0019 مجاز است؛ تأیید مالک 2026-11-09.)

جزئیات، معیارهای پذیرش و دستورالعمل Agent فقط در [سند فنی تطبیق قابلیت‌ها](CROSS_PROJECT_CAPABILITY_ALIGNMENT.md) و [Roadmap](PRODUCT_ROADMAP.md) نگهداری می‌شود. این برد عمداً وضعیت موازی جداگانه ایجاد نمی‌کند.

## قواعد ثابت

1. Secret، آدرس میزبان، کلید خصوصی، Token یا مقدار متغیر محرمانه در مستند عمومی/Log ثبت نشود.
2. بدون SHA دقیق و شاهد متناظر CI/LAB، وضعیت PASS ثبت نشود.
3. Unknown یا نبود داده هرگز Healthy تلقی نشود.
4. شبیه‌سازی، تست محلی، CI و LAB واقعی دو‌میزبانه از هم تفکیک شوند.
5. فقط منابعی پاک‌سازی شوند که مالکیت‌شان قابل اثبات است؛ تغییرات نامرتبط میزبان ممنوع است.
6. اجرای LAB روی میزبان‌های مشترک فقط یک Run در هر زمان.
7. Merge، Release و Deploy گیت‌های جداگانه‌اند؛ بسته‌شدن کد به‌تنهایی مجوز انتشار نیست.
