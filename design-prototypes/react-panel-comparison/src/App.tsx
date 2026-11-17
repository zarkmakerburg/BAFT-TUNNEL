import { useEffect, useState } from "react";

type Page = "overview" | "tunnels" | "servers" | "monitoring" | "setup" | "diagnostics" | "activity" | "agent" | "settings";
type Lang = "fa" | "en";
const pages: Page[] = ["overview","tunnels","servers","monitoring","setup","diagnostics","activity","agent","settings"];
const icons: Record<Page,string> = {overview:"▦",tunnels:"⇄",servers:"▤",monitoring:"⌁",setup:"✳",diagnostics:"⌕",activity:"≡",agent:"◈",settings:"⚙"};
const labels = {
  fa: {overview:"نمای کلی",tunnels:"تونل‌ها",servers:"سرورها",monitoring:"پایش",setup:"راه‌اندازی",diagnostics:"عیب‌یابی",activity:"فعالیت‌ها",agent:"عامل",settings:"تنظیمات",operate:"عملیات",configure:"راه‌اندازی",support:"پشتیبانی",system:"سیستم",sample:"پیش‌نمایش؛ داده زنده متصل نیست",unknown:"نامشخص",unavailable:"در دسترس نیست",error:"خطا",fresh:"تازگی داده: بررسی نشده",local:"عامل محلی",title:"پنل مدیریت تونل",search:"جست‌وجو",updated:"آخرین بررسی: ثبت نشده",details:"جزئیات",view:"مشاهده",start:"شروع راه‌اندازی",create:"ساخت تونل",all:"همه",none:"هنوز داده‌ای از API دریافت نشده است",health:"وضعیت عملیاتی",traffic:"ترافیک",latency:"تأخیر",attention:"نیازمند توجه",history:"تاریخچه",identity:"شناسه محلی",peer:"همتا",capabilities:"قابلیت‌ها",trust:"اعتماد و گواهی",run:"اجرای بررسی",source:"منبع داده",result:"نتیجه",time:"زمان",event:"رویداد",setting:"گزینه",value:"مقدار",step:"مرحله",name:"نام",port:"درگاه",role:"نقش",language:"زبان",theme:"پوسته",version:"نسخه"},
  en: {overview:"Overview",tunnels:"Tunnels",servers:"Servers",monitoring:"Monitoring",setup:"Setup",diagnostics:"Diagnostics",activity:"Activity",agent:"Agent",settings:"Settings",operate:"OPERATE",configure:"SET UP",support:"TROUBLESHOOT",system:"SYSTEM",sample:"Preview only · no live data connected",unknown:"Unknown",unavailable:"Unavailable",error:"Error",fresh:"Data freshness: not verified",local:"Local Agent",title:"Tunnel operations",search:"Search",updated:"Last checked: not recorded",details:"Details",view:"View",start:"Start setup",create:"Create tunnel",all:"All",none:"No data has been received from the API yet.",health:"Operational state",traffic:"Traffic",latency:"Latency",attention:"Needs attention",history:"History",identity:"Local identity",peer:"Peer",capabilities:"Capabilities",trust:"Trust & certificates",run:"Run check",source:"Data source",result:"Result",time:"Time",event:"Event",setting:"Setting",value:"Value",step:"Step",name:"Name",port:"Port",role:"Role",language:"Language",theme:"Theme",version:"Version"}
};
const navGroups: {title:keyof typeof labels.fa; items:Page[]}[] = [
  {title:"operate",items:["overview","tunnels","servers","monitoring"]},
  {title:"configure",items:["setup"]},
  {title:"support",items:["diagnostics","activity"]},
  {title:"system",items:["agent","settings"]}
];

function Pill({children,kind="unknown"}:{children:React.ReactNode;kind?:string}) { return <span className={"pill "+kind}><i aria-hidden="true">{kind==="ok"?"✓":kind==="warn"?"!":kind==="bad"?"×":"?"}</i>{children}</span>; }
function Metric({title,value,sub,kind="unknown"}:{title:string;value:string;sub:string;kind?:string}) { return <div className="metric"><span className="metric-title">{title}</span><strong>{value}</strong><small>{sub}</small><Pill kind={kind}>{kind==="ok"?"Observed":kind==="warn"?"Needs review":"Unknown"}</Pill></div>; }
function Panel({title,extra,children}:{title:string;extra?:React.ReactNode;children:React.ReactNode}) { return <section className="panel"><header><h3>{title}</h3>{extra}</header>{children}</section>; }
function Chart({bars=false}:{bars?:boolean}) { return <div className={"chart "+(bars?"bars":"")} aria-label="Illustrative chart; no live samples"><div className="chart-grid"><span/><span/><span/><span/></div><svg viewBox="0 0 600 150" role="img" aria-label="Illustrative chart, no live samples"><path className="chart-area" d="M0 115 C55 108 65 70 120 82 S195 120 250 72 S340 92 390 48 S485 68 540 28 S580 50 600 18 L600 150 L0 150Z"/><path className="chart-line" d="M0 115 C55 108 65 70 120 82 S195 120 250 72 S340 92 390 48 S485 68 540 28 S580 50 600 18"/></svg><div className="chart-foot"><span>—</span><span>{bars?"Samples":"Timeline"}</span><span>—</span></div></div>; }
function PageContent({page,lang}:{page:Page;lang:Lang}) {
  const t=labels[lang];
  if(page==="overview") return <>
    <div className="hero"><div><Pill>{t.unknown}</Pill><h2>{t.health}</h2><p>{t.none}</p></div><button className="primary">{t.start}</button></div>
    <div className="metrics"><Metric title={t.tunnels} value="—" sub={t.source}/><Metric title={t.latency} value="—" sub="ms · no sample"/><Metric title={t.traffic} value="—" sub="bytes · no sample"/><Metric title={t.attention} value="—" sub="not evaluated"/></div>
    <div className="content-grid"><Panel title={t.health} extra={<Pill>{t.unknown}</Pill>}><Chart/><p className="muted">{t.none}</p></Panel><Panel title={t.attention}><div className="empty-state"><span>ⓘ</span><p>{t.none}</p></div></Panel><Panel title={t.traffic}><Chart bars/></Panel><Panel title={t.history}><div className="empty-state"><span>◷</span><p>{t.none}</p></div></Panel></div>
  </>;
  if(page==="tunnels") return <div className="content-grid"><Panel title={t.tunnels} extra={<button className="primary">{t.create}</button>}><div className="empty-state"><span>⇄</span><Pill>{t.unknown}</Pill><p>{t.none}</p></div></Panel><Panel title={t.details}><dl><dt>{t.name}</dt><dd>—</dd><dt>{t.role}</dt><dd>{t.unknown}</dd><dt>{t.peer}</dt><dd>{t.unknown}</dd><dt>{t.result}</dt><dd><Pill>{t.unknown}</Pill></dd></dl></Panel></div>;
  if(page==="servers") return <div className="content-grid"><Panel title={t.local}><dl><dt>{t.identity}</dt><dd dir="ltr">—</dd><dt>{t.version}</dt><dd>—</dd><dt>{t.capabilities}</dt><dd><Pill>{t.unknown}</Pill></dd><dt>{t.trust}</dt><dd><Pill>{t.unknown}</Pill></dd></dl></Panel><Panel title={t.peer}><div className="empty-state"><span>◌</span><Pill>{t.unknown}</Pill><p>{t.none}</p></div></Panel><Panel title="Fleet"><p className="muted">Central Fleet is outside this local panel prototype.</p><Pill>{t.unavailable}</Pill></Panel></div>;
  if(page==="monitoring") return <div className="stack"><Panel title={t.history} extra={<div className="segmented"><button>1h</button><button>24h</button></div>}><Chart/></Panel><div className="metrics"><Metric title={t.latency} value="—" sub="ms"/><Metric title={t.traffic} value="—" sub="bytes"/></div><Panel title={t.health}><Chart bars/></Panel></div>;
  if(page==="setup") return <Panel title={t.create}><div className="steps">{["۱","۲","۳","۴","۵","۶"].map((n,i)=><div className={i===0?"step active":"step"} key={n}><b>{n}</b><span>{lang==="fa"?["نقش","نام","پورت","موتور","پیش‌بررسی","تأیید"][i]:["Role","Name","Port","Engine","Preflight","Review"][i]}</span></div>)}</div><div className="form-grid"><label>{t.name}<input placeholder="edge-link" disabled/></label><label>{t.port}<input placeholder="443" disabled/></label><label>{t.role}<select disabled><option>{t.unknown}</option></select></label></div><p className="muted">{t.sample}</p><button className="primary" disabled>{t.start}</button></Panel>;
  if(page==="diagnostics") return <div className="stack"><Panel title={t.run}><button className="primary" disabled>{t.run}</button><Pill>{t.unknown}</Pill><p className="muted">{t.none}</p></Panel><Panel title={t.result}><div className="table"><div>{t.step}</div><div>{t.result}</div><div>{t.source}</div>{["Local","Transport","Peer","Integrity"].map(x=><React.Fragment key={x}><span>{x}</span><span><Pill>{t.unknown}</Pill></span><span>—</span></React.Fragment>)}</div></Panel></div>;
  if(page==="activity") return <Panel title={t.activity}><div className="table"><div>{t.time}</div><div>{t.event}</div><div>{t.result}</div>{[1,2,3].map(x=><React.Fragment key={x}><span>—</span><span>{t.none}</span><span><Pill>{t.unknown}</Pill></span></React.Fragment>)}</div></Panel>;
  if(page==="agent") return <div className="content-grid"><Panel title={t.local}><dl><dt>{t.version}</dt><dd>—</dd><dt>{t.health}</dt><dd><Pill>{t.unknown}</Pill></dd><dt>{t.capabilities}</dt><dd><Pill>{t.unknown}</Pill></dd></dl></Panel><Panel title={t.source}><p>{t.none}</p><Pill>{t.unavailable}</Pill></Panel></div>;
  return <Panel title={t.settings}><div className="settings-list">{[[t.language,lang==="fa"?"فارسی":"English"],[t.theme,"Dark"],[t.version,"—"]].map(([a,b])=><div key={a}><span>{a}</span><strong>{b}</strong></div>)}</div></Panel>;
}
function Variant({variant,page,lang}:{variant:"a"|"b"|"c";page:Page;lang:Lang}) {
  const t=labels[lang], rtl=lang==="fa";
  return <article className={"variant variant-"+variant}>
    <div className="variant-label"><b>{variant==="a"?"01 · WEAVE":variant==="b"?"02 · CLARITY":"03 · COMMAND"}</b><span>{variant==="a"?(lang==="fa"?"تار و پود · تیره و آرام": "Weave · calm dark"):variant==="b"?(lang==="fa"?"وضوح · روشن و مینیمال":"Clarity · light minimal"):(lang==="fa"?"فرمان · فشرده و عملیاتی":"Command · dense operations")}</span></div>
    <div className="preview-shell">
      <aside className="preview-side">
        <div className="brand"><span className="brand-mark">⌘</span><b>BAFT-TUNNEL</b></div>
        {navGroups.map(g=><div className="nav-group" key={g.title}><small>{t[g.title]}</small>{g.items.map(p=><button key={p} onClick={()=>window.dispatchEvent(new CustomEvent("prototype-page",{detail:p}))} className={page===p?"selected":""}><span>{icons[p]}</span><em>{t[p]}</em></button>)}</div>)}
        <div className="side-foot"><Pill>{t.unknown}</Pill><small>{t.local}</small></div>
      </aside>
      <main className="preview-main">
        <header className="topbar"><div><small>{t.local} / {t[page]}</small><h1>{t[page]}</h1></div><div className="top-actions"><span className="fresh">{t.fresh}</span><button aria-label={t.search}>⌕</button><button aria-label="Notifications">♧</button></div></header>
        <div className="page-body"><div className="sample-banner">◇ {t.sample}</div><PageContent page={page} lang={lang}/></div>
        <footer><span>{t.updated}</span><span>BAFT-TUNNEL · {t.local}</span></footer>
      </main>
    </div>
  </article>;
}
export default function App() {
  const [page,setPage]=useState<Page>("overview");
  const [lang,setLang]=useState<Lang>("fa");
  const [mobile,setMobile]=useState(false);
  useEffect(()=>{const f=(e:Event)=>setPage((e as CustomEvent<Page>).detail);window.addEventListener("prototype-page",f);return ()=>window.removeEventListener("prototype-page",f);},[]);
  const t=labels[lang];
  return <div className="comparison" dir={lang==="fa"?"rtl":"ltr"}>
    <header className="comparison-head"><div><span className="eyebrow">BAFT-TUNNEL · UI EXPLORATION</span><h1>{lang==="fa"?"مقایسه سه مسیر طراحی پنل":"Compare three panel design directions"}</h1><p>{lang==="fa"?"یک صفحه را انتخاب کن و هر سه ظاهر را هم‌زمان مقایسه کن. این محیط فقط پیش‌نمایش است و به Agent یا API وصل نیست.":"Choose a page and compare all three visual directions at once. This preview is not connected to an Agent or API."}</p></div><div className="controls"><label>{lang==="fa"?"صفحه":"Page"}<select value={page} onChange={e=>setPage(e.target.value as Page)}>{pages.map(p=><option value={p} key={p}>{t[p]}</option>)}</select></label><button onClick={()=>setLang(lang==="fa"?"en":"fa")}>{lang==="fa"?"English":"فارسی"}</button><button className="mobile-toggle" onClick={()=>setMobile(!mobile)}>{mobile?(lang==="fa"?"نمایش سه‌تایی":"Show all"): (lang==="fa"?"مقایسه تک‌به‌تک":"Focus one")}</button></div></header>
    <div className={"variants "+(mobile?"focus":"")}>
      <Variant variant="a" page={page} lang={lang}/><Variant variant="b" page={page} lang={lang}/><Variant variant="c" page={page} lang={lang}/>
    </div>
    <div className="comparison-foot"><span>Prototype only · no API calls · no production files changed</span><span>React 19 · TypeScript · Vite · self-contained assets</span></div>
  </div>;
}
