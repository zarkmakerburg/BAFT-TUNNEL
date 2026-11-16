#!/usr/bin/env bash
# Interactive BAFT-TUNNEL menu (Persian / English). It only wraps the existing commands:
# install.sh, install-agent-control.sh, panel-https.sh, `baft-tunnel setup|config`, systemctl/journalctl for
# baft-tunnel@NAME, backup.sh, restore.sh, update.sh, rollback.sh, uninstall.sh and the PKI
# helpers. Every command is shown before it runs. No engine, protocol or host setting is added.
#
#   menu.sh [--lang fa|en] [--plain] [--dry-run] [--destdir ROOT]
#   menu.sh --banner        print the banner and exit
#
# Keys (on a terminal): Up/Down (or k/j) move, Enter selects, Esc (or q) goes back/quits,
# 1-9 jump. --plain (automatic when stdin or stdout is not a terminal): numbered list, type a
# number, 0 goes back. --dry-run prints the commands instead of running them. --destdir ROOT
# is the staging mode of the scripts (no systemd; systemctl/journalctl steps are skipped).
set -Euo pipefail

SCRIPTS="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd -P)"
LANG_SEL="${BAFT_LANG:-}"
PLAIN=0; DRY=0; ROOT_PREFIX=""
NAME_RE='^[A-Za-z0-9][A-Za-z0-9_-]{0,62}$'

banner() {
  local cols="${BOX_COLS:-${COLUMNS:-}}"
  [[ -n "$cols" ]] || cols="$(tput cols 2>/dev/null || echo 80)"
  if (( cols >= 66 )); then
    cat <<'EOF'
  ____    _    _____ _____     _____ _   _ _   _ _   _ _____ _
 | __ )  / \  |  ___|_   _|   |_   _| | | | \ | | \ | | ____| |
 |  _ \ / _ \ | |_    | |_____ | | | | | |  \| |  \| |  _| | |
 | |_) / ___ \|  _|   | |_____|| | | |_| | |\  | |\  | |___| |___
 |____/_/   \_\_|     |_|      |_|  \___/|_| \_|_| \_|_____|_____|
EOF
  else
    cat <<'EOF'
  ___   _   ___ _____
 | _ ) /_\ | __|_   _|
 | _ \/ _ \| _|  | |    T U N N E L
 |___/_/ \_\_|   |_|
EOF
  fi
}

while (($#)); do
  case "$1" in
    --lang) (($# >= 2)) || { echo "menu: --lang needs fa or en" >&2; exit 2; }; LANG_SEL="$2"; shift 2 ;;
    --plain) PLAIN=1; shift ;;
    --dry-run) DRY=1; shift ;;
    --destdir) (($# >= 2)) || { echo "menu: --destdir needs a path" >&2; exit 2; }; ROOT_PREFIX="$2"; shift 2 ;;
    --banner) banner; exit 0 ;;
    -h|--help) sed -n 2,13p "$0" | sed 's/^# \{0,1\}//'; exit 0 ;;
    *) echo "menu: unknown argument: $1" >&2; exit 2 ;;
  esac
done
case "$LANG_SEL" in fa|en|"") ;; *) echo "menu: --lang must be fa or en" >&2; exit 2 ;; esac

# Input: the terminal for the arrow-key menu; stdin in plain mode (also used by the tests).
if [[ "$PLAIN" == "0" ]] && [[ -t 0 && -t 1 ]] && { exec 3</dev/tty; } 2>/dev/null; then
  :
else
  PLAIN=1
  exec 3<&0
fi

STAGING=0
if [[ -n "$ROOT_PREFIX" ]]; then
  mkdir -p "$ROOT_PREFIX" && ROOT_PREFIX="$(cd "$ROOT_PREFIX" && pwd -P)" || exit 2
  STAGING=1
fi
CONF_DIR="$ROOT_PREFIX/etc/baft-tunnel"
BACKUPS="$ROOT_PREFIX/var/backups"
if [[ -n "$ROOT_PREFIX" ]]; then
  BIN="$ROOT_PREFIX/usr/local/bin/baft-tunnel"
else
  BIN="$(command -v baft-tunnel 2>/dev/null || echo /usr/local/bin/baft-tunnel)"
fi

# ---------------------------------------------------------------- text (en / fa)
declare -A EN FA
EN=(
  [choose_lang]="Language / زبان"
  [hint]="Up/Down: move   Enter: select   Esc: back"
  [pick]="Number (0 = back):"
  [back]="Back"
  [main]="Main menu"
  [m_install]="Install or upgrade"
  [m_register]="Register an instance with the local agent (panel)"
  [m_panel]="Panel: show the web link and login password"
  [m_https]="Panel HTTPS: set up or retry (domain or IP)"
  [m_pair]="Pair the two nodes: legacy noise-tcp (advanced)"
  [pair_legacy]="Legacy/advanced: this creates a noise-tcp tunnel (Experimental; Reverse is Preview). The supported setup is the panel Wizard (tcp-tls): ssh -L 18080:127.0.0.1:18080 HOST, then open http://127.0.0.1:18080/."
  [m_config]="Configuration (plan / validate / apply)"
  [m_service]="Tunnel: start / stop / restart"
  [m_status]="Status and diagnostics"
  [m_crl]="Renew the CRL (tcp-tls only)"
  [m_backup]="Backup / restore"
  [m_update]="Update / roll back"
  [m_uninstall]="Uninstall"
  [m_lang]="Language: فارسی / English"
  [m_exit]="Exit"
  [not_root]="Note: not running as root; most actions need: sudo baft-tunnel menu"
  [staging]="Staging mode (--destdir %s): nothing on this host is changed; systemd steps are skipped."
  [dry]="Dry run: commands are printed, not run."
  [running]="Running:"
  [would]="Would run:"
  [skipped]="Skipped in staging mode (needs systemd):"
  [ok]="Done."
  [failed]="Failed (exit %s). Nothing else was changed by the menu."
  [enter]="Press Enter to continue..."
  [cancel]="Cancelled."
  [bye]="Bye."
  [default]="default"
  [empty_cancel]="empty = cancel"
  [bad_value]="Invalid value, try again."
  [yes]="Yes"
  [no]="No"
  [type_yes]="Type yes to confirm:"
  [installed_yes]="An installation exists (%s)."
  [up_guarded]="Upgrade the binary (guarded, rolls back automatically)"
  [reinstall]="Install/reinstall files (install.sh)"
  [q_binary]="Path of the new baft-tunnel binary"
  [no_binary]="Not an executable file: %s"
  [q_register]="Instance name to register with the agent (empty = skip)"
  [q_instance]="Instance name (letters, digits, _ -)"
  [pick_instance]="Choose the instance"
  [other_instance]="Other name..."
  [no_instances]="No instance configuration found in %s yet; type a name."
  [next]="Next step:"
  [next_install]="Set up the tunnel in the panel Wizard (supported tcp-tls path): register the agent (menu > Register), open the panel (menu > Panel: show the web link; no HTTPS yet: menu > Panel HTTPS) and follow Create tunnel on both nodes. Menu pairing is legacy noise-tcp (advanced)."
  [next_register]="The agent runs now: pair from the panel (Create tunnel) or with setup init/join; it loads %s/%s.json by itself. Panel address and password: menu > Panel: show the web link and login password."
  [panel_title]="Panel web access"
  [panel_nohttps]="HTTPS is not set up on this host, so the panel is only on loopback. Set it up with: menu > Panel HTTPS (domain or IP). Without HTTPS: ssh -L 18080:127.0.0.1:18080 HOST, then open http://127.0.0.1:18080/ ."
  [panel_bad_state]="The HTTPS state under %s is unreadable or invalid. Re-run: menu > Panel HTTPS."
  [panel_link]="Link"
  [panel_gate_missing]="web path file not found (%s); re-run menu > Panel HTTPS."
  [panel_cert_ip]="Certificate: own CA generated on this host. The browser warns until you trust or pin this CA: %s"
  [panel_cert_le]="Certificate: Let's Encrypt, renewed automatically."
  [panel_keep_private]="Keep the link private; it is part of the login."
  [q_show_pw]="Show the login password on this screen?"
  [panel_pw]="Password"
  [panel_pw_cannot]="Cannot read the password file %s (run as root: sudo baft-tunnel menu)."
  [panel_pw_hint]="Paste it on the login page. It never leaves this host."
  [h_domain]="Domain: Let's Encrypt certificate (ports 80 and 443 must be open)"
  [h_ip]="IP address: own-CA certificate (browser warns until the CA is trusted)"
  [h_remove]="Remove the HTTPS front end (panel returns to loopback only)"
  [q_domain]="Domain name that points at this host"
  [q_hip]="Public IP address of this host"
  [q_email]="Email for Let's Encrypt expiry notices (empty = none)"
  [q_remove_https]="Remove the HTTPS front end? The panel is then reachable only through an SSH tunnel."
  [no_panel_script]="panel-https.sh is not installed next to the menu; upgrade to a release that includes it (menu > Install or upgrade)."
  [https_staging]="Staging mode: panel HTTPS changes nginx and certificates on the real host, so it is skipped here (use --dry-run to see the command)."
  [next_https]="Show the link again any time: menu > Panel: show the web link and login password. Allow 443/tcp (and 80/tcp for a domain) in your firewall; the menu never changes firewall rules."
  [p_init]="This node first: create key and pairing bundle (init)"
  [p_join]="I have the other node's bundle: check and write config (join)"
  [q_role]="Role of THIS node"
  [r_forward]="forward: this node connects to the other node (usually the Iran side)"
  [r_serve]="serve: this node waits for the other node (usually the server abroad)"
  [q_direction]="Direction"
  [d_direct]="direct (recommended)"
  [d_reverse]="reverse (Preview)"
  [q_node]="Name of THIS node (e.g. iran)"
  [q_peer]="Name of the OTHER node (e.g. abroad)"
  [q_service]="Service port carried by the tunnel (1024-65535)"
  [q_tport]="Tunnel port (1024-65535)"
  [q_peerip]="IP or host name of the OTHER node"
  [next_init]="1) Copy the bundle above to the other node (for example: scp %s root@OTHER:/root/peer-bundle.json).\n2) Read the checksum to the other side by phone or messenger, never in the same channel as the bundle.\n3) On the other node: menu > Pair the two nodes > join, with this bundle and checksum."
  [q_bundle]="Path of the OTHER node's bundle file"
  [q_checksum]="Checksum the OTHER node showed (xxxxxxxx-xxxxxxxx-xxxxxxxx-xxxxxxxx)"
  [chk_show]="You typed: %s"
  [chk_same]="Is this exactly what the other node shows, read to you out of band?"
  [chk_bad]="A checksum is 32 hex characters (0-9, a-f)."
  [next_join]="Review: menu > Configuration > Plan. Then start: menu > Tunnel > Start (or from the panel)."
  [c_plan]="Plan (show what will run; changes nothing)"
  [c_validate]="Validate"
  [c_apply]="Apply: start now and at boot"
  [no_config]="Configuration not found: %s (pair the nodes first)."
  [s_start]="Start (now and at boot)"
  [s_stop]="Stop (and not at boot)"
  [s_restart]="Restart"
  [q_apply]="Start baft-tunnel@%s now and enable it at boot?"
  [q_stop]="Stop baft-tunnel@%s and disable it at boot?"
  [st_service]="Service state"
  [st_logs]="Recent logs (50 lines)"
  [st_validate]="Validate configuration"
  [st_bundle]="Support bundle (redacted)"
  [st_crl]="CRL freshness (tcp-tls)"
  [crl_note]="Only for the tcp-tls engine (noise-tcp has no certificates). Run on the machine that holds the offline CA."
  [q_ca]="CA directory"
  [q_out]="Empty output directory for the new CRL"
  [q_days]="Validity in days (2-90)"
  [q_crl]="CRL file"
  [next_crl]="Distribute it to both nodes: scripts/pki/push-crl.sh (see --help)."
  [b_backup]="Back up now"
  [b_restore]="Restore a backup"
  [no_backups]="No backups found in %s."
  [pick_backup]="Choose a backup (newest first)"
  [q_restore]="Restore replaces the current configuration and keys; running tunnels are stopped first."
  [next_restore]="Start the tunnel again: menu > Tunnel > Start, then check menu > Status."
  [u_update]="Update the binary (guarded, automatic rollback)"
  [u_rollback]="Roll back to the binary before an install"
  [q_rollback]="Roll back binary/unit from this install backup?"
  [next_rollback]="Restart the tunnels that use it: menu > Tunnel > Restart, then check menu > Status."
  [next_update]="Check: menu > Status and diagnostics."
  [q_uninstall]="Remove the binary, units, agent and helper? Config and keys are kept."
  [q_purge]="Also delete configuration, keys and state (cannot be undone)?"
  [next_uninstall]="Configuration and keys were kept in %s. Reinstall any time with: install.sh --binary PATH"
)
FA=(
  [hint]="بالا/پایین: حرکت   Enter: انتخاب   Esc: بازگشت"
  [pick]="شماره (۰ = بازگشت):"
  [back]="بازگشت"
  [main]="منوی اصلی"
  [m_install]="نصب یا ارتقا"
  [m_register]="ثبت اینستنس در agent محلی (پنل)"
  [m_panel]="پنل: نمایش لینک وب و رمز ورود"
  [m_https]="HTTPS پنل: راه‌اندازی یا تلاش دوباره (دامنه یا IP)"
  [m_pair]="جفت‌کردن دو نود: noise-tcp قدیمی (پیشرفته)"
  [pair_legacy]="قدیمی/پیشرفته: این گزینه تونل noise-tcp می‌سازد (آزمایشی؛ Reverse پیش‌نمایش است). مسیر پشتیبانی‌شده Wizard پنل (tcp-tls) است: ssh -L 18080:127.0.0.1:18080 HOST و بعد http://127.0.0.1:18080/ را باز کنید."
  [m_config]="پیکربندی (بازبینی / اعتبارسنجی / اعمال)"
  [m_service]="تونل: شروع / توقف / راه‌اندازی دوباره"
  [m_status]="وضعیت و عیب‌یابی"
  [m_crl]="تمدید CRL (فقط tcp-tls)"
  [m_backup]="پشتیبان‌گیری / بازگردانی"
  [m_update]="به‌روزرسانی / برگشت به نسخهٔ قبل"
  [m_uninstall]="حذف نصب"
  [m_exit]="خروج"
  [not_root]="توجه: با root اجرا نشده؛ بیشتر کارها نیاز دارند به: sudo baft-tunnel menu"
  [staging]="حالت staging (--destdir %s): روی این سیستم چیزی تغییر نمی‌کند؛ مراحل systemd رد می‌شوند."
  [dry]="اجرای آزمایشی: دستورها فقط نمایش داده می‌شوند."
  [running]="در حال اجرا:"
  [would]="اجرا می‌شد:"
  [skipped]="در حالت staging رد شد (نیاز به systemd):"
  [ok]="انجام شد."
  [failed]="ناموفق (کد خروج %s). منو چیز دیگری را تغییر نداد."
  [enter]="برای ادامه Enter بزنید..."
  [cancel]="لغو شد."
  [bye]="خداحافظ."
  [default]="پیش‌فرض"
  [empty_cancel]="خالی = لغو"
  [bad_value]="مقدار نامعتبر است، دوباره وارد کنید."
  [yes]="بله"
  [no]="خیر"
  [type_yes]="برای تأیید yes بنویسید:"
  [installed_yes]="نصب قبلی وجود دارد (%s)."
  [up_guarded]="ارتقای باینری (محافظت‌شده، در صورت خطا خودکار برمی‌گردد)"
  [reinstall]="نصب/نصب دوبارهٔ فایل‌ها (install.sh)"
  [q_binary]="مسیر باینری جدید baft-tunnel"
  [no_binary]="فایل اجرایی نیست: %s"
  [q_register]="نام اینستنس برای ثبت در agent (خالی = رد شدن)"
  [q_instance]="نام اینستنس (حروف انگلیسی، عدد، _ -)"
  [pick_instance]="اینستنس را انتخاب کنید"
  [other_instance]="نام دیگر..."
  [no_instances]="هنوز پیکربندی اینستنسی در %s نیست؛ نام را بنویسید."
  [next]="قدم بعدی:"
  [next_install]="تونل را در Wizard پنل بسازید (مسیر پشتیبانی‌شدهٔ tcp-tls): agent را ثبت کنید (منو > ثبت)، پنل را باز کنید (منو > پنل: نمایش لینک وب؛ اگر هنوز HTTPS ندارید: منو > HTTPS پنل) و روی هر دو نود «ساخت تونل» را دنبال کنید. جفت‌کردن از منو روش قدیمی noise-tcp (پیشرفته) است."
  [next_register]="agent همین حالا روشن است: از پنل (ساخت تونل) یا با setup init/join جفت کنید؛ %s/%s.json را خودش بارگذاری می‌کند. آدرس و رمز پنل: منو > پنل: نمایش لینک وب و رمز ورود."
  [panel_title]="دسترسی وب به پنل"
  [panel_nohttps]="HTTPS روی این سرور راه‌اندازی نشده و پنل فقط روی loopback است. راه‌اندازی: منو > HTTPS پنل (دامنه یا IP). بدون HTTPS: ssh -L 18080:127.0.0.1:18080 HOST و بعد http://127.0.0.1:18080/ را باز کنید."
  [panel_bad_state]="وضعیت HTTPS در %s خوانا یا معتبر نیست. دوباره اجرا کنید: منو > HTTPS پنل."
  [panel_link]="لینک"
  [panel_gate_missing]="فایل مسیر وب پیدا نشد (%s)؛ دوباره اجرا کنید: منو > HTTPS پنل."
  [panel_cert_ip]="گواهی: CA اختصاصی ساخته‌شده روی همین سرور. تا وقتی این CA را معتبر یا pin نکنید مرورگر هشدار می‌دهد: %s"
  [panel_cert_le]="گواهی: Let's Encrypt، با تمدید خودکار."
  [panel_keep_private]="لینک را خصوصی نگه دارید؛ بخشی از ورود است."
  [q_show_pw]="رمز ورود روی این صفحه نمایش داده شود؟"
  [panel_pw]="رمز"
  [panel_pw_cannot]="فایل رمز %s خوانده نمی‌شود (با root اجرا کنید: sudo baft-tunnel menu)."
  [panel_pw_hint]="آن را در صفحهٔ ورود بچسبانید. از این سرور بیرون نمی‌رود."
  [h_domain]="دامنه: گواهی Let's Encrypt (پورت‌های 80 و 443 باید باز باشند)"
  [h_ip]="آدرس IP: گواهی CA اختصاصی (تا معتبر کردن CA مرورگر هشدار می‌دهد)"
  [h_remove]="حذف HTTPS (پنل دوباره فقط روی loopback)"
  [q_domain]="نام دامنه‌ای که به این سرور اشاره می‌کند"
  [q_hip]="آدرس IP عمومی این سرور"
  [q_email]="ایمیل برای اعلان انقضای Let's Encrypt (خالی = بدون ایمیل)"
  [q_remove_https]="HTTPS حذف شود؟ پنل بعد از آن فقط با تونل SSH در دسترس است."
  [no_panel_script]="panel-https.sh کنار منو نصب نیست؛ به نسخه‌ای که آن را دارد ارتقا دهید (منو > نصب یا ارتقا)."
  [https_staging]="حالت staging: HTTPS پنل nginx و گواهی‌های سرور واقعی را تغییر می‌دهد، پس اینجا رد می‌شود (برای دیدن دستور از --dry-run استفاده کنید)."
  [next_https]="هر وقت خواستید لینک را دوباره ببینید: منو > پنل: نمایش لینک وب و رمز ورود. پورت 443/tcp (و برای دامنه 80/tcp) را در فایروال باز کنید؛ منو هیچ‌وقت قوانین فایروال را تغییر نمی‌دهد."
  [p_init]="اول این نود: ساخت کلید و bundle جفت‌سازی (init)"
  [p_join]="bundle نود دیگر را دارم: بررسی و نوشتن پیکربندی (join)"
  [q_role]="نقش همین نود"
  [r_forward]="forward: این نود به نود دیگر وصل می‌شود (معمولاً سمت ایران)"
  [r_serve]="serve: این نود منتظر نود دیگر می‌ماند (معمولاً سرور خارج)"
  [q_direction]="جهت"
  [d_direct]="direct (پیشنهادی)"
  [d_reverse]="reverse (پیش‌نمایش)"
  [q_node]="نام همین نود (مثلاً iran)"
  [q_peer]="نام نود دیگر (مثلاً abroad)"
  [q_service]="پورت سرویسی که از تونل عبور می‌کند (1024-65535)"
  [q_tport]="پورت تونل (1024-65535)"
  [q_peerip]="IP یا نام میزبان نود دیگر"
  [next_init]="۱) bundle بالا را به نود دیگر ببرید (مثلاً: scp %s root@OTHER:/root/peer-bundle.json).\n۲) چک‌سام را تلفنی یا با پیام‌رسان برای طرف مقابل بخوانید، نه از همان راهی که bundle را فرستادید.\n۳) روی نود دیگر: منو > جفت‌کردن دو نود > join، با همین bundle و چک‌سام."
  [q_bundle]="مسیر فایل bundle نود دیگر"
  [q_checksum]="چک‌سامی که نود دیگر نشان داد (xxxxxxxx-xxxxxxxx-xxxxxxxx-xxxxxxxx)"
  [chk_show]="شما نوشتید: %s"
  [chk_same]="آیا دقیقاً همانی است که نود دیگر نشان می‌دهد و از راه جداگانه برایتان خوانده شد؟"
  [chk_bad]="چک‌سام ۳۲ کاراکتر هگز است (0-9 و a-f)."
  [next_join]="بازبینی: منو > پیکربندی > بازبینی. بعد شروع: منو > تونل > شروع (یا از پنل)."
  [c_plan]="بازبینی (نمایش آنچه اجرا می‌شود؛ چیزی تغییر نمی‌کند)"
  [c_validate]="اعتبارسنجی"
  [c_apply]="اعمال: شروع از الان و هنگام بوت"
  [no_config]="پیکربندی پیدا نشد: %s (اول دو نود را جفت کنید)."
  [s_start]="شروع (از الان و هنگام بوت)"
  [s_stop]="توقف (و غیرفعال در بوت)"
  [s_restart]="راه‌اندازی دوباره"
  [q_apply]="baft-tunnel@%s الان شروع و برای بوت فعال شود؟"
  [q_stop]="baft-tunnel@%s متوقف و در بوت غیرفعال شود؟"
  [st_service]="وضعیت سرویس"
  [st_logs]="لاگ‌های اخیر (۵۰ خط)"
  [st_validate]="اعتبارسنجی پیکربندی"
  [st_bundle]="بستهٔ پشتیبانی (بدون اطلاعات حساس)"
  [st_crl]="تازگی CRL (tcp-tls)"
  [crl_note]="فقط برای موتور tcp-tls (noise-tcp گواهی ندارد). روی ماشینی اجرا کنید که CA آفلاین روی آن است."
  [q_ca]="پوشهٔ CA"
  [q_out]="پوشهٔ خالی خروجی برای CRL جدید"
  [q_days]="اعتبار به روز (2-90)"
  [q_crl]="فایل CRL"
  [next_crl]="آن را به هر دو نود برسانید: scripts/pki/push-crl.sh (راهنما: --help)."
  [b_backup]="همین الان پشتیبان بگیر"
  [b_restore]="بازگردانی یک پشتیبان"
  [no_backups]="پشتیبانی در %s پیدا نشد."
  [pick_backup]="یک پشتیبان انتخاب کنید (جدیدترین اول)"
  [q_restore]="بازگردانی، پیکربندی و کلیدهای فعلی را جایگزین می‌کند؛ تونل‌های در حال اجرا اول متوقف می‌شوند."
  [next_restore]="تونل را دوباره روشن کنید: منو > تونل > شروع، بعد منو > وضعیت را ببینید."
  [u_update]="به‌روزرسانی باینری (محافظت‌شده، برگشت خودکار)"
  [u_rollback]="برگشت به باینری پیش از یک نصب"
  [q_rollback]="باینری/یونیت از این پشتیبان نصب برگردانده شود؟"
  [next_rollback]="تونل‌هایی که از آن استفاده می‌کنند را دوباره راه‌اندازی کنید: منو > تونل > راه‌اندازی دوباره، بعد منو > وضعیت."
  [next_update]="بررسی: منو > وضعیت و عیب‌یابی."
  [q_uninstall]="باینری، یونیت‌ها، agent و helper حذف شوند؟ پیکربندی و کلیدها می‌مانند."
  [q_purge]="پیکربندی، کلیدها و وضعیت هم پاک شوند (برگشت‌ناپذیر)؟"
  [next_uninstall]="پیکربندی و کلیدها در %s ماندند. نصب دوباره: install.sh --binary PATH"
)
t() {
  local key="$1"
  if [[ "$LANG_SEL" == "fa" && -n "${FA[$key]:-}" ]]; then printf '%s' "${FA[$key]}"; else printf '%s' "${EN[$key]:-$key}"; fi
}
# shellcheck disable=SC2059 # the format comes from the fixed tables above
tf() { local key="$1"; shift; printf "$(t "$key")" "$@"; }
say() { printf '%s\n' "$*"; }

# Persian/Arabic-Indic digits -> ASCII, surrounding blanks trimmed.
normalize() {
  local s="$1" i fa=(۰ ۱ ۲ ۳ ۴ ۵ ۶ ۷ ۸ ۹) ar=(٠ ١ ٢ ٣ ٤ ٥ ٦ ٧ ٨ ٩)
  for i in 0 1 2 3 4 5 6 7 8 9; do s="${s//${fa[i]}/$i}"; s="${s//${ar[i]}/$i}"; done
  s="${s#"${s%%[![:space:]]*}"}"; s="${s%"${s##*[![:space:]]}"}"
  printf '%s' "$s"
}

# ---------------------------------------------------------------- terminal UI
cleanup_tty() { [[ "$PLAIN" == "1" ]] || printf '\033[?25h'; }
trap cleanup_tty EXIT
trap 'cleanup_tty; echo; exit 130' INT TERM

quit() { cleanup_tty; echo; say "$(t bye)"; exit 0; }

read_key() {
  local k rest=""
  IFS= read -rsn1 k <&3 || { echo eof; return; }
  if [[ "$k" == $'\e' ]]; then
    IFS= read -rsn2 -t 0.05 rest <&3 || true
    case "$rest" in
      '[A'|'OA') echo up ;;
      '[B'|'OB') echo down ;;
      '') echo esc ;;
      *) IFS= read -rsn5 -t 0.01 _ <&3 || true; echo other ;;
    esac
  elif [[ -z "$k" ]]; then echo enter
  else
    case "$k" in
      k|K) echo up ;; j|J) echo down ;; q|Q) echo esc ;;
      [1-9]) echo "n$k" ;;
      *) echo other ;;
    esac
  fi
}

# ---------------------------------------------------------------- boxes
# Unicode box drawing on UTF-8 terminals, ASCII otherwise. Widths count characters, not bytes,
# and ignore zero-width marks (ZWNJ, Persian/Arabic diacritics) so Persian lines align too.
if [[ "$(locale charmap 2>/dev/null)" == UTF-8 ]]; then
  B_TL='╭' B_TR='╮' B_BL='╰' B_BR='╯' B_H='─' B_V='│' B_ML='├' B_MR='┤' B_PTR='▶'
else
  B_TL='+' B_TR='+' B_BL='+' B_BR='+' B_H='-' B_V='|' B_ML='+' B_MR='+' B_PTR='>'
fi
ZW_MARKS=("$(printf '\342\200\214')" "$(printf '\342\200\215')" "$(printf '\331\224')"
  "$(printf '\331\213')" "$(printf '\331\214')" "$(printf '\331\215')" "$(printf '\331\216')"
  "$(printf '\331\217')" "$(printf '\331\220')" "$(printf '\331\221')" "$(printf '\331\222')")
dispw() {
  local LC_ALL=C.UTF-8 s="$1" m
  for m in "${ZW_MARKS[@]}"; do s="${s//"$m"/}"; done
  printf '%s' "${#s}"
}
BOX_W=70
box_width() {
  local cols="${COLUMNS:-}"
  [[ -n "$cols" ]] || cols="$(tput cols 2>/dev/null || echo 80)"
  BOX_W=$(( cols - 4 )); (( BOX_W <= 70 )) || BOX_W=70; (( BOX_W >= 30 )) || BOX_W=30
}
box_rule() { local l="$1" r="$2" line; printf -v line '%*s' "$((BOX_W + 2))" ''; printf '%s%s%s\n' "$l" "${line// /$B_H}" "$r"; }
# box_line TEXT [STYLE]: one framed row; STYLE is an SGR code for the text area (e.g. 7 = reverse).
box_line() {
  local text="$1" style="${2:-}" w pad
  w="$(dispw "$text")"; pad=$(( BOX_W - w )); (( pad >= 0 )) || pad=0
  if [[ -n "$style" ]]; then printf '%s \033[%sm%s%*s\033[0m %s\n' "$B_V" "$style" "$text" "$pad" '' "$B_V"
  else printf '%s %s%*s %s\n' "$B_V" "$text" "$pad" '' "$B_V"; fi
}
box_text() { # wrap a long text into framed rows
  local word line="" 
  for word in $1; do
    if [[ -n "$line" ]] && (( $(dispw "$line $word") > BOX_W )); then box_line "$line"; line="$word"
    else line="${line:+$line }$word"; fi
  done
  [[ -z "$line" ]] || box_line "$line"
}

header() {
  banner
  if [[ "$DRY" == "1" ]]; then say "$(t dry)"; fi
  if [[ "$STAGING" == "1" ]]; then tf staging "$ROOT_PREFIX"; echo; fi
  if [[ "$STAGING" == "0" && "$DRY" == "0" && "$(id -u)" != "0" ]]; then say "$(t not_root)"; fi
}

# choose TITLE ITEM... -> CHOICE = 0-based index, or -1 for back (Esc / 0).
CHOICE=-1
choose() {
  local title="$1"; shift
  local items=("$@") n=$# sel=0 i key line num
  if [[ "$PLAIN" == "1" ]]; then
    printf '\n== %s ==\n' "$title"
    for i in "${!items[@]}"; do printf ' %d) %s\n' "$((i + 1))" "${items[i]}"; done
    printf ' 0) %s\n' "$(t back)"
    while :; do
      printf '%s ' "$(t pick)"
      IFS= read -r key <&3 || quit
      key="$(normalize "$key")"
      if [[ "$key" == 0 || "$key" == q ]]; then CHOICE=-1; return; fi
      if [[ "$key" =~ ^[0-9]{1,2}$ ]] && (( key >= 1 && key <= n )); then CHOICE=$((key - 1)); return; fi
      say "$(t bad_value)"
    done
  fi
  printf '\033[?25l'
  while :; do
    printf '\033[H\033[2J'
    box_width
    box_rule "$B_TL" "$B_TR"
    while IFS= read -r line; do box_line "$line" 36; done < <(BOX_COLS=$BOX_W banner)
    [[ "$DRY" == "0" ]] || box_text "$(t dry)"
    [[ "$STAGING" == "0" ]] || box_text "$(tf staging "$ROOT_PREFIX")"
    if [[ "$STAGING" == "0" && "$DRY" == "0" && "$(id -u)" != "0" ]]; then box_text "$(t not_root)"; fi
    box_rule "$B_ML" "$B_MR"
    box_line "$title" 1
    box_rule "$B_ML" "$B_MR"
    for i in "${!items[@]}"; do
      printf -v num '%*d' "${#n}" "$((i + 1))"
      if (( i == sel )); then box_line "$B_PTR $num. ${items[i]}" 7
      else box_line "  $num. ${items[i]}"; fi
    done
    box_rule "$B_ML" "$B_MR"
    box_line "$(t hint)" 2
    box_rule "$B_BL" "$B_BR"
    key="$(read_key)"
    case "$key" in
      up) sel=$(( (sel - 1 + n) % n )) ;;
      down) sel=$(( (sel + 1) % n )) ;;
      enter) printf '\033[?25h'; CHOICE=$sel; echo; return ;;
      esc) printf '\033[?25h'; CHOICE=-1; echo; return ;;
      eof) quit ;;
      n*) i="${key#n}"; if (( i <= n )); then printf '\033[?25h'; CHOICE=$((i - 1)); echo; return; fi ;;
    esac
  done
}

# ask VAR PROMPT [DEFAULT] [REGEX] -> 0, or 1 when cancelled (empty answer without default).
ask() {
  local __var="$1" prompt="$2" def="${3:-}" re="${4:-}" ans
  while :; do
    if [[ -n "$def" ]]; then printf '%s [%s: %s]: ' "$prompt" "$(t default)" "$def"
    else printf '%s (%s): ' "$prompt" "$(t empty_cancel)"; fi
    IFS= read -r ans <&3 || quit
    ans="$(normalize "$ans")"
    [[ -n "$ans" ]] || ans="$def"
    [[ -n "$ans" ]] || { say "$(t cancel)"; return 1; }
    if [[ -z "$re" || "$ans" =~ $re ]]; then printf -v "$__var" '%s' "$ans"; return 0; fi
    say "$(t bad_value)"
  done
}

confirm() { # confirm QUESTION -> 0 on yes (default no)
  choose "$1" "$(t no)" "$(t yes)"
  [[ "$CHOICE" == "1" ]]
}

confirm_typed() { # for steps that cannot be undone
  local ans
  say "$1"
  printf '%s ' "$(t type_yes)"
  IFS= read -r ans <&3 || quit
  ans="$(normalize "$ans")"
  [[ "$ans" == yes || "$ans" == YES || "$ans" == "بله" ]]
}

pause() {
  [[ "$PLAIN" == "1" ]] && return 0
  printf '\n%s' "$(t enter)"
  IFS= read -r _ <&3 || true
}

next_step() {
  local text line
  box_width
  printf -v text '%b' "$*"
  echo
  box_rule "$B_TL" "$B_TR"
  box_line "$(t next)" 1
  while IFS= read -r line; do box_text "$line"; done <<< "$text"
  box_rule "$B_BL" "$B_BR"
}

# run CMD... : shows the command, runs it (or prints it in --dry-run), reports the result.
LAST_STATUS=0
run() {
  local shown
  shown="$(printf '%q ' "$@")"
  if [[ "$STAGING" == "1" && ( "$1" == systemctl || "$1" == journalctl ) ]]; then
    printf '\n%s %s\n' "$(t skipped)" "$shown"; LAST_STATUS=0; return 0
  fi
  if [[ "$DRY" == "1" ]]; then printf '\n%s %s\n' "$(t would)" "$shown"; LAST_STATUS=0; return 0; fi
  printf '\n%s %s\n' "$(t running)" "$shown"
  box_width; box_rule "$B_H" "$B_H"
  "$@" <&3
  LAST_STATUS=$?
  if (( LAST_STATUS == 0 )); then say "$(t ok)"; else tf failed "$LAST_STATUS"; echo; fi
  return "$LAST_STATUS"
}

dest_args() { [[ "$STAGING" == "0" ]] || printf '%s\0' --destdir "$ROOT_PREFIX"; }
DEST=()
mapfile -d '' -t DEST < <(dest_args)

# ---------------------------------------------------------------- helpers
list_instances() {
  local f n
  shopt -s nullglob
  for f in "$CONF_DIR"/*.json; do
    n="$(basename "$f" .json)"
    case "$n" in config|bundle|pairing-state|desired) continue ;; esac
    [[ "$n" =~ $NAME_RE ]] && printf '%s\n' "$n"
  done
  shopt -u nullglob
}

INSTANCE=""
pick_instance() {
  local names=()
  mapfile -t names < <(list_instances)
  if ((${#names[@]} == 0)); then
    tf no_instances "$CONF_DIR"; echo
    ask INSTANCE "$(t q_instance)" "" "$NAME_RE"
    return
  fi
  if ((${#names[@]} == 1)); then INSTANCE="${names[0]}"; return 0; fi
  choose "$(t pick_instance)" "${names[@]}" "$(t other_instance)"
  ((CHOICE >= 0)) || return 1
  if ((CHOICE < ${#names[@]})); then INSTANCE="${names[CHOICE]}"; return 0; fi
  ask INSTANCE "$(t q_instance)" "" "$NAME_RE"
}

default_binary() {
  local c
  for c in "$PWD/baft-tunnel" "$SCRIPTS/../baft-tunnel" "$SCRIPTS/../target/release/baft-tunnel"; do
    if [[ -f "$c" && -x "$c" ]]; then printf '%s' "$(cd "$(dirname "$c")" && pwd -P)/baft-tunnel"; return; fi
  done
}

ask_binary() { # -> BINARY
  local d
  d="$(default_binary)"
  while :; do
    ask BINARY "$(t q_binary)" "$d" || return 1
    [[ -f "$BINARY" && -x "$BINARY" ]] && return 0
    tf no_binary "$BINARY"; echo
    d=""
  done
}

pick_backup() { # pick_backup GLOB-PREFIX -> PICKED
  local found=()
  shopt -s nullglob
  # newest first
  mapfile -t found < <(for d in "$BACKUPS/$1".*; do [[ -d "$d" ]] && printf '%s\t%s\n' "$(stat -c %Y "$d")" "$d"; done | sort -rn | cut -f2-)
  shopt -u nullglob
  if ((${#found[@]} == 0)); then tf no_backups "$BACKUPS"; echo; return 1; fi
  choose "$(t pick_backup)" "${found[@]}"
  ((CHOICE >= 0)) || return 1
  PICKED="${found[CHOICE]}"
}

# ---------------------------------------------------------------- actions
do_install() {
  local name="" BINARY
  if [[ -x "$BIN" ]]; then
    tf installed_yes "$BIN"; echo
    choose "$(t m_install)" "$(t up_guarded)" "$(t reinstall)"
    case "$CHOICE" in
      0) ask_binary || return 0
         run "$SCRIPTS/update.sh" --binary "$BINARY" "${DEST[@]}" && next_step "$(t next_update)"
         return 0 ;;
      1) ;;
      *) return 0 ;;
    esac
  fi
  ask_binary || return 0
  printf '%s (%s): ' "$(t q_register)" "$(t empty_cancel)"
  IFS= read -r name <&3 || quit
  name="$(normalize "$name")"
  while [[ -n "$name" && ! "$name" =~ $NAME_RE ]]; do
    say "$(t bad_value)"; printf '%s: ' "$(t q_register)"
    IFS= read -r name <&3 || quit; name="$(normalize "$name")"
  done
  local args=(--binary "$BINARY")
  [[ -z "$name" ]] || args+=(--register "$name")
  run "$SCRIPTS/install.sh" "${args[@]}" "${DEST[@]}" || return 0
  next_step "$(t next_install)"
  [[ -z "$name" ]] || next_step "$(tf next_register "/etc/baft-tunnel" "$name")"
}

do_register() {
  ask INSTANCE "$(t q_instance)" "$(list_instances | head -n 1)" "$NAME_RE" || return 0
  run "$SCRIPTS/install-agent-control.sh" --instance "$INSTANCE" "${DEST[@]}" || return 0
  next_step "$(tf next_register "/etc/baft-tunnel" "$INSTANCE")"
}

DOMAIN_RE='^([A-Za-z0-9]([A-Za-z0-9-]{0,61}[A-Za-z0-9])?\.)+[A-Za-z]{2,63}$'
IP_RE='^([0-9]{1,3}\.){3}[0-9]{1,3}$|^[0-9A-Fa-f]*:[0-9A-Fa-f:]{2,38}$'
HTTPS_STATE_DIR="$ROOT_PREFIX/var/lib/baft-tunnel-https"
TOKEN_FILE="$ROOT_PREFIX/var/lib/baft-tunnel-agent/token"

# Shows the panel HTTPS link built from the state panel-https.sh wrote, and (only after a
# confirmation) the login password. Nothing is changed; the password goes to the terminal only.
do_panel() {
  local st="$HTTPS_STATE_DIR/state" gf="$HTTPS_STATE_DIR/gate" host="" kind="" port="" self="" gate="" suffix="" link=""
  say "$(t panel_title)"
  if [[ ! -r "$st" ]]; then
    [[ -e "$st" ]] && { tf panel_bad_state "$HTTPS_STATE_DIR"; echo; } || say "$(t panel_nohttps)"
    return 0
  fi
  host="$(sed -n 's/^HOST=//p' "$st" | head -n 1)"; kind="$(sed -n 's/^KIND=//p' "$st" | head -n 1)"
  port="$(sed -n 's/^HTTPS_PORT=//p' "$st" | head -n 1)"; self="$(sed -n 's/^SELF_SIGNED=//p' "$st" | head -n 1)"
  if [[ ! "$host" =~ ^[A-Za-z0-9.:_-]+$ || ! "$port" =~ ^[0-9]{1,5}$ ]]; then tf panel_bad_state "$HTTPS_STATE_DIR"; echo; return 0; fi
  if [[ ! -r "$gf" ]]; then tf panel_gate_missing "$gf"; echo; return 0; fi
  gate="$(tr -d '[:space:]' < "$gf")"
  if [[ ! "$gate" =~ ^[A-Za-z0-9_-]{16}$ ]]; then tf panel_bad_state "$HTTPS_STATE_DIR"; echo; return 0; fi
  [[ "$host" != *:* ]] || host="[$host]"
  [[ "$port" == 443 ]] || suffix=":$port"
  link="https://$host$suffix/$gate/"
  echo
  say "$(t panel_link): $link"
  if [[ "$kind" == ip || "$self" == 1 ]]; then tf panel_cert_ip "/etc/baft-tunnel-https/ca.crt"; echo; else say "$(t panel_cert_le)"; fi
  say "$(t panel_keep_private)"
  confirm "$(t q_show_pw)" || return 0
  if [[ -r "$TOKEN_FILE" ]]; then
    say "$(t panel_pw): $(cat "$TOKEN_FILE")"; say "$(t panel_pw_hint)"
  else
    tf panel_pw_cannot "$TOKEN_FILE"; echo
  fi
}

# Set up or retry HTTPS (domain: Let's Encrypt; IP: own CA) by calling panel-https.sh, which
# rolls back on any failure and never downgrades to plain HTTP.
do_https() {
  local script="$SCRIPTS/panel-https.sh" val="" email="" args=()
  [[ -f "$script" ]] || { say "$(t no_panel_script)"; return 0; }
  if [[ "$STAGING" == "1" && "$DRY" == "0" ]]; then say "$(t https_staging)"; return 0; fi
  choose "$(t m_https)" "$(t h_domain)" "$(t h_ip)" "$(t h_remove)"
  case "$CHOICE" in
    0) ask val "$(t q_domain)" "" "$DOMAIN_RE" || return 0
       printf '%s: ' "$(t q_email)"; IFS= read -r email <&3 || quit; email="$(normalize "$email")"
       args=(--domain "$val"); [[ -z "$email" ]] || args+=(--email "$email") ;;
    1) ask val "$(t q_hip)" "" "$IP_RE" || return 0
       args=(--ip "$val") ;;
    2) confirm "$(t q_remove_https)" || { say "$(t cancel)"; return 0; }
       run bash "$script" --remove && next_step "$(t next_update)"
       return 0 ;;
    *) return 0 ;;
  esac
  run bash "$script" "${args[@]}" && next_step "$(t next_https)"
}

do_pair_init() {
  local role direction node peer tunnel service tport peerip="" engine=noise-tcp
  choose "$(t q_role)" "$(t r_forward)" "$(t r_serve)"
  case "$CHOICE" in 0) role=forward ;; 1) role=serve ;; *) return 0 ;; esac
  choose "$(t q_direction)" "$(t d_direct)" "$(t d_reverse)"
  case "$CHOICE" in 0) direction=direct ;; 1) direction=reverse ;; *) return 0 ;; esac
  local host_default
  host_default="$(hostname -s 2>/dev/null | tr -cd 'A-Za-z0-9_-' | cut -c1-63)"
  [[ "$host_default" =~ $NAME_RE ]] || host_default=""
  ask node "$(t q_node)" "$host_default" "$NAME_RE" || return 0
  ask peer "$(t q_peer)" "" "$NAME_RE" || return 0
  ask INSTANCE "$(t q_instance)" "main" "$NAME_RE" || return 0
  tunnel="$INSTANCE"
  ask service "$(t q_service)" "" '^[0-9]{4,5}$' || return 0
  ask tport "$(t q_tport)" "8443" '^[0-9]{4,5}$' || return 0
  if [[ "$role" == forward ]]; then
    ask peerip "$(t q_peerip)" "" '^[A-Za-z0-9.:_-]{1,253}$' || return 0
  fi
  local args=(setup init --engine "$engine" --role "$role" --direction "$direction"
    --node-id "$node" --peer-id "$peer" --tunnel-id "$tunnel"
    --service-port "$service" --tunnel-port "$tport" --dir "$CONF_DIR" --instance "$INSTANCE")
  [[ -z "$peerip" ]] || args+=(--peer-host "$peerip")
  run "$BIN" "${args[@]}" || return 0
  next_step "$(tf next_init "$CONF_DIR/bundle.json")"
}

do_pair_join() {
  local bundle sum
  ask bundle "$(t q_bundle)" "/root/peer-bundle.json" || return 0
  while :; do
    ask sum "$(t q_checksum)" "" || return 0
    sum="$(printf '%s' "$sum" | tr 'A-F' 'a-f' | tr -d ' \t-')"
    if [[ "$sum" =~ ^[0-9a-f]{32}$ ]]; then break; fi
    say "$(t chk_bad)"
  done
  sum="${sum:0:8}-${sum:8:8}-${sum:16:8}-${sum:24:8}"
  tf chk_show "$sum"; echo
  confirm "$(t chk_same)" || { say "$(t cancel)"; return 0; }
  run "$BIN" setup join --dir "$CONF_DIR" --bundle "$bundle" --checksum "$sum" || return 0
  next_step "$(t next_join)"
}

do_pair() {
  say "$(t pair_legacy)"
  choose "$(t m_pair)" "$(t p_init)" "$(t p_join)"
  case "$CHOICE" in 0) do_pair_init ;; 1) do_pair_join ;; esac
}

require_config() {
  local cfg="$CONF_DIR/$INSTANCE.json"
  [[ -f "$cfg" ]] && return 0
  tf no_config "$cfg"; echo
  return 1
}

service_start() {
  confirm "$(tf q_apply "$INSTANCE")" || { say "$(t cancel)"; return 0; }
  run systemctl enable --now "baft-tunnel@$INSTANCE.service" || return 0
  run systemctl --no-pager --lines=0 status "baft-tunnel@$INSTANCE.service"
}

do_config() {
  pick_instance || return 0
  choose "$(t m_config) - $INSTANCE" "$(t c_plan)" "$(t c_validate)" "$(t c_apply)"
  case "$CHOICE" in
    0) require_config && run "$BIN" config plan --config "$CONF_DIR/$INSTANCE.json" ;;
    1) require_config && run "$BIN" config validate --config "$CONF_DIR/$INSTANCE.json" ;;
    2) require_config && service_start ;;
  esac
}

do_service() {
  pick_instance || return 0
  local unit="baft-tunnel@$INSTANCE.service"
  choose "$(t m_service) - $INSTANCE" "$(t s_start)" "$(t s_stop)" "$(t s_restart)"
  case "$CHOICE" in
    0) require_config && service_start ;;
    1) confirm "$(tf q_stop "$INSTANCE")" || { say "$(t cancel)"; return 0; }
       run systemctl disable --now "$unit" && run systemctl is-enabled "$unit" ;;
    2) run systemctl restart "$unit" && run systemctl --no-pager --lines=0 status "$unit" ;;
  esac
}

do_status() {
  pick_instance || return 0
  local unit="baft-tunnel@$INSTANCE.service" cfg="$CONF_DIR/$INSTANCE.json" crl
  choose "$(t m_status) - $INSTANCE" "$(t st_service)" "$(t st_logs)" "$(t st_validate)" "$(t st_bundle)" "$(t st_crl)"
  case "$CHOICE" in
    0) run systemctl --no-pager --lines=0 status "$unit"; run systemctl is-enabled "$unit" ;;
    1) run journalctl -u "$unit" -n 50 --no-pager ;;
    2) require_config && run "$BIN" config validate --config "$cfg" ;;
    3) require_config && run "$BIN" support-bundle --config "$cfg" ;;
    4) ask crl "$(t q_crl)" "$CONF_DIR/pki/ca.crl" && run "$SCRIPTS/pki/crl-status.sh" "$crl" ;;
  esac
}

do_crl() {
  local ca out days
  say "$(t crl_note)"
  ask ca "$(t q_ca)" "/root/baft-ca" || return 0
  ask out "$(t q_out)" "/root/baft-crl-$(date +%Y%m%d)" || return 0
  ask days "$(t q_days)" "30" '^[0-9]{1,2}$' || return 0
  run "$SCRIPTS/pki/baft-pki.sh" renew-crl "$ca" "$out" "$days" || return 0
  run "$SCRIPTS/pki/crl-status.sh" "$out/ca.crl"
  next_step "$(t next_crl)"
}

do_backup() {
  choose "$(t m_backup)" "$(t b_backup)" "$(t b_restore)"
  case "$CHOICE" in
    0) run "$SCRIPTS/backup.sh" "${DEST[@]}" ;;
    1) pick_backup baft-tunnel-backup || return 0
       confirm_typed "$(t q_restore)" || { say "$(t cancel)"; return 0; }
       run "$SCRIPTS/restore.sh" "${DEST[@]}" --stop --yes "$PICKED" && next_step "$(t next_restore)" ;;
  esac
}

do_update() {
  local BINARY
  choose "$(t m_update)" "$(t u_update)" "$(t u_rollback)"
  case "$CHOICE" in
    0) ask_binary || return 0
       run "$SCRIPTS/update.sh" --binary "$BINARY" "${DEST[@]}" && next_step "$(t next_update)" ;;
    1) pick_backup baft-tunnel-install || return 0
       confirm "$(t q_rollback)" || { say "$(t cancel)"; return 0; }
       run "$SCRIPTS/rollback.sh" "${DEST[@]}" "$PICKED" && next_step "$(t next_rollback)"
       # Rolling back to a release without the menu removes these scripts too.
       [[ -f "$SCRIPTS/menu.sh" ]] || quit ;;
  esac
}

do_uninstall() {
  confirm "$(t q_uninstall)" || { say "$(t cancel)"; return 0; }
  local args=(--stop)
  if confirm "$(t q_purge)" && confirm_typed "$(t q_purge)"; then args+=(--purge-config --yes); fi
  run "$SCRIPTS/uninstall.sh" "${DEST[@]}" "${args[@]}" || return 0
  [[ " ${args[*]} " == *" --purge-config "* ]] || next_step "$(tf next_uninstall "/etc/baft-tunnel")"
  # The menu itself was removed with the scripts; nothing more to offer.
  [[ -f "$SCRIPTS/install.sh" ]] || quit
}

# ---------------------------------------------------------------- main
if [[ -z "$LANG_SEL" ]]; then
  case "${LC_ALL:-${LANG:-}}" in fa*) LANG_SEL=fa ;; esac
fi
if [[ -z "$LANG_SEL" ]]; then
  if [[ "$PLAIN" == "1" ]]; then LANG_SEL=en
  else
    LANG_SEL=en
    choose "$(t choose_lang)" "English" "فارسی"
    [[ "$CHOICE" == "1" ]] && LANG_SEL=fa
  fi
fi
[[ "$PLAIN" == "0" ]] || header

while :; do
  choose "$(t main)" "$(t m_install)" "$(t m_register)" "$(t m_panel)" "$(t m_https)" "$(t m_pair)" "$(t m_config)" \
    "$(t m_service)" "$(t m_status)" "$(t m_crl)" "$(t m_backup)" "$(t m_update)" \
    "$(t m_uninstall)" "$(t m_lang)" "$(t m_exit)"
  case "$CHOICE" in
    0) do_install ;; 1) do_register ;; 2) do_panel ;; 3) do_https ;; 4) do_pair ;; 5) do_config ;;
    6) do_service ;; 7) do_status ;; 8) do_crl ;; 9) do_backup ;; 10) do_update ;; 11) do_uninstall ;;
    12) if [[ "$LANG_SEL" == fa ]]; then LANG_SEL=en; else LANG_SEL=fa; fi; continue ;;
    *) quit ;;
  esac
  # Esc in a submenu returns straight to the main menu; anything else waits so the output stays readable.
  [[ "$CHOICE" == "-1" ]] || pause
done
