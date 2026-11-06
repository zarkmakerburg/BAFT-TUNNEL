# شروع سریع BAFT-TUNNEL

> وضعیت: پیش‌نویس فاز صفر؛ هنوز نسخه قابل نصب یا موتور تونل منتشر نشده است.

## اجرای آزمایشگاه شبیه‌سازی
نیازمند Docker Engine و افزونه Docker Compose:

```bash
cd lab
docker compose build
docker compose up -d
sh scripts/apply-netem.sh moderate
sh scripts/show-netem.sh
sh scripts/clear-netem.sh
docker compose down
```

این آزمایش فقط شبیه‌سازی شبکه بین دو کانتینر است و تست اتصال واقعی ایران به خارج محسوب نمی‌شود.

## پیش‌آزمایش شش سرور واقعی
راهنمای [REAL_HOSTS_LAB.md](REAL_HOSTS_LAB.md) را ببین. این مرحله فقط‌خواندنی است و به شش سرور تست، کلید SSH اختصاصی و اثرانگشت‌های میزبانِ مستقلانه تأییدشده نیاز دارد.
