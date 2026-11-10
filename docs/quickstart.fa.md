# BAFT-TUNNEL — شروع سریع

> نسخهٔ پیش‌انتشار (Preview) است؛ فقط روی سرورهای تستیِ قابل‌پاک‌کردن. هنوز آمادهٔ استفادهٔ عملیاتی نیست.

دو سرور Ubuntu 22.04 یا جدیدتر (systemd 249+): **خارج** سرویس می‌دهد (serve) و **ایران** فوروارد می‌کند (forward). همهٔ دستورها با root و روی هر دو سرور اجرا شوند، مگر خلافش گفته شود.

## ۰. نصب یک‌خطی (Ubuntu 22.04+، معماری x86_64)

روی هر سرور اجرا کن؛ نام نمونهٔ این نود t1 است و می‌توانی آن را عوض کنی:

    curl -fsSL https://raw.githubusercontent.com/zarkmakerburg/BAFT-TUNNEL/main/scripts/quick-install.sh | sudo bash -s -- --register t1

این دستور جدیدترین نسخهٔ منتشرشده را (فعلاً شامل Preview) پیدا می‌کند، امضای Sigstore و SHA-256 را بررسی می‌کند و برنامه، سرویس‌های systemd و Agent پنل را نصب می‌کند؛ **تونل را روشن نمی‌کند**. برای دریافت فقط نسخهٔ پایدار، --stable و برای نسخهٔ مشخص، --tag vX.Y.Z را اضافه کن. فعلاً فقط Linux x86_64 پشتیبانی می‌شود.

## ۱. دریافت و بررسی بستهٔ امضاشده

```bash
TAG=v0.1.0-rc.6   # نسخه‌ای که نصب می‌کنید
R=https://github.com/zarkmakerburg/BAFT-TUNNEL/releases/download/$TAG
P=baft-tunnel-$TAG-linux-x86_64
curl -fLO $R/$P.tar.gz -fLO $R/SHA256SUMS -fLO $R/SHA256SUMS.cosign.bundle
cosign verify-blob SHA256SUMS --bundle SHA256SUMS.cosign.bundle \
  --certificate-identity-regexp '^https://github.com/zarkmakerburg/BAFT-TUNNEL/.github/workflows/release.yml@refs/(heads/main|tags/v[0-9][0-9A-Za-z.+-]*)$' \
  --certificate-oidc-issuer https://token.actions.githubusercontent.com
sha256sum -c --ignore-missing SHA256SUMS      # باید چاپ کند: $P.tar.gz: OK
tar -xzf $P.tar.gz && cd $P
```

بدون cosign، `sha256sum -c` فقط سالم‌بودن دانلود را نشان می‌دهد، نه این‌که چه کسی آن را امضا کرده.

## ۲. نصب (برنامه، سرویس‌ها، منو، ایجنت محلی)

```bash
sudo scripts/install.sh --binary ./baft-tunnel --register t1
```

اگر منو را ترجیح می‌دهید: `sudo baft-tunnel menu` (فارسی/انگلیسی) همهٔ مراحل زیر را انجام می‌دهد.

## ۳. جفت‌کردن دو سرور

موتور پیش‌فرض `noise-tcp` گواهی لازم ندارد. (برای `tcp-tls` اول PKI آفلاین را با `scripts/pki/baft-pki.sh` بسازید؛ [راهنمای اپراتور](https://github.com/zarkmakerburg/BAFT-TUNNEL/blob/main/docs/OPERATOR_GUIDE.md).)

```bash
C="--engine noise-tcp --direction direct --tunnel-id t1 --service-port 8080 --instance t1"
# خارج:
sudo baft-tunnel setup init $C --role serve   --node-id abroad --peer-id iran
# ایران:
sudo baft-tunnel setup init $C --role forward --node-id iran --peer-id abroad --peer-host ABROAD_IP
```

هر کدام یک **Checksum** چاپ می‌کند. فایل `/etc/baft-tunnel/bundle.json` هر سرور را به سرور دیگر کپی کنید، Checksumها را از یک کانال مطمئن (تلفن یا پیام‌رسان دیگر) با هم مقایسه کنید، سپس روی هر سرور:

```bash
sudo baft-tunnel setup join --bundle PEER_BUNDLE.json --checksum PEER_CHECKSUM
```

## ۴. روشن‌کردن و بررسی

```bash
sudo baft-tunnel config plan --config /etc/baft-tunnel/t1.json    # مرور تنظیمات
ssh -L 18080:127.0.0.1:18080 HOST                                 # از کامپیوتر خودتان
sudo cat /var/lib/baft-tunnel-agent/token                         # توکن پنل
```

`http://127.0.0.1:18080/` را باز کنید، توکن را وارد کنید، در بخش Tunnels دکمهٔ **Start** و بعد **Run diagnostics** را بزنید. بدون پنل: `sudo systemctl enable --now baft-tunnel@t1`.

## ۵. نگهداری

- لاگ: `journalctl -u baft-tunnel@t1`؛ گزارش پشتیبانی: `baft-tunnel support-bundle --config /etc/baft-tunnel/t1.json`.
- به‌روزرسانی: `sudo scripts/update.sh --binary ./baft-tunnel --instance t1` از بستهٔ جدیدتر؛ بازگشت با `scripts/rollback.sh BACKUP_DIR`.
- فقط tcp-tls: CRL را پیش از انقضا تمدید کنید (`baft-pki.sh renew-crl` و `push-crl.sh`).
- حذف: `sudo scripts/uninstall.sh --stop` (با `--purge-config --yes` تنظیمات و کلیدها هم پاک می‌شوند).

محدودیت‌ها: فایروال خودکار تنظیم نمی‌شود؛ پورت تونل را خودتان باز کنید. Reverse در حالت Preview است. [عیب‌یابی](https://github.com/zarkmakerburg/BAFT-TUNNEL/blob/main/docs/TROUBLESHOOTING.md).

---

## پیوست: ساخت از سورس برای توسعه‌دهنده (مسیر قدیمی)

> از سورس می‌سازد و برنامه را دستی و بیرون از systemd اجرا می‌کند. برای سرور از مراحل بالا استفاده کنید.

### پیش‌نیازها

- Linux یا macOS و Rust نسخه 1.85 یا جدیدتر.
- برای هر نود یک گواهی صادرشده از CA خصوصی؛ گواهی باید URI SAN دقیق `urn:baft-tunnel:node:<node-id>` داشته باشد.
- گواهی سرور علاوه بر URI SAN باید DNS/IP SAN مطابق مقدار `--server-name` داشته باشد.
- هر دو سمت باید CRL معتبر و امضاشده توسط همان CA را داشته باشند. CRL مفقود، منقضی یا نامعتبر باعث رد اتصال می‌شود.
- اثرانگشت SHA-256 گواهی هر نود را خارج از کانال تبادل کنید.

### ساخت

```bash
cargo build --release
```

### آماده‌سازی گواهی‌ها

راهنمای [صدور آفلاین گواهی‌ها](OPERATOR_GUIDE.md) را دنبال کنید. کلید خصوصی CA باید رمزگذاری‌شده و آفلاین بماند. کلید خصوصی هر نود فعلاً فایل PEM بدون رمز با مجوز دسترسی `0600` است؛ هرگز آن را از نود خارج نکنید.

### اجرای Direct

روی سرور مقصد خارج:

```bash
./target/release/baft-tunnel serve   --listen 0.0.0.0:9443 --target 127.0.0.1:8080   --local-id node-abroad --peer-id node-iran --tunnel-id demo-tunnel --direction direct   --peer-fingerprint FINGERPRINT_IRAN   --ca ./pki/ca.crt --crl ./pki/ca.crl --cert ./pki/abroad.crt --key ./pki/abroad.key
```

روی نود ایران:

```bash
./target/release/baft-tunnel forward   --listen 127.0.0.1:18080 --remote 203.0.113.10:9443 --server-name tunnel.abroad.example.test   --local-id node-iran --peer-id node-abroad --tunnel-id demo-tunnel --direction direct   --peer-fingerprint FINGERPRINT_ABROAD   --ca ./pki/ca.crt --crl ./pki/ca.crl --cert ./pki/iran.crt --key ./pki/iran.key
```

### اجرای Reverse

برای Reverse جای دو نقش را عوض کنید: `serve` روی ایران و `forward` روی خارج اجرا شود. مقدار `--direction reverse` باید روی هر دو سمت یکسان باشد.

### محدودیت‌های فعلی

- مقصد TCP ثابت و توسط اپراتور تعیین می‌شود؛ همتا نمی‌تواند مقصد دلخواه انتخاب کند.
- احراز هویت TLS 1.3/mTLS، اثرانگشت، هویت SAN، جهت تونل و مجوز هر Stream جداگانه بررسی می‌شوند.
- CRL به‌صورت hot-reload بارگذاری نمی‌شود. CRL جدید را با `baft-pki.sh renew-crl` بسازید، با `push-crl.sh` به هر دو سرور برسانید و هر دو فرایند را Restart کنید.
- در قطع مسیر، Streamهای فعال از نو ادامه پیدا نمی‌کنند؛ اتصال‌های جدید پس از برقراری مجدد مسیر قابل‌ارسال‌اند.
- نصب‌کننده و سرویس systemd وجود دارد (`scripts/install.sh`، راهنمای اپراتور)؛ تنظیم خودکار فایروال وجود ندارد.
- این نسخه آزمایشی است؛ ارزیابی امنیتی P0+P1 انجام شده ولی بازبینی مستقل کامل پیش از نسخهٔ پایدار و انتشار عملیاتی هنوز انجام نشده است.
