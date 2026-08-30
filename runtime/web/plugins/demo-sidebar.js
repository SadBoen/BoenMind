/* 示例插件:demo-sidebar(2026-08-30,验证前端界面插槽机制)
 * 不改界面源码,仅通过 window.boenmind.register 向声明过的插槽注册节点。
 * 本插件只做纯外观演示(时钟 + 提示),不调用任何后端 API——
 * 功能类插件的数据必须走后端权限合同(ADR-0006),插槽本身不给能力。 */
(function () {
  if (window.__demoSidebarInstalled) return;
  window.__demoSidebarInstalled = true;

  const box = document.createElement("div");
  box.style.cssText =
    "border:1px solid var(--bm-alias-border-l2);border-radius:10px;padding:9px 11px;" +
    "font-size:12px;color:var(--bm-alias-label-secondary);margin:0 2px;display:grid;gap:3px";
  const title = document.createElement("b");
  title.textContent = "插件 · 侧栏小工具";
  title.style.cssText = "font-weight:500;color:var(--bm-alias-label-primary)";
  const clock = document.createElement("span");
  const tip = document.createElement("span");
  tip.textContent = "经 sidebar-extra 插槽注册";
  box.append(title, clock, tip);

  const tick = () => {
    clock.textContent = "当前时间 " + new Date().toLocaleTimeString("zh-CN", { hour12: false });
  };
  tick();
  const timer = setInterval(tick, 1000);

  const ok = window.boenmind.register({ slot: "sidebar-extra", id: "demo-sidebar", node: box, order: 1 });
  if (!ok) { clearInterval(timer); return; }

  // 卸载:一次注册绑定一条生命周期,清节点 + 停定时器
  window.__demoPluginUnload = function () {
    window.boenmind.unregister("demo-sidebar");
    clearInterval(timer);
    window.__demoSidebarInstalled = false;
    delete window.__demoPluginUnload;
  };
})();
