const _K = ["\u767d\u767dLOVE\u5c39\u5c39", "LANDESK-bbloveyy-2026", "bbyybb", "buymeacoffee.com/bbyybb", "sponsors/bbyybb"];

export function verifyIntegrity(): boolean {
  const f = document.getElementById("appAuthorFooter");
  if (!f || !f.innerHTML.includes(_K[0])) return false;
  if (!f.dataset.sig || f.dataset.sig !== _K[1]) return false;
  if (!f.querySelector('a[href="#donate"]')) return false;
  return true;
}

export function verifyDonateDialog(): boolean {
  const d = document.querySelector('[data-author="' + _K[2] + '"]');
  if (!d) return true;
  const h = d.innerHTML;
  for (const m of _K) { if (!h.includes(m)) return false; }
  return true;
}

export function showTamperWarning(_r: string) {
  while (document.body.firstChild) document.body.removeChild(document.body.firstChild);
  const outer = document.createElement("div");
  outer.style.cssText = "display:flex;align-items:center;justify-content:center;height:100vh;background:#1a1a2e;color:#e74c3c;font-family:sans-serif;text-align:center;padding:40px";
  const inner = document.createElement("div");
  const h1 = document.createElement("h1");
  h1.style.cssText = "font-size:24px;margin-bottom:16px";
  h1.textContent = "\u5b8c\u6574\u6027\u68c0\u67e5\u5931\u8d25 / Integrity Check Failed";
  const p1 = document.createElement("p");
  p1.style.color = "#aaa";
  p1.textContent = "\u4f5c\u8005\u7f72\u540d\u4fe1\u606f\u5df2\u88ab\u7be1\u6539 / Author attribution has been tampered with.";
  const p2 = document.createElement("p");
  p2.style.cssText = "color:#888;margin-top:24px";
  p2.appendChild(document.createTextNode("LAN-Desk by "));
  const b = document.createElement("b");
  b.style.color = "#fff";
  b.textContent = "\u767d\u767dLOVE\u5c39\u5c39";
  p2.appendChild(b);
  inner.append(h1, p1, p2);
  outer.appendChild(inner);
  document.body.appendChild(outer);
}

let _n = 0;

export function initIntegrityGuard() {
  // 兼容 SPA：如果页面已加载完成，直接延迟检查；否则等待 load 事件
  const scheduleInitialCheck = () => {
    setTimeout(() => { if (!verifyIntegrity()) showTamperWarning("load"); }, 2000);
  };
  if (document.readyState === "complete") {
    scheduleInitialCheck();
  } else {
    window.addEventListener("load", scheduleInitialCheck);
  }
  setInterval(() => {
    if (!verifyIntegrity() || !verifyDonateDialog()) showTamperWarning("timer");
  }, 45000);
}

export function checkOnNavigation() {
  _n++;
  if (_n % 5 === 0 && !verifyIntegrity()) showTamperWarning("nav");
}
