# برد پروژه BAFT-TUNNEL (نمای کوتاه فارسی)

> **این فایل فقط اشاره‌گر است.** منبع واحد وضعیت فازها، ماتریس Engineها، مسیر تا MVP و گام بعدی: [`PRODUCT_ROADMAP.md`](PRODUCT_ROADMAP.md). شواهد (شناسه اجرا و SHA): [`PHASE1_IMPLEMENTATION_STATUS.md`](PHASE1_IMPLEMENTATION_STATUS.md). اگر این فایل با رودمپ فرق داشت، رودمپ درست است. هیچ وضعیت یا شمارهٔ PR جاری در این فایل نگهداری نمی‌شود.

- **مخزن:** [zarkmakerburg/BAFT-TUNNEL](https://github.com/zarkmakerburg/BAFT-TUNNEL)
- **برد/HQ:** [Issue #2](https://github.com/zarkmakerburg/BAFT-TUNNEL/issues/2)
- **وضعیت محصول:** آماده Production نیست؛ فقط Pre-release `v0.1.0-phase1-pre` منتشر شده است.

## گام بعدی (به ترتیب؛ جزئیات و معیار پذیرش در رودمپ)

1. **M0:** تست LAB واقعی Reverse برای `noise-tcp`. تا پاس‌شدن، Noise در Wizard پنهان می‌ماند.
2. **P3-001:** `setup` CLI؛ برنامه و معیارهای عددی در [`PLAN-P3-001-setup-cli.md`](PLAN-P3-001-setup-cli.md).
3. بعد از آن: Agent و API محلی، پنل واقعی، تست Upgrade/Rollback، Fuzz، پذیرش یکپارچه LAB، سپس v0.2.0.

## فازها (خلاصه؛ وضعیت دقیق فقط در رودمپ)

| فاز | موضوع |
|---|---|
| 0 | زیرساخت و مستندات |
| 1 | هستهٔ Runtime، PKI، نصب‌کننده، LAB (بسته‌شده با استثناهای مستند) |
| 2 | چارچوب Engine (Trait انجام شده؛ Noise+TCP پشت Trait) |
| 3 | Agent، API محلی، `setup` CLI |
| 4 | پنل وب و Wizard |
| 5 | Camouflage و ترنسپورت‌های بیشتر |
| 6 | سخت‌سازی و نسخه پایدار |

## قواعد ثابت

1. هرگز Secret یا آدرس میزبان در لاگ یا مستندات عمومی نیاید. کد BAFT/C-BAFT فقط پس از بررسی (بدون Secret) وارد شود.
2. پایهٔ پیش‌فرض Ubuntu 22.04 / systemd 249؛ بدون ارتقای OS یا systemd.
3. بدون SHA و شاهد CI یا LAB ادعای تکمیل نشود؛ شبیه‌سازی، CI میزبانی‌شده و LAB واقعی شواهد متفاوت‌اند.
4. تغییرات LAB ایزوله و پاک‌شونده باشد.
5. یک تسک کوچک در هر زمان؛ فقط فایل‌های همان تسک تغییر کند.
6. هر PR که وضعیت را عوض می‌کند، همان‌جا رودمپ را به‌روز می‌کند و این فایل را دست نمی‌زند مگر گام بعدی تغییر کند.
