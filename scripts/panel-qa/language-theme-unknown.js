// Browser QA for the panel. Needs a running agent: BAFT_TOKEN_FILE=<token file> [BAFT_URL=http://127.0.0.1:18080] [BAFT_SHOTS=<dir>] node language-theme-unknown.js
const { chromium } = require('playwright');
const BASE = (process.env.BAFT_URL || 'http://127.0.0.1:18080').replace(/\/$/, '');
const SHOTS = process.env.BAFT_SHOTS || require('os').tmpdir();
const TOKEN = require('fs').readFileSync(process.env.BAFT_TOKEN_FILE,'utf8').trim();
const R=[]; const ok=(n,c,d)=>R.push([c?'PASS':'FAIL',n,d||'']);
(async()=>{
 const b=await chromium.launch(); const ctx=await b.newContext({viewport:{width:1280,height:900}}); const p=await ctx.newPage();
 const csp=[]; p.on('console',m=>{ if(/Content Security Policy|Refused/i.test(m.text())) csp.push(m.text()); });
 await p.addInitScript(t=>sessionStorage.setItem('token',t),TOKEN);
 await p.goto(BASE+'/'); await p.waitForSelector('.shell');
 ok('default language fa/rtl', await p.evaluate(()=>document.documentElement.dir)==='rtl');
 ok('default theme dark', await p.evaluate(()=>document.documentElement.getAttribute('data-theme'))==='dark');
 await p.goto(BASE+'/#/settings'); await p.waitForTimeout(500);
 await p.locator('button',{hasText:'English'}).first().click(); await p.waitForTimeout(600);
 ok('switch to English -> ltr', await p.evaluate(()=>document.documentElement.dir)==='ltr' && await p.locator('h1').innerText()==='Settings');
 const navLeft = await p.evaluate(()=>document.querySelector('.sidebar').getBoundingClientRect().left);
 ok('English sidebar on left', navLeft===0);
 await p.locator('button',{hasText:'Light'}).click(); await p.waitForTimeout(500);
 ok('light theme applied', await p.evaluate(()=>document.documentElement.getAttribute('data-theme'))==='light');
 await p.goto(BASE+'/#/overview'); await p.waitForTimeout(800);
 await p.screenshot({path:''+SHOTS+'/desktop-en-light-overview.png',fullPage:true});
 await p.locator('button',{hasText:'فارسی'}).first().click(); await p.waitForTimeout(600);
 ok('switch back to fa -> rtl, sidebar on right', await p.evaluate(()=>document.documentElement.dir==='rtl' && Math.round(document.querySelector('.sidebar').getBoundingClientRect().right)===window.innerWidth));
 // keyboard: tab reaches skip link first
 await p.keyboard.press('Tab'); ok('first Tab focuses skip link', await p.evaluate(()=>document.activeElement.className)==='skip');
 // unreachable state: status fails -> Unknown, no stale health
 await p.route('**/v1/status', r=>r.fulfill({status:500,body:'{}'}));
 await p.goto(BASE+'/#/overview'); await p.reload(); await p.waitForTimeout(1200);
 const txt=await p.locator('.content').innerText();
 ok('status failure shows unreachable banner and no healthy/listening claim', await p.locator('.banner.bad').count()===1 && !/گوش می‌دهد|Listening/.test(txt));
 ok('topbar badge is Unknown when status fails', /نامشخص|Unknown/.test(await p.locator('.topbar .badge').innerText()));
 await p.screenshot({path:''+SHOTS+'/desktop-fa-unreachable.png'});
 ok('no CSP violations', csp.length===0, csp.join('|'));
 // innerHTML never used
 await b.close(); R.forEach(r=>console.log(r.join(' '))); 
})();
