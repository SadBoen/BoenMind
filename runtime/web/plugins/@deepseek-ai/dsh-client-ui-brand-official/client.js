window.__ModuleLoader__.load({
	id: "@deepseek-ai/dsh-client-ui-brand-official",
	factory: (require) => {
		var module = { exports: {} };
		var exports = module.exports;
		Object.defineProperty(exports, Symbol.toStringTag, { value: "Module" });
		let react_jsx_runtime = require("react/jsx-runtime");
		//#region BoenMind 品牌替换(2026-08-30)
		/* 原作: causebefore 结构同源—— OfficialBrandMark 渲染鲸鱼标(FishLogo),
		 * OfficialBrandName 渲染 deepseek 字标(BrandWordmark)。
		 * BoenMind 版:以文字圆标 + 文字字标替代矢量鲸鱼,其余插槽注册结构不变。 */
		function OfficialBrandMark({ size, className }) {
			const px = typeof size === "number" ? size : 28;
			return (0, react_jsx_runtime.jsx)("span", {
				className,
				style: {
					display: "inline-grid", placeItems: "center",
					width: px, height: px, borderRadius: px / 3.2,
					background: "var(--dsw-alias-button-info-fill, #4d93f8)", color: "#fff",
					fontWeight: 700, fontSize: px * 0.52, fontFamily: "inherit",
					lineHeight: 1, userSelect: "none", letterSpacing: 0
				},
				children: "B"
			});
		}
		function OfficialBrandName() {
			return (0, react_jsx_runtime.jsx)("span", {
				style: {
					fontWeight: 650, fontSize: "1.06rem", letterSpacing: ".01em",
					color: "inherit", userSelect: "none"
				},
				children: "BoenMind"
			});
		}
		//#endregion
		//#region lib/types/client/index.js
		/** Required service: the UI slot registry. */
		const inject = ["slots"];
		/**
		 * Fill every shipped brand slot as one declaration-aware registration set.
		 * @param ctx - Client root context.
		 */
		function apply(ctx) {
			ctx.slots.inject("sidebar.brand.mark", () => ctx.slots.inject("sidebar.brand.name", () => ctx.slots.inject("conversation.hero.brand.mark", function* () {
				yield ctx.slots.register({ name: "sidebar.brand.mark" }, OfficialBrandMark);
				yield ctx.slots.register({ name: "sidebar.brand.name" }, OfficialBrandName);
				yield ctx.slots.register({ name: "conversation.hero.brand.mark" }, OfficialBrandMark);
			})));
		}
		//#endregion
		exports.apply = apply;
		exports.inject = inject;
		return module.exports;
	}
});

//# sourceMappingURL=client.js.map
