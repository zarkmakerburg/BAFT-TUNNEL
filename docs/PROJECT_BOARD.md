# برد پروژه BAFT-TUNNEL — نمای کوتاه فارسی

> این فایل فقط نقطه ورود و خلاصه مدیریتی است. مرجع واحد فازها، اولویت‌ها و گام بعدی: [PRODUCT_ROADMAP.md](PRODUCT_ROADMAP.md). ارزیابی فنی تطبیق BAFT / Hashem / BCC با کد فعلی و دستورکار اجرایی Agent: [CROSS_PROJECT_CAPABILITY_ALIGNMENT.md](CROSS_PROJECT_CAPABILITY_ALIGNMENT.md). شواهد تاریخی دقیق و شناسه اجراها: [PHASE1_IMPLEMENTATION_STATUS.md](PHASE1_IMPLEMENTATION_STATUS.md). برد تصمیمات و گزارش عملیات: [HQ Issue #2](https://github.com/zarkmakerburg/BAFT-TUNNEL/issues/2).

## وضعیت در 2026-11-09

- **Baseline بررسی‌شده:** `main` در SHA `92b0645f8104694bbe9d016e24f7ec0c8f43eeb0` (PR #106).
- **وضعیت محصول:** توسعه فعال / Pre-release؛ **آماده Production نیست.** Tag پایدار، Release پایدار یا Deploy انجام نشده است.
- **گیت‌های P0:** هر پنج گیت با SHA و شناسه اجرا بسته شده‌اند؛ شواهد کامل در [HQ Issue #2](https://github.com/zarkmakerburg/BAFT-TUNNEL/issues/2) و [Roadmap](PRODUCT_ROADMAP.md).
  - **P0-1** (PR #88، helper سیستم‌دی): PR #88 ادغام شده؛ تست واقعی systemd 249 در اجرای 42523536944 (SHA `22e94f1`).
  - **P0-2:** رسید Apply با نشانگر Session مجاز (اجرای 42524638625) و اثبات دو‌میزبانه در P0-5؛ `ready` عمداً Unknown می‌ماند.
  - **P0-3** (Engine Store ← Launcher برای BAFT): اجراهای 42510577167 و 42522105698 با Asset امضاشده rc.2؛ BAFT هنوز در Wizard مخفی است.
  - **P0-4** (Reverse R3): اجرای 42525790470 روی `5a72a25`؛ Reverse همچنان **Preview**.
  - **P0-5** (MVP یکپارچه): اجرای 42538458094 روی `92c4e70` با Prerelease امضاشده `v0.1.0-rc.4` روی هر دو میزبان LAB؛ یکپارچگی 64 KiB و 2 MiB تأیید شد.
- **حفاظت `main` (2026-11-09):** PR الزامی، ۰ تأییدیه، چک‌های الزامی `rust` و `ubuntu-2204-baseline` و `lab-config`، بدون force push و حذف؛ Bypass ادمین مجاز است.
- **rc.3 منسوخ است** (نیازمند glibc 2.39؛ روی Ubuntu 22.04 اجرا نمی‌شود)؛ rc.4 جایگزین آن است.
- **هشدار:** روی میزبان خارج از کشور Drift نامرتبط Firewall دیده شد؛ فقط «بدون تغییر Firewall مربوط به BAFT» ادعا می‌شود، نه «بدون هیچ اثر Firewall».
- **TCP-TLS Direct:** مسیر پشتیبانی‌شده با شواهد واقعی LAB. **Reverse:** فقط Preview. **Noise-TCP:** Direct آزمایشی؛ Reverse اثبات‌نشده و غیرقابل انتخاب.
- **پنل:** رابط محلی سبک، فارسی RTL و انگلیسی LTR؛ Fleet مرکزی، آمار کامل ترافیک و Firewall inspection هنوز موجود نیست.

## مسیر اجرایی بعدی

۱. بازبینی مالک روی شواهد P0؛ P1/P2 تا آن زمان متوقف است.
۲. حفاظت `main` برقرار شد؛ سیاست Merge (تأییدیه‌ها، Bypass ادمین) در بازبینی مالک نهایی شود.
۳. اجرای CI فاز ۱ روی SHA دقیق `main`.
۴. تکمیل P1: Diagnostics واقعی، وضعیت End-to-End، شمارنده ترافیک و Audit.
۵. ارزیابی امنیتی مستقل، تست Backup/Restore و Update/Rollback.
۶. پذیرش جداگانه Reverse و BAFT پیش از نمایش در محصول.

جزئیات، معیارهای پذیرش و دستورالعمل Agent فقط در [سند فنی تطبیق قابلیت‌ها](CROSS_PROJECT_CAPABILITY_ALIGNMENT.md) و [Roadmap](PRODUCT_ROADMAP.md) نگهداری می‌شود. این برد عمداً وضعیت موازی جداگانه ایجاد نمی‌کند.

## قواعد ثابت

1. Secret، آدرس میزبان، کلید خصوصی، Token یا مقدار متغیر محرمانه در مستند عمومی/Log ثبت نشود.
2. بدون SHA دقیق و شاهد متناظر CI/LAB، وضعیت PASS ثبت نشود.
3. Unknown یا نبود داده هرگز Healthy تلقی نشود.
4. شبیه‌سازی، تست محلی، CI و LAB واقعی دو‌میزبانه از هم تفکیک شوند.
5. فقط منابعی پاک‌سازی شوند که مالکیت‌شان قابل اثبات است؛ تغییرات نامرتبط میزبان ممنوع است.
6. اجرای LAB روی میزبان‌های مشترک فقط یک Run در هر زمان.
7. Merge، Release و Deploy گیت‌های جداگانه‌اند؛ بسته‌شدن کد به‌تنهایی مجوز انتشار نیست.
