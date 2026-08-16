/**
 * 外观深链（2026-08-16）：?skin=&effect=&wallpaper= 预置皮肤/背景特效/壁纸，
 * 在一切模块初始化前写入 localStorage（本文件必须是 main.tsx 的第一个 import）。
 * 用途：跨浏览器/新环境一键复现外观状态（测试与分享），无参数时零副作用。
 */
const seed = new URLSearchParams(window.location.search);
const seedKey = (storageKey: string, param: string, allowed?: readonly string[]) => {
  const v = seed.get(param);
  if (v && (!allowed || allowed.includes(v))) localStorage.setItem(storageKey, v);
};

seedKey("boenmind.skin", "skin", ["classic", "glass"]);
seedKey("boenmind.skin.effect", "effect", ["none", "wave"]);
seedKey(
  "boenmind.skin.wallpaper",
  "wallpaper",
  ["aqua", "sunset", "aurora", "nebula"],
);
