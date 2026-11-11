# BAFT-TUNNEL — شروع سریع

> **نسخهٔ پیش‌انتشار امضاشدهٔ v0.1.0-rc.7؛ هنوز نسخهٔ پایدار نیست.** ابتدا فقط در محیط آزمایشی استفاده کن. پذیرش واقعی دو‌میزبانه در [اجرای U7 شمارهٔ 43063199428](https://github.com/zarkmakerburg/BAFT-TUNNEL/actions/runs/43063199428) در تاریخ 2026-11-11 موفق شد. این نتیجه فقط مسیر Direct این راهنما را پوشش می‌دهد و به معنی آمادگی Production نیست.

## مسیر پشتیبانی‌شده و پیش‌نیازها

- دو سرور x86_64 با Ubuntu 22.04 LTS و systemd 249؛ یکی serve (معمولاً خارج) و دیگری forward (معمولاً ایران).
- پورت TCP تونل روی سرور serve باید از سمت سرور دیگر قابل‌دسترسی باشد. بازکردن پورت در فایروال شبکه یا ارائه‌دهنده بر عهدهٔ اپراتور است؛ BAFT-TUNNEL هیچ قانون Firewall را تغییر نمی‌دهد.
- روی سرور serve یک سرویس TCP محلی داشته باش که به loopback متصل است؛ پورت آن از تونل عبور می‌کند.
- مسیر پشتیبانی‌شدهٔ پیش‌انتشار tcp-tls + Direct است. Reverse فقط Preview است؛ noise-tcp فقط Direct آزمایشی است؛ موتور BAFT در وضعیت Preview است و Wizard هنوز نمی‌تواند آن را پیکربندی کند.

## ۱. دریافت و بررسی بستهٔ امضاشده

روی هر دو سرور، بسته و فایل‌های امضای همان Release را دریافت کن:

```bash
TAG=v0.1.0-rc.7
BASE="https://github.com/zarkmakerburg/BAFT-TUNNEL/releases/download/$TAG"
PKG="baft-tunnel-$TAG-linux-x86_64"
curl -fL "$BASE/$PKG.tar.gz" -o "$PKG.tar.gz"
curl -fL "$BASE/SHA256SUMS" -o SHA256SUMS
curl -fL "$BASE/SHA256SUMS.cosign.bundle" -o SHA256SUMS.cosign.bundle

cosign verify-blob SHA256SUMS --bundle SHA256SUMS.cosign.bundle \
  --certificate-identity "https://github.com/zarkmakerburg/BAFT-TUNNEL/.github/workflows/release.yml@refs/tags/$TAG" \
  --certificate-oidc-issuer https://token.actions.githubusercontent.com
sha256sum --ignore-missing --check SHA256SUMS
tar -xzf "$PKG.tar.gz"
cd "$PKG"
```

تا وقتی تأیید Cosign موفق نشده و Checksum بسته OK نشده، ادامه نده. بررسی Checksum بدون تأیید امضا مشخص نمی‌کند چه کسی بسته را منتشر کرده است.

## ۲. نصب و ثبت Agent محلی

روی هر سرور یک نام Instance یکتا انتخاب کن (در مثال t1):

```bash
sudo scripts/install.sh --binary ./baft-tunnel --register t1
sudo baft-tunnel menu
```

این دستور باینری، سرویس‌های systemd، Agent محلی و منوی فارسی/انگلیسی را نصب می‌کند؛ تونل را روشن نمی‌کند. در Ubuntu 22.04 از systemd LoadCredential= برای دسترسی محدود سرویس به کلید استفاده می‌شود؛ این روش رمزنگاری در حالت سکون نیست. از طریق SSH Tunnel مرحلهٔ ۵ پنل را باز کن. **نکتهٔ مهم:** گزینهٔ مستقل «Pair the two nodes» در منو فعلاً موتور noise-tcp (آزمایشی) را می‌سازد؛ برای مسیر پشتیبانی‌شدهٔ tcp-tls از آن استفاده نکن و Wizard پنل را به‌کار ببر.

## ۳. دریافت گواهی tcp-tls از طریق پنل و CA آفلاین

روی هر سرور، پنل محلی را از طریق SSH (مرحلهٔ ۵) باز کن و در بخش PKI مربوط به Wizard:

1. DNS/IP گواهی همین نود را جدا از نام گواهی همتا وارد کن.
2. گزینهٔ **Generate CSR** را بزن. کلید خصوصی روی همان نود ساخته می‌شود و از آن خارج نمی‌شود. فقط CSR عمومی و اطلاعات نمایشی آن را کپی/دریافت کن.
3. روی یک سیستم جداگانه و آفلاین، با روش مستندشدهٔ `scripts/pki/baft-pki.sh issue-node` گواهی را صادر کن. کلید خصوصی CA هرگز به هیچ‌یک از دو سرور منتقل نمی‌شود. شناسهٔ دقیق نود و SAN مربوط به DNS/IP که پنل درخواست کرده را استفاده کن؛ SAN اضافی یا wildcard اضافه نکن.
4. فقط سه فایل عمومی leaf certificate، CA certificate و CRL فعلی را به همان نود برگردان و در پنل بارگذاری کن. Root helper بررسی می‌کند کلید محلی با گواهی منطبق باشد، CA/CRL معتبر باشند، شناسه و SAN دقیق باشند و گواهی لغو نشده باشد. بارگذاری باعث Restart سرویس نمی‌شود.
5. تا دریافت رسید موفق نصب در پنل صبر کن. اگر اعتبارسنجی شکست بخورد، مجموعهٔ گواهی فعال قبلی بدون تغییر می‌ماند.

کلید CA آفلاین می‌ماند. کلید خصوصی، دیتابیس CA یا رمز عبور CA را آپلود نکن. برای صدور آفلاین، تمدید گواهی و CRL و Push به [راهنمای اپراتور](OPERATOR_GUIDE.md) مراجعه کن. امضا آفلاین و دستی است؛ این فرآیند Online enrollment نیست.

## ۴. جفت‌کردن دو سرور با Wizard (Direct)

روی هر دو سرور Wizard را در پنل محلی باز کن:

1. موتور **tcp-tls**، حالت **Direct** و نقش همین سرور را انتخاب کن. روی سروری که Listener تونل از بیرون قابل‌دسترسی دارد (معمولاً خارج) نقش serve و روی سرور دیگر نقش forward را انتخاب کن.
2. Tunnel ID، پورت سرویس و پورت تونل را در هر دو سرور یکسان وارد کن. در نود forward، IP یا DNS قابل‌دسترسی سرور serve را وارد کن.
3. مرحلهٔ Init نمایش‌داده‌شده را روی هر نود اجرا کن. فقط Bundle عمومی تولیدشده را جابه‌جا کن؛ کلید خصوصی را منتقل نکن.
4. Checksumها را از یک کانال مستقل و مطمئن (مثلاً تماس تلفنی) با طرف مقابل مقایسه کن. روی هر نود Join را با Bundle همتا و Checksum اعلام‌شده از سمت مقابل کامل کن.
5. پیش از Apply، Plan و هویت همتا را بررسی کن. Apply یک اقدام مستقل و صریح است.

Bundle محرمانه نیست، اما مقایسهٔ Checksum از کانال جدا برای جلوگیری از جایگزینی ضروری است. در صورت عدم تطابق ادامه نده. Setup روی Instance موجود بازنویسی نمی‌کند؛ پیش از تلاش دوباره وضعیت نصب قبلی را آگاهانه بررسی کن.

## ۵. روشن‌کردن و بررسی

پس از موفقیت Pairing روی هر دو سرور از پنل گزینهٔ **Start** را انتخاب کن. Start سرویس را روشن و برای اجرای خودکار هنگام Boot فعال می‌کند؛ Stop آن را متوقف و از Boot غیرفعال می‌کند؛ Restart سرویس را راه‌اندازی مجدد می‌کند. در پیام تأیید، اقدام را تأیید کن. منوی فارسی/انگلیسی نیز این عملیات سرویس و نگهداری را دارد.

برای بازکردن پنل محلی، از کامپیوتر خودت SSH Tunnel بساز:

```bash
ssh -L 18080:127.0.0.1:18080 USER@HOST
```

آدرس http://127.0.0.1:18080/ را باز کن و Token محلی Agent را که منو نشان می‌دهد یا در مسیر /var/lib/baft-tunnel-agent/token است وارد کن (فقط با دسترسی root بخوان و آن را برای کسی نفرست). در پنل از Tunnels → Start و سپس Run diagnostics استفاده کن. پنل و Agent فقط روی loopback گوش می‌دهند؛ پورت 18080 را عمومی نکن.

از طریق پورت سرویس محلی، ترافیک واقعی برنامه را عبور بده. Diagnostics شواهد TCP reachability، secure channel، identity، authorization، session و data path را جدا می‌کند. **Integrity در Diagnostics عادی محصول همچنان Unknown است**؛ قابل‌دسترسی بودن پورت یا وجود Session اثبات صحت Payload نیست. بررسی Hash در اندازه‌های ۶۴ KiB و ۲ MiB، شاهد مستقل LAB در U7 است.

## ۶. نگهداری و عملیات روزمره

- **وضعیت و لاگ:** از گزینهٔ Status and diagnostics در منو استفاده کن؛ لاگ سرویس با journalctl -u baft-tunnel@t1 قابل مشاهده است.
- **Backup/Restore:** از گزینه‌های Backup / restore در منو استفاده کن. Restore تنظیمات و کلیدها را جایگزین می‌کند و ابتدا تونل‌های مربوط را متوقف می‌کند؛ پس از Restore وضعیت Instance را جداگانه بررسی کن.
- **Update:** ابتدا بستهٔ نسخهٔ بعدی را دریافت و امضای آن را بررسی کن؛ سپس از گزینهٔ Update منو با باینری جدید استفاده کن. Installer نسخهٔ قبلی را Backup می‌کند. اگر باینری جدید در بررسی Start شکست بخورد، Update محافظت‌شده به‌صورت خودکار Rollback می‌کند. Rollback دستی: sudo scripts/rollback.sh BACKUP_DIR.
- **تمدید CRL:** برای tcp-tls تمدید فقط روی CA آفلاین انجام می‌شود و Push/Activation طبق بخش مربوط در [راهنمای اپراتور](OPERATOR_GUIDE.md) است. اجرای مستقل U1 به شمارهٔ [42656507521](https://github.com/zarkmakerburg/BAFT-TUNNEL/actions/runs/42656507521) Push یک CRL سی‌روزه، افزایش شماره CRL، بازیابی تونل و رد CRL کوتاه/تکراری را روی دو میزبان تأیید کرد. این شاهد جدا از U7 است.
- **حذف:** sudo scripts/uninstall.sh --stop سرویس و اجزای Agent نصب‌شده را حذف می‌کند ولی تنظیمات و کلیدها را نگه می‌دارد. فقط اگر واقعاً قصد حذف دائمی آن‌ها را داری --purge-config --yes را اضافه کن؛ این کار مخرب و غیرقابل‌بازگشت است.

## محدودیت‌های تأییدشده

- U7 در [اجرای 43063199428](https://github.com/zarkmakerburg/BAFT-TUNNEL/actions/runs/43063199428) روی بستهٔ امضاشدهٔ RC.7 موفق شد: نصب تمیز، Setup هدایت‌شدهٔ Direct، Apply Start برای هر دو نقش، صحت Hash در ۶۴ KiB و ۲ MiB، Diagnostics/Status/Traffic/Audit، ارتقای RC.5 به RC.7، Soak به‌مدت ۳۶۰۰ ثانیه (۲۶۴ دور، صفر Probe ناموفق، بدون Restart)، بازیابی فرایند، Backup/Restore، Update محافظت‌شده و Rollback باینری خراب، رد گواهی لغوشده، Uninstall و پاک‌سازی.
- U7، Reverse، BAFT، Noise، آمادگی Production یا انتشار پایدار را اثبات نمی‌کند. مقدار ready طبق طراحی Unknown می‌ماند. محصول فایروال را مدیریت نمی‌کند.
- در پایان U7 یک Drift نامرتبط شش‌خطی در Address Set فایروال میزبان خارج مشاهده شد؛ هیچ قانون BAFT-TUNNEL وجود نداشت. منشأ Drift هنوز باز است و در S10 پیگیری می‌شود؛ بنابراین ادعای بی‌اثری کامل روی Firewall مجاز نیست.
- Streamهای TCP فعال پس از قطع Carrier به‌صورت شفاف ادامه پیدا نمی‌کنند؛ برنامهٔ مصرف‌کننده باید اتصال جدید برقرار کند.
- این راهنما مجوز انتشار پایدار یا Deploy عملیاتی نیست.

برای عیب‌یابی به [Troubleshooting](TROUBLESHOOTING.md) و برای جزئیات نگهداری به [Operator Guide](OPERATOR_GUIDE.md) مراجعه کن.
