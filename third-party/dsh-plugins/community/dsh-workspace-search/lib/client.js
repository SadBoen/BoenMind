/**
 * dsh-workspace-search — prebuilt client bundle for the dsh web shell.
 *
 * Registers a VS Code-style search Tab into better-sidebar's `ctx.betterSidebar`
 * service: keyword input, per-file grouped match lists with line numbers, and
 * click-to-open in better-sidebar's built-in editor. Self-contained except for
 * the platform seed modules (react).
 */
window.__ModuleLoader__.load({
  id: "dsh-workspace-search",
  factory: function (require) {
    var module = { exports: {} };
    var exports = module.exports;
    Object.defineProperty(exports, Symbol.toStringTag, { value: "Module" });
    var React = require("react");
    var createElement = React.createElement;
    var useEffect = React.useEffect;
    var useSyncExternalStore = React.useSyncExternalStore;
    var UiPrimitives = require("@deepseek-ai/dsh-client-ui-primitives");

    var name = "dsh-workspace-search";
    var inject = ["betterSidebar", "connection"];

    // ── module store ─────────────────────────────────────────────────────────
    var store = {
      query: "",
      caseSensitive: false,
      regex: false,
      include: "",
      exclude: "",
      filtersOpen: false,
      busy: false,
      error: null,
      result: null,
      expanded: {}, // file path -> bool
      token: 0,
    };
    var listeners = new Set();
    function setStore(patch) {
      store = Object.assign({}, store, patch);
      listeners.forEach(function (l) { l(); });
    }
    function subscribe(fn) {
      listeners.add(fn);
      return function () { listeners.delete(fn); };
    }
    function useStore(sel) {
      return useSyncExternalStore(subscribe, function () { return sel(store); }, function () { return sel(store); });
    }

    var wire = {
      call: function () {
        return Promise.reject(new Error("dsh-workspace-search: rpc not wired"));
      },
    };

    function runSearch(root, query, caseSensitive, regex, include, exclude) {
      if (query.trim() === "") {
        setStore({ query: query, result: null, error: null });
        return;
      }
      var token = store.token + 1;
      setStore({ query: query, busy: true, error: null, token: token });
      wire.call("/workspace-search", "search", {
        root: root,
        query: query,
        caseSensitive: caseSensitive,
        regex: regex,
        include: include,
        exclude: exclude,
      }).then(function (result) {
        if (!result.ok) throw new Error(result.error ? result.error.message : "rpc failed");
        return result.value;
      }).then(function (value) {
        if (store.token !== token) return;
        setStore({ busy: false, result: value, expanded: {} });
      }, function (err) {
        if (store.token !== token) return;
        setStore({ busy: false, error: err && err.message ? err.message : "network" });
      });
    }

    function basenameOf(p) {
      var i = p.lastIndexOf("/");
      return i === -1 ? p : p.slice(i + 1);
    }

    // ── component ────────────────────────────────────────────────────────────
    function SearchView(props) {
      var scope = props.scope || {};
      var ctx = props.ctx;
      var root = scope.cwd;

      var query = useStore(function (s) { return s.query; });
      var caseSensitive = useStore(function (s) { return s.caseSensitive; });
      var regex = useStore(function (s) { return s.regex; });
      var include = useStore(function (s) { return s.include; });
      var exclude = useStore(function (s) { return s.exclude; });
      var filtersOpen = useStore(function (s) { return s.filtersOpen; });
      var busy = useStore(function (s) { return s.busy; });
      var error = useStore(function (s) { return s.error; });
      var result = useStore(function (s) { return s.result; });
      var expanded = useStore(function (s) { return s.expanded; });

      useEffect(function () {
        var t = setTimeout(function () {
          if (store.query !== "" && !store.busy) runSearch(root, store.query, store.caseSensitive, store.regex, store.include, store.exclude);
        }, 250);
        return function () { clearTimeout(t); };
      }, [query, caseSensitive, regex, include, exclude, root]);

      var totalMatches = result ? result.results.reduce(function (n, r) { return n + r.matches.length; }, 0) : 0;

      var openFile = function (path) {
        if (ctx && ctx.betterSidebar) {
          ctx.betterSidebar.openTab({ type: "editor", path: path, title: basenameOf(path) });
        }
      };

      var now = function () { return { query: query, caseSensitive: caseSensitive, regex: regex, include: include, exclude: exclude }; };
      var submitNow = function () { runSearch(root, query, caseSensitive, regex, include, exclude); };

      return createElement("div", { className: "wss-root" },
        createElement("div", { className: "wss-inputrow" },
          createElement("input", {
            className: "wss-input",
            type: "text",
            placeholder: "Search workspace (file names / content)…",
            value: query,
            autoFocus: true,
            spellCheck: false,
            onChange: function (e) { setStore({ query: e.target.value }); },
            onKeyDown: function (e) {
              if (e.key === "Enter") submitNow();
              if (e.key === "Escape") setStore({ query: "", result: null, error: null });
            },
          }),
          createElement("button", {
            type: "button",
            className: "wss-case" + (caseSensitive ? " wss-case-on" : ""),
            title: "Match case",
            onClick: function () { setStore({ caseSensitive: !caseSensitive }); },
          }, "Aa"),
          createElement("button", {
            type: "button",
            className: "wss-case" + (regex ? " wss-case-on" : ""),
            title: "Use regular expression",
            onClick: function () { setStore({ regex: !regex }); },
          }, ".*"),
          createElement("button", {
            type: "button",
            className: "wss-case" + (filtersOpen ? " wss-case-on" : ""),
            title: "Toggle include/exclude patterns",
            onClick: function () { setStore({ filtersOpen: !filtersOpen }); },
          }, "⌄")),
        filtersOpen ? createElement("div", { className: "wss-filters" },
          createElement("input", {
            className: "wss-filter-input",
            type: "text",
            placeholder: "files to include (e.g. src/**/*.ts, *.md)",
            value: include,
            spellCheck: false,
            onChange: function (e) { setStore({ include: e.target.value }); },
          }),
          createElement("input", {
            className: "wss-filter-input",
            type: "text",
            placeholder: "files to exclude (e.g. **/vendor/**, *.min.js)",
            value: exclude,
            spellCheck: false,
            onChange: function (e) { setStore({ exclude: e.target.value }); },
          })) : null,
        createElement("div", { className: "wss-status" },
          busy ? "Searching…"
            : error !== null ? ("Error: " + error)
              : result === null ? (root ? "Scope: " + root : "No workspace bound")
                : result.error === "invalid-regex" ? "Invalid regular expression"
                  : (result.results.length + " files · " + totalMatches + " matches"
                    + (result.truncatedFiles ? " · file scan truncated" : "")
                    + (result.truncatedMatches ? " · matches truncated" : ""))),
        createElement("div", { className: "wss-results" },
          result !== null && result.results.length === 0 ? createElement("div", { className: "wss-empty" }, "No matches")
            : result !== null ? result.results.map(function (r) {
              var open = expanded[r.path] === true;
              return createElement("div", { key: r.path, className: "wss-file" },
                createElement("button", {
                  type: "button",
                  className: "wss-filerow",
                  title: r.path,
                  onClick: function () {
                    var next = Object.assign({}, expanded);
                    next[r.path] = !open;
                    setStore({ expanded: next });
                  },
                },
                  createElement("span", { className: "wss-caret" }, open ? "▾" : "▸"),
                  createElement("span", { className: "wss-filename" }, r.name + (r.nameMatch ? "  ⚑" : "")),
                  createElement("span", { className: "wss-filedir" }, r.path),
                  r.matches.length > 0 ? createElement("span", { className: "wss-count" }, String(r.matches.length)) : null),
                open ? r.matches.map(function (m) {
                  return createElement("button", {
                    type: "button",
                    key: r.path + ":" + m.line,
                    className: "wss-match",
                    title: r.path + ":" + m.line,
                    onClick: function () { openFile(r.path); },
                  },
                    createElement("span", { className: "wss-lineno" }, String(m.line)),
                    createElement("span", { className: "wss-linetext" }, m.text));
                }) : null);
            }) : null));
    }

    function apply(ctx) {
      var connection = ctx.get("connection");
      wire.call = function (channel, endpoint, payload) {
        return connection.rpc.call(channel, endpoint, payload);
      };
      ctx.effect(function () {
        return ctx.betterSidebar.registerTab({
          id: "workspace-search:search",
          title: "Search",
          icon: createElement(UiPrimitives.IconSearchOutline16, { size: 14 }),
          order: 50,
          single: true,
          component: SearchView,
        });
      }, "workspace-search: sidebar tab");
    }
    exports.apply = apply;
    exports.inject = inject;
    return module.exports;
  }
});

// ── stylesheet (injected once) ──────────────────────────────────────────────
if (typeof document !== "undefined" && !document.getElementById("dsh-workspace-search-style")) {
  var style = document.createElement("style");
  style.id = "dsh-workspace-search-style";
  style.textContent = [
    ".wss-root{display:flex;flex-direction:column;height:100%;min-height:0;font-size:13px;color:#d6d6de;overflow:hidden}",
    ".wss-inputrow{display:flex;gap:6px;padding:8px 10px;flex:none}",
    ".wss-input{flex:1;min-width:0;height:30px;border:1px solid #33333d;border-radius:6px;background:#0f0f14;color:#e6e6ea;padding:0 10px;font-size:13px;outline:none;font-family:inherit}",
    ".wss-input:focus{border-color:#4a5a80}",
    ".wss-case{width:30px;height:30px;flex:none;border:1px solid #33333d;border-radius:6px;background:transparent;color:#9a9aa6;cursor:pointer;font-size:12px}",
    ".wss-case-on{background:#2a3040;color:#e6e6ea;border-color:#4a5a80}",
    ".wss-filters{display:flex;flex-direction:column;gap:6px;padding:0 10px 8px;flex:none}",
    ".wss-filter-input{height:28px;border:1px solid #33333d;border-radius:6px;background:#0f0f14;color:#c9c9d2;padding:0 8px;font-size:12px;outline:none;font-family:ui-monospace,SFMono-Regular,Menlo,Consolas,monospace}",
    ".wss-filter-input:focus{border-color:#4a5a80}",
    ".wss-status{flex:none;padding:0 12px 8px;color:#8a8a96;font-size:11.5px;overflow:hidden;text-overflow:ellipsis;white-space:nowrap}",
    ".wss-results{flex:1;min-height:0;overflow-y:auto;padding:0 6px 12px}",
    ".wss-empty{padding:18px 12px;color:#8a8a96}",
    ".wss-file{margin-bottom:1px}",
    ".wss-filerow{display:flex;align-items:center;gap:6px;width:100%;padding:5px 8px;border:none;border-radius:6px;background:transparent;color:#d6d6de;cursor:pointer;text-align:left;font-size:12.5px}",
    ".wss-filerow:hover{background:#22222b}",
    ".wss-caret{width:12px;flex:none;color:#8a8a96}",
    ".wss-filename{flex:none;max-width:40%;overflow:hidden;text-overflow:ellipsis;white-space:nowrap;font-weight:600}",
    ".wss-filedir{flex:1;min-width:0;overflow:hidden;text-overflow:ellipsis;white-space:nowrap;color:#7c7c88;font-family:ui-monospace,SFMono-Regular,Menlo,Consolas,monospace;font-size:11px;direction:rtl;text-align:left}",
    ".wss-count{flex:none;font-size:11px;color:#9aa6c4;background:#22222b;border-radius:8px;padding:0 6px;line-height:16px}",
    ".wss-match{display:flex;gap:8px;width:100%;padding:2px 8px 2px 26px;border:none;background:transparent;color:#c9c9d2;cursor:pointer;text-align:left;font-size:12px}",
    ".wss-match:hover{background:#22222b}",
    ".wss-lineno{flex:none;min-width:34px;text-align:right;color:#6f6f7a;font-family:ui-monospace,SFMono-Regular,Menlo,Consolas,monospace;font-size:11px;user-select:none}",
    ".wss-linetext{flex:1;min-width:0;overflow:hidden;text-overflow:ellipsis;white-space:nowrap;font-family:ui-monospace,SFMono-Regular,Menlo,Consolas,monospace;font-size:11.5px}",
  ].join("\n");
  document.head.appendChild(style);
}
