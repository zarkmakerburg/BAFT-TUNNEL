# برد پروژه BAFT-TUNNEL — نمای کوتاه فارسی

> این فایل فقط نقطه ورود و خلاصه مدیریتی است. مرجع واحد فازها، اولویت‌ها و گام بعدی: [PRODUCT_ROADMAP.md](PRODUCT_ROADMAP.md). ارزیابی فنی تطبیق BAFT / Hashem / BCC با کد فعلی و دستورکار اجرایی Agent: [CROSS_PROJECT_CAPABILITY_ALIGNMENT.md](CROSS_PROJECT_CAPABILITY_ALIGNMENT.md). شواهد تاریخی دقیق و شناسه اجراها: [PHASE1_IMPLEMENTATION_STATUS.md](PHASE1_IMPLEMENTATION_STATUS.md). برد تصمیمات و گزارش عملیات: [HQ Issue #2](https://github.com/zarkmakerburg/BAFT-TUNNEL/issues/2).

## وضعیت در 2026-11-08

- **Baseline بررسی‌شده:** `main` در SHA `05a2b8078cc7afae751520209dba7fd474914bfb` (PR #86).
- **وضعیت محصول:** توسعه فعال / Pre-release؛ آماده Production نیست.
- **TCP-TLS Direct:** مسیر پشتیبانی‌شده با شواهد واقعی LAB.
- **TCP-TLS Reverse:** پیاده‌سازی واقعی و یک تست دو‌میزبانه موفق؛ فقط **Preview**، نه Stable.
- **Noise-TCP:** Direct آزمایشی؛ Reverse همچنان اثبات‌نشده و غیرقابل انتخاب.
- **BAFT Engine:** پردازش Go مستقل و Hash-pinned؛ اتصال Engine Store به Launcher در کد main ادغام شده، ولی مسیر Store به‌طور مستقل در LAB واقعی اثبات نشده و موتور در Wizard مخفی است.
- **Apply در main:** فقط مراحل محلی `init` و `join` با Verify، Receipt و rollback محدود.
- **PR #88:** افزودن مرحله Start از طریق systemd helper؛ باز و Merge نشده و هنوز تست واقعی systemd ندارد.
- **پنل:** رابط محلی سبک، فارسی RTL و انگلیسی LTR؛ Fleet مرکزی، اتصال احراز‌شده همتا، آمار کامل ترافیک و Firewall inspection هنوز موجود نیست.

## مسیر اجرایی بعدی

1. تکمیل بررسی امنیتی و LAB واقعی PR #88 روی Ubuntu 22.04 / systemd 249.
2. تکمیل قرارداد وضعیت اتصال: تفکیک Listener، TCP Reachability، احراز هویت همتا، Session و Data Path.
3. تست واقعی Engine Store → Launcher برای BAFT با Manifest و SHA دقیق.
4. تکمیل شواهد R3 برای TCP-TLS Reverse؛ حفظ Preview تا پذیرش.
5. پذیرش یکپارچه با Signed Artifact روی دو میزبان، سپس تصمیم درباره نسخه MVP.

جزئیات، معیارهای پذیرش و دستورالعمل Agent فقط در [سند فنی تطبیق قابلیت‌ها](CROSS_PROJECT_CAPABILITY_ALIGNMENT.md) و [Roadmap](PRODUCT_ROADMAP.md) نگهداری می‌شود. این برد عمداً وضعیت موازی جداگانه ایجاد نمی‌کند.

## قواعد ثابت

1. Secret، آدرس میزبان، کلید خصوصی، Token یا مقدار متغیر محرمانه در مستند عمومی/Log ثبت نشود.
2. بدون SHA دقیق و شاهد متناظر CI/LAB، وضعیت PASS ثبت نشود.
3. Unknown یا نبود داده هرگز Healthy تلقی نشود.
4. شبیه‌سازی، تست محلی، CI و LAB واقعی دو‌میزبانه از هم تفکیک شوند.
5. فقط منابعی پاک‌سازی شوند که مالکیت‌شان قابل اثبات است؛ تغییرات نامرتبط میزبان ممنوع است.
6. اجرای LAB روی میزبان‌های مشترک فقط یک Run در هر زمان.
7. Merge، Release و Deploy گیت‌های جداگانه‌اند؛ بسته‌شدن کد به‌تنهایی مجوز انتشار نیست.
