# BAFT-TUNNEL — شروع سریع فاز ۱

> نسخه آزمایشی توسعه است؛ فقط روی Loopback یا سرورهای تستِ قابل‌حذف اجرا شود. هنوز آماده استفاده عملیاتی نیست.

## پیش‌نیازها

- Linux یا macOS و Rust نسخه 1.85 یا جدیدتر.
- برای هر نود یک گواهی صادرشده از CA خصوصی؛ گواهی باید URI SAN دقیق `urn:baft-tunnel:node:<node-id>` داشته باشد.
- گواهی سرور علاوه بر URI SAN باید DNS/IP SAN مطابق مقدار `--server-name` داشته باشد.
- هر دو سمت باید CRL معتبر و امضاشده توسط همان CA را داشته باشند. CRL مفقود، منقضی یا نامعتبر باعث رد اتصال می‌شود.
- اثرانگشت SHA-256 گواهی هر نود را خارج از کانال تبادل کنید.

## ساخت

```bash
cargo build --release
```

## آماده‌سازی گواهی‌ها

راهنمای [صدور آفلاین گواهی‌ها](OPERATOR_GUIDE.md) را دنبال کنید. کلید خصوصی CA باید رمزگذاری‌شده و آفلاین بماند. کلید خصوصی هر نود فعلاً فایل PEM بدون رمز با مجوز دسترسی `0600` است؛ هرگز آن را از نود خارج نکنید.

## اجرای Direct

روی سرور مقصد خارج:

```bash
./target/release/baft-tunnel serve   --listen 0.0.0.0:9443 --target 127.0.0.1:8080   --local-id node-abroad --peer-id node-iran --tunnel-id demo-tunnel --direction direct   --peer-fingerprint FINGERPRINT_IRAN   --ca ./pki/ca.crt --crl ./pki/ca.crl --cert ./pki/abroad.crt --key ./pki/abroad.key
```

روی نود ایران:

```bash
./target/release/baft-tunnel forward   --listen 127.0.0.1:18080 --remote 203.0.113.10:9443 --server-name tunnel.abroad.example.test   --local-id node-iran --peer-id node-abroad --tunnel-id demo-tunnel --direction direct   --peer-fingerprint FINGERPRINT_ABROAD   --ca ./pki/ca.crt --crl ./pki/ca.crl --cert ./pki/iran.crt --key ./pki/iran.key
```

## اجرای Reverse

برای Reverse جای دو نقش را عوض کنید: `serve` روی ایران و `forward` روی خارج اجرا شود. مقدار `--direction reverse` باید روی هر دو سمت یکسان باشد.

## محدودیت‌های فعلی

- مقصد TCP ثابت و توسط اپراتور تعیین می‌شود؛ همتا نمی‌تواند مقصد دلخواه انتخاب کند.
- احراز هویت TLS 1.3/mTLS، اثرانگشت، هویت SAN، جهت تونل و مجوز هر Stream جداگانه بررسی می‌شوند.
- CRL به‌صورت خودکار توزیع یا hot-reload نمی‌شود؛ پس از انتشار CRL جدید، هر دو فرایند را Restart کنید.
- در قطع مسیر، Streamهای فعال از نو ادامه پیدا نمی‌کنند؛ اتصال‌های جدید پس از برقراری مجدد مسیر قابل‌ارسال‌اند.
- نصب سرویس systemd و تنظیم خودکار فایروال هنوز پیاده‌سازی نشده است.
- این نسخه آزمایشی است و هنوز تأیید امنیتی مستقل یا انتشار عملیاتی ندارد.
