window.__ModuleLoader__.load({
	id: "@deepseek-ai/dsh-client-ui-directory-picker-native",
	factory: (require) => {
		var module = { exports: {} };
		var exports = module.exports;
		Object.defineProperty(exports, Symbol.toStringTag, { value: "Module" });
		let react = require("react");

		function PanelDirectoryFlow(props) {
			return (0, react.createElement)("div", {
				"data-boenmind": "workspace-panel-overlay",
				style: "position:fixed;inset:0;background:rgba(255,0,0,.45);z-index:2147483647;display:flex;align-items:center;justify-content:center;color:#fff;font:20px system-ui"
			}, "PANEL PIPELINE TEST open=" + String(!!props.open));
		}

		const inject = ["slots", "workspaces"];
		function apply(ctx) {
			const injected = () => ({
				listDirectory: (path) => ctx.workspaces.listDirectory(path)
			});
			ctx.slots.inject("conversation.hero.workspace.directoryFlow", () => ctx.slots.inject("sidebar.workspaces.directoryFlow", function* () {
				yield ctx.slots.register({
					name: "conversation.hero.workspace.directoryFlow",
					inject: injected
				}, PanelDirectoryFlow);
				yield ctx.slots.register({
					name: "sidebar.workspaces.directoryFlow",
					inject: injected
				}, PanelDirectoryFlow);
			}));
		}

		exports.apply = apply;
		exports.inject = inject;
		return module.exports;
	}
});
