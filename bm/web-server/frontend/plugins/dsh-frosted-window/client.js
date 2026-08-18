window.__ModuleLoader__.load({
	id: "dsh-frosted-window",
	factory: (require) => {
		var module = { exports: {} };
		var exports = module.exports;
		Object.defineProperty(exports, Symbol.toStringTag, { value: "Module" });
		let react = require("react");
		let react_jsx_runtime = require("react/jsx-runtime");
		//#region src/client/constants.ts
		/** Package id — also the theme override layer source and the loader entry id. */
		const PACKAGE_ID = "dsh-frosted-window";
		/** Body attribute that scopes every injected style. */
		const BODY_ATTR = "data-dsh-frosted-window";
		/** Settings locale namespace. */
		const LOCALE_NS = "settings.frosted-window";
		/** localStorage key for knobs (never the image bytes). */
		const KNOBS_KEY = "dsh-frosted-window:knobs";
		/** IndexedDB database that holds the uploaded wallpaper blob. */
		const IMAGE_DB = "dsh-frosted-window";
		const IMAGE_STORE = "files";
		const IMAGE_KEY = "wallpaper";
		/** Allowed image MIME types at the upload boundary. */
		const ALLOWED_TYPES = [
			"image/jpeg",
			"image/png",
			"image/webp",
			"image/gif"
		];
		//#endregion
		//#region src/client/FrostedSection.tsx
		/**
		* Settings page: preview, sliders, explicit save / delete.
		* @param props - inject face from apply().
		*/
		function FrostedSection({ store, t, setEnabled, setKnob, upload, save, remove }) {
			const state = (0, react.useSyncExternalStore)(store.subscribe, store.get, store.get);
			const inputRef = (0, react.useRef)(null);
			const [over, setOver] = (0, react.useState)(false);
			const onFiles = (files) => {
				if (state.busy) return;
				const file = files?.[0];
				if (file === void 0) return;
				upload(file);
			};
			const pick = () => {
				if (!state.busy) inputRef.current?.click();
			};
			const previewStyle = {
				"--fw-ui-glass": String(state.glassOpacity),
				"--fw-ui-blur": `${state.blurPx}px`,
				"--fw-ui-sat": `${Math.round(state.saturate * 100)}%`
			};
			const meta = [state.fileName, state.width > 0 && state.height > 0 ? `${state.width}×${state.height}` : null].filter(Boolean).join(" · ");
			return /* @__PURE__ */ (0, react_jsx_runtime.jsx)("div", {
				className: "fw-section",
				children: /* @__PURE__ */ (0, react_jsx_runtime.jsxs)("div", {
					className: "fw-panel",
					children: [
						/* @__PURE__ */ (0, react_jsx_runtime.jsxs)("div", {
							className: "fw-head",
							children: [/* @__PURE__ */ (0, react_jsx_runtime.jsxs)("div", {
								className: "fw-lead",
								children: [
									/* @__PURE__ */ (0, react_jsx_runtime.jsx)("div", {
										className: "fw-kicker",
										children: "Theme"
									}),
									/* @__PURE__ */ (0, react_jsx_runtime.jsx)("div", {
										className: "fw-title",
										children: t("title")
									}),
									/* @__PURE__ */ (0, react_jsx_runtime.jsx)("div", {
										className: "fw-desc",
										children: t("description")
									})
								]
							}), /* @__PURE__ */ (0, react_jsx_runtime.jsx)("span", {
								className: "fw-chip",
								"data-tone": state.dirty ? "warn" : void 0,
								children: state.dirty ? t("unsaved") : t("saved")
							})]
						}),
						/* @__PURE__ */ (0, react_jsx_runtime.jsxs)("label", {
							className: "fw-switch",
							children: [/* @__PURE__ */ (0, react_jsx_runtime.jsx)("span", { children: t("enable") }), /* @__PURE__ */ (0, react_jsx_runtime.jsx)("input", {
								type: "checkbox",
								checked: state.enabled,
								onChange: (event) => {
									setEnabled(event.target.checked);
								}
							})]
						}),
						/* @__PURE__ */ (0, react_jsx_runtime.jsxs)("button", {
							type: "button",
							className: "fw-hero",
							style: previewStyle,
							"data-over": over ? "true" : "false",
							"data-has": state.hasImage ? "true" : "false",
							disabled: state.busy,
							onClick: pick,
							onDragOver: (event) => {
								event.preventDefault();
								setOver(true);
							},
							onDragLeave: () => {
								setOver(false);
							},
							onDrop: (event) => {
								event.preventDefault();
								setOver(false);
								onFiles(event.dataTransfer.files);
							},
							children: [
								state.previewUrl !== null ? /* @__PURE__ */ (0, react_jsx_runtime.jsx)("img", {
									src: state.previewUrl,
									alt: ""
								}) : null,
								state.hasImage ? /* @__PURE__ */ (0, react_jsx_runtime.jsx)("span", { className: "fw-hero-glass" }) : null,
								/* @__PURE__ */ (0, react_jsx_runtime.jsxs)("span", {
									className: "fw-hero-copy",
									children: [/* @__PURE__ */ (0, react_jsx_runtime.jsx)("strong", { children: state.busy ? t("busy") : state.hasImage ? t("dropReplace") : t("drop") }), /* @__PURE__ */ (0, react_jsx_runtime.jsx)("span", { children: state.hasImage ? meta || t("dropReplace") : t("empty") })]
								})
							]
						}),
						/* @__PURE__ */ (0, react_jsx_runtime.jsx)("input", {
							ref: inputRef,
							className: "fw-hidden",
							type: "file",
							accept: "image/jpeg,image/jpg,image/png,image/webp,image/gif,image/*",
							onChange: (event) => {
								onFiles(event.target.files);
								event.target.value = "";
							}
						}),
						state.error !== null ? /* @__PURE__ */ (0, react_jsx_runtime.jsx)("div", {
							className: "fw-error",
							role: "alert",
							children: state.error
						}) : null,
						/* @__PURE__ */ (0, react_jsx_runtime.jsxs)("div", {
							className: "fw-grid",
							children: [
								/* @__PURE__ */ (0, react_jsx_runtime.jsx)(Slider, {
									label: t("glass"),
									value: state.glassOpacity,
									min: .18,
									max: .82,
									step: .01,
									display: `${Math.round(state.glassOpacity * 100)}%`,
									onChange: (value) => {
										setKnob("glassOpacity", value);
									}
								}),
								/* @__PURE__ */ (0, react_jsx_runtime.jsx)(Slider, {
									label: t("blur"),
									value: state.blurPx,
									min: 8,
									max: 64,
									step: 1,
									display: `${Math.round(state.blurPx)}px`,
									onChange: (value) => {
										setKnob("blurPx", value);
									}
								}),
								/* @__PURE__ */ (0, react_jsx_runtime.jsx)(Slider, {
									label: t("saturate"),
									value: state.saturate,
									min: 1,
									max: 2,
									step: .01,
									display: `${Math.round(state.saturate * 100)}%`,
									onChange: (value) => {
										setKnob("saturate", value);
									}
								}),
								/* @__PURE__ */ (0, react_jsx_runtime.jsx)(Slider, {
									label: t("dim"),
									value: state.dim,
									min: 0,
									max: .65,
									step: .01,
									display: `${Math.round(state.dim * 100)}%`,
									onChange: (value) => {
										setKnob("dim", value);
									}
								})
							]
						}),
						/* @__PURE__ */ (0, react_jsx_runtime.jsxs)("div", {
							className: "fw-bar",
							children: [
								/* @__PURE__ */ (0, react_jsx_runtime.jsx)("button", {
									type: "button",
									className: "fw-btn",
									"data-kind": "danger",
									disabled: !state.hasImage || state.busy,
									onClick: () => {
										remove();
									},
									children: t("remove")
								}),
								/* @__PURE__ */ (0, react_jsx_runtime.jsx)("button", {
									type: "button",
									className: "fw-btn",
									onClick: pick,
									disabled: state.busy,
									children: t("choose")
								}),
								/* @__PURE__ */ (0, react_jsx_runtime.jsx)("button", {
									type: "button",
									className: "fw-btn",
									"data-kind": "primary",
									disabled: !state.dirty || state.busy,
									onClick: () => {
										save();
									},
									children: t("save")
								})
							]
						})
					]
				})
			});
		}
		function Slider(props) {
			return /* @__PURE__ */ (0, react_jsx_runtime.jsxs)("label", {
				className: "fw-row",
				children: [/* @__PURE__ */ (0, react_jsx_runtime.jsxs)("span", {
					className: "fw-row-head",
					children: [/* @__PURE__ */ (0, react_jsx_runtime.jsx)("span", { children: props.label }), /* @__PURE__ */ (0, react_jsx_runtime.jsx)("span", { children: props.display })]
				}), /* @__PURE__ */ (0, react_jsx_runtime.jsx)("input", {
					type: "range",
					min: props.min,
					max: props.max,
					step: props.step,
					value: props.value,
					onChange: (event) => {
						props.onChange(Number(event.target.value));
					}
				})]
			});
		}
		//#endregion
		//#region src/client/image.ts
		/** Build a Blob for object URLs, keeping the original encoded type. */
		function wallpaperBlob(record) {
			const type = ALLOWED_TYPES.includes(record.mime) ? record.mime : "image/jpeg";
			return new Blob([Uint8Array.from(record.bytes)], { type });
		}
		var ImageValidationError = class extends Error {
			name = "ImageValidationError";
		};
		const MAGIC = [
			{
				mime: "image/jpeg",
				test: (b) => b[0] === 255 && b[1] === 216 && b[2] === 255
			},
			{
				mime: "image/png",
				test: (b) => b[0] === 137 && b[1] === 80 && b[2] === 78 && b[3] === 71
			},
			{
				mime: "image/gif",
				test: (b) => b[0] === 71 && b[1] === 73 && b[2] === 70 && b[3] === 56
			},
			{
				mime: "image/webp",
				test: (b) => b[0] === 82 && b[1] === 73 && b[2] === 70 && b[3] === 70 && b[8] === 87 && b[9] === 69 && b[10] === 66 && b[11] === 80
			}
		];
		function normalizeImageType(type) {
			if (type === "image/jpg") return "image/jpeg";
			return ALLOWED_TYPES.find((allowed) => allowed === type);
		}
		/**
		* Reject files that are not a supported image. Size is intentionally unbounded.
		* @param file - browser File from an <input> or drop.
		*/
		function assertImageFile(file) {
			if (file.size <= 0) throw new ImageValidationError("empty image");
			if (file.type !== "" && normalizeImageType(file.type) === void 0) throw new ImageValidationError(`unsupported type: ${file.type}`);
		}
		/** Confirm the file header matches a declared or inferred image type. */
		async function assertImageMagic(file) {
			const header = new Uint8Array(await readFileBytes(file.slice(0, 16)));
			const declared = normalizeImageType(file.type);
			const match = MAGIC.find((entry) => entry.test(header));
			if (match === void 0) throw new ImageValidationError(`unsupported type: ${file.type || "unknown"}`);
			if (declared !== void 0 && declared !== match.mime) throw new ImageValidationError(`unsupported type: ${file.type}`);
			return match.mime;
		}
		/** Read width/height from the container when the header is well-formed. */
		function readImageSize(bytes) {
			if (bytes.length >= 24 && MAGIC[1].test(bytes)) return {
				width: readU32(bytes, 16),
				height: readU32(bytes, 20)
			};
			if (bytes.length >= 10 && MAGIC[2].test(bytes)) return {
				width: bytes[6] | bytes[7] << 8,
				height: bytes[8] | bytes[9] << 8
			};
			if (bytes.length >= 30 && MAGIC[3].test(bytes)) return readWebpSize(bytes);
			if (bytes.length >= 4 && MAGIC[0].test(bytes)) return readJpegSize(bytes);
		}
		/** Accept a record that came back from IndexedDB. */
		function sanitizeWallpaperRecord(raw) {
			if (raw === null || typeof raw !== "object") return void 0;
			const value = raw;
			if (!Array.isArray(value.bytes) || value.bytes.length < 1) return void 0;
			const mime = normalizeImageType(String(value.mime ?? "")) ?? "image/jpeg";
			const width = Number(value.width);
			const height = Number(value.height);
			return {
				bytes: value.bytes.map((item) => Number(item) & 255),
				mime,
				name: typeof value.name === "string" ? value.name : "wallpaper",
				width: Number.isFinite(width) && width > 0 ? width : 0,
				height: Number.isFinite(height) && height > 0 ? height : 0,
				updatedAt: Number(value.updatedAt) || 0
			};
		}
		/**
		* Keep the original encoded image. No resize and no byte cap.
		*/
		async function prepareWallpaper(file) {
			assertImageFile(file);
			const mime = await assertImageMagic(file);
			const source = new Uint8Array(await readFileBytes(file));
			const declared = readImageSize(source);
			return {
				bytes: Array.from(source),
				mime,
				name: file.name,
				width: declared?.width ?? 0,
				height: declared?.height ?? 0,
				updatedAt: Date.now()
			};
		}
		function readU32(bytes, offset) {
			return (bytes[offset] << 24 | bytes[offset + 1] << 16 | bytes[offset + 2] << 8 | bytes[offset + 3]) >>> 0;
		}
		function readWebpSize(bytes) {
			const fourcc = String.fromCharCode(bytes[12], bytes[13], bytes[14], bytes[15]);
			if (fourcc === "VP8X" && bytes.length >= 30) return {
				width: 1 + (bytes[24] | bytes[25] << 8 | bytes[26] << 16),
				height: 1 + (bytes[27] | bytes[28] << 8 | bytes[29] << 16)
			};
			if (fourcc === "VP8 " && bytes.length >= 30 && bytes[23] === 157 && bytes[24] === 1 && bytes[25] === 42) return {
				width: (bytes[26] | bytes[27] << 8) & 16383,
				height: (bytes[28] | bytes[29] << 8) & 16383
			};
			if (fourcc === "VP8L" && bytes.length >= 25 && bytes[20] === 47) {
				const bits = bytes[21] | bytes[22] << 8 | bytes[23] << 16 | bytes[24] << 24;
				return {
					width: (bits & 16383) + 1,
					height: (bits >> 14 & 16383) + 1
				};
			}
		}
		function readJpegSize(bytes) {
			let offset = 2;
			while (offset + 8 < bytes.length) {
				if (bytes[offset] !== 255) return void 0;
				const marker = bytes[offset + 1];
				offset += 2;
				if (marker === 216 || marker === 217 || marker >= 208 && marker <= 215) continue;
				const length = bytes[offset] << 8 | bytes[offset + 1];
				if (length < 2) return void 0;
				if (marker >= 192 && marker <= 207 && marker !== 196 && marker !== 200 && marker !== 204) return {
					height: bytes[offset + 3] << 8 | bytes[offset + 4],
					width: bytes[offset + 5] << 8 | bytes[offset + 6]
				};
				offset += length;
			}
		}
		function readFileBytes(file) {
			if (typeof file.arrayBuffer === "function") return file.arrayBuffer();
			return new Promise((resolve, reject) => {
				const reader = new FileReader();
				reader.onload = () => {
					resolve(reader.result);
				};
				reader.onerror = () => {
					reject(reader.error ?? /* @__PURE__ */ new Error("failed to read image"));
				};
				reader.readAsArrayBuffer(file);
			});
		}
		//#endregion
		//#region src/client/image-store.ts
		/**
		* Open the IndexedDB-backed store. The database is created on first use.
		*/
		function openImageStore() {
			return {
				get: async () => {
					return hydrate(await withStore("readonly", (store) => requestToPromise(store.get(IMAGE_KEY))));
				},
				put: (record) => withStore("readwrite", (store) => requestToPromise(store.put({
					bytes: record.bytes,
					mime: record.mime,
					name: record.name,
					width: record.width,
					height: record.height,
					updatedAt: record.updatedAt
				}, IMAGE_KEY))),
				clear: () => withStore("readwrite", (store) => requestToPromise(store.delete(IMAGE_KEY)))
			};
		}
		function hydrate(stored) {
			return sanitizeWallpaperRecord(stored);
		}
		async function withStore(mode, use) {
			const db = await openDb();
			try {
				const tx = db.transaction(IMAGE_STORE, mode);
				const result = await use(tx.objectStore(IMAGE_STORE));
				await txDone(tx);
				return result;
			} finally {
				db.close();
			}
		}
		function openDb() {
			return new Promise((resolve, reject) => {
				const req = indexedDB.open(IMAGE_DB, 1);
				req.onupgradeneeded = () => {
					if (!req.result.objectStoreNames.contains("files")) req.result.createObjectStore(IMAGE_STORE);
				};
				req.onsuccess = () => {
					resolve(req.result);
				};
				req.onerror = () => {
					reject(req.error ?? /* @__PURE__ */ new Error("indexedDB open failed"));
				};
			});
		}
		function requestToPromise(req) {
			return new Promise((resolve, reject) => {
				req.onsuccess = () => {
					resolve(req.result);
				};
				req.onerror = () => {
					reject(req.error ?? /* @__PURE__ */ new Error("indexedDB request failed"));
				};
			});
		}
		function txDone(tx) {
			return new Promise((resolve, reject) => {
				tx.oncomplete = () => {
					resolve();
				};
				tx.onerror = () => {
					reject(tx.error ?? /* @__PURE__ */ new Error("indexedDB transaction failed"));
				};
				tx.onabort = () => {
					reject(tx.error ?? /* @__PURE__ */ new Error("indexedDB transaction aborted"));
				};
			});
		}
		//#endregion
		//#region src/client/knobs.ts
		const DEFAULT_KNOBS = {
			enabled: true,
			glassOpacity: .46,
			blurPx: 28,
			saturate: 1.55,
			dim: .28
		};
		const RANGES = {
			glassOpacity: [.18, .82],
			blurPx: [8, 64],
			saturate: [1, 2],
			dim: [0, .65]
		};
		/**
		* Clamp one numeric knob into its published range.
		* @param key - numeric knob name.
		* @param value - raw number.
		*/
		function clampKnob(key, value) {
			const [min, max] = RANGES[key];
			if (!Number.isFinite(value)) return DEFAULT_KNOBS[key];
			return Math.min(max, Math.max(min, value));
		}
		/**
		* Normalize a partial / unknown record into a complete knob set.
		* @param raw - persisted JSON or UI draft.
		*/
		function normalizeKnobs(raw) {
			const input = raw !== null && typeof raw === "object" ? raw : {};
			return {
				enabled: input.enabled !== false,
				glassOpacity: clampKnob("glassOpacity", Number(input.glassOpacity)),
				blurPx: clampKnob("blurPx", Number(input.blurPx)),
				saturate: clampKnob("saturate", Number(input.saturate)),
				dim: clampKnob("dim", Number(input.dim))
			};
		}
		/** Read knobs from localStorage; missing or corrupt values become defaults. */
		function loadKnobs() {
			try {
				const raw = localStorage.getItem(KNOBS_KEY);
				if (raw === null) return { ...DEFAULT_KNOBS };
				return normalizeKnobs(JSON.parse(raw));
			} catch {
				return { ...DEFAULT_KNOBS };
			}
		}
		/** Persist a complete knob set. Failures stay local (private mode / quota). */
		function saveKnobs(knobs) {
			try {
				localStorage.setItem(KNOBS_KEY, JSON.stringify(normalizeKnobs(knobs)));
			} catch {}
		}
		//#endregion
		//#region src/client/locales.ts
		/** Settings copy. Chinese is the key-set source of truth. */
		const zh = {
			nav: "磨砂主题",
			title: "磨砂玻璃窗口",
			description: "选一张图铺满整个窗口，界面以半透明磨砂叠上去。浅色 / 深色仍跟随官方外观。",
			enable: "启用主题",
			drop: "把图片拖到这里，或点击选择",
			dropReplace: "更换图片",
			choose: "选择图片",
			save: "保存",
			saved: "已保存",
			unsaved: "未保存",
			remove: "删除",
			glass: "玻璃浓度",
			blur: "磨砂模糊",
			saturate: "色彩饱和",
			dim: "壁纸压暗",
			busy: "正在读取图片…",
			empty: "还没有壁纸",
			errorType: "只支持 JPEG、PNG、WebP 或 GIF。",
			errorGeneric: "无法读取这张图片，请换一张再试。"
		};
		const en = {
			nav: "Frosted",
			title: "Frosted window",
			description: "Pick an image for the whole window. The chrome sits on it as frosted glass and still follows official Light / Dark.",
			enable: "Enable",
			drop: "Drop an image here, or click to choose",
			dropReplace: "Replace image",
			choose: "Choose image",
			save: "Save",
			saved: "Saved",
			unsaved: "Unsaved",
			remove: "Delete",
			glass: "Glass",
			blur: "Blur",
			saturate: "Saturation",
			dim: "Dim",
			busy: "Reading image…",
			empty: "No wallpaper yet",
			errorType: "JPEG, PNG, WebP, or GIF only.",
			errorGeneric: "Could not read that image. Try another file."
		};
		//#endregion
		//#region src/client/store.ts
		const INITIAL_STATE = {
			...DEFAULT_KNOBS,
			hasImage: false,
			previewUrl: null,
			fileName: null,
			width: 0,
			height: 0,
			dirty: false,
			busy: false,
			error: null,
			revision: -1
		};
		/** Create an in-memory store for the settings section. */
		function createFrostedStore(init = INITIAL_STATE) {
			let state = init;
			const listeners = /* @__PURE__ */ new Set();
			return {
				get: () => state,
				set: (next) => {
					state = next;
					for (const listener of listeners) listener();
				},
				subscribe: (listener) => {
					listeners.add(listener);
					return () => {
						listeners.delete(listener);
					};
				}
			};
		}
		//#endregion
		//#region src/client/tokens.ts
		const pair = (light, dark) => ({
			light,
			dark
		});
		const rgba = (rgb, alpha) => `rgba(${rgb}, ${Number(alpha.toFixed(3))})`;
		/**
		* Build a reversible override layer that turns official opaque fills into
		* frosted plates. Both palettes are always supplied so a scheme switch
		* cannot leave a token illegible (ThemeRuntime contract).
		* @param knobs - current glass opacity.
		*/
		function glassTokenOverrides(knobs) {
			const a = knobs.glassOpacity;
			const aBase = Math.max(.08, a * .42);
			const aRaised = Math.min(.92, a + .1);
			const aOverlay = Math.min(.94, a + .22);
			const aInput = Math.min(.9, a + .12);
			const aMenu = Math.min(.9, a + .16);
			const aBubble = Math.min(.88, a + .08);
			const aHover = Math.min(.55, a * .7);
			return {
				"--dsw-alias-bg-base": pair(rgba("255, 255, 255", aBase), rgba("15, 17, 21", aBase)),
				"--dsw-alias-bg-layer-1": pair(rgba("255, 255, 255", a), rgba("27, 27, 28", a)),
				"--dsw-alias-bg-layer-2": pair(rgba("255, 255, 255", aRaised), rgba("33, 33, 35", aRaised)),
				"--dsw-alias-bg-layer-3": pair(rgba("248, 250, 252", aRaised), rgba("41, 41, 41", aRaised)),
				"--dsw-alias-bg-overlay": pair(rgba("255, 255, 255", aOverlay), rgba("44, 44, 46", aOverlay)),
				"--dsw-alias-bg-module-platform": pair(rgba("245, 246, 247", aRaised), rgba("53, 54, 56", aRaised)),
				"--dsw-specific-sidebar-fill": pair(rgba("249, 250, 251", a), rgba("21, 21, 23", a)),
				"--dsw-specific-input-major": pair(rgba("255, 255, 255", aInput), rgba("33, 33, 35", aInput)),
				"--dsw-specific-menu": pair(rgba("255, 255, 255", aMenu), rgba("41, 41, 41", aMenu)),
				"--dsw-specific-bubble": pair(rgba("237, 243, 254", aBubble), rgba("33, 33, 35", aBubble)),
				"--dsw-specific-selector": pair(rgba("245, 246, 247", aRaised), rgba("53, 54, 56", aRaised)),
				"--dsw-alias-button-elevated-fill": pair(rgba("255, 255, 255", aInput), rgba("67, 69, 74", aInput)),
				"--dsw-alias-button-floating-fill": pair(rgba("255, 255, 255", aInput), rgba("33, 33, 35", aInput)),
				"--dsw-alias-markdown-code-block": pair(rgba("250, 250, 251", aRaised), rgba("15, 15, 15", aRaised)),
				"--dsw-alias-markdown-inline-code": pair(rgba("245, 246, 247", aRaised), rgba("33, 33, 35", aRaised)),
				"--dsw-specific-sidebar-nav-item-active": pair(rgba("235, 238, 242", aRaised), rgba("67, 69, 74", aRaised)),
				"--dsw-specific-sidebar-nav-item-hover": pair(rgba("241, 243, 245", aHover), rgba("33, 33, 35", aHover)),
				"--dsw-alias-bg-mask-drop": pair(rgba("255, 255, 255", .45), rgba("15, 17, 21", .45))
			};
		}
		//#endregion
		//#region src/client/glass-css.ts
		/**
		* Scoped glass stylesheet. Every rule hangs off the plugin body attribute so
		* dispose is one attribute removal + one style-tag removal. Selectors use
		* official `data-slot` names, never hashed CSS-module class names.
		*/
		const GLASS_CSS = `
[${BODY_ATTR}-wallpaper] {
  position: fixed;
  inset: 0;
  z-index: -2;
  pointer-events: none;
  background-repeat: no-repeat;
  background-position: center;
  background-size: cover;
}

[${BODY_ATTR}-dim] {
  position: fixed;
  inset: 0;
  z-index: -1;
  pointer-events: none;
  background: rgba(12, 16, 24, var(--fw-dim, 0.28));
}

body[${BODY_ATTR}] {
  --fw-blur: 28px;
  --fw-saturate: 155%;
  --fw-dim: 0.28;
  --fw-highlight: rgba(255, 255, 255, 0.55);
  --fw-edge: rgba(255, 255, 255, 0.28);
  background-color: transparent;
}

body[${BODY_ATTR}='dark'] [${BODY_ATTR}-dim] {
  background: rgba(6, 8, 12, var(--fw-dim, 0.28));
}

body[${BODY_ATTR}='dark'] {
  --fw-highlight: rgba(255, 255, 255, 0.14);
  --fw-edge: rgba(255, 255, 255, 0.12);
}

/* AppFrame + slot roots: drop opaque fills so the wallpaper shows through. */
body[${BODY_ATTR}] *:has(> [data-slot='sidebar']):has(> [data-slot='conversation']),
body[${BODY_ATTR}] [data-slot='sidebar'],
body[${BODY_ATTR}] [data-slot='sidebar'] > :first-child,
body[${BODY_ATTR}] [data-slot='conversation'],
body[${BODY_ATTR}] [data-slot='details'] {
  background-color: transparent !important;
}

/*
 * One frosted plate per column, painted on ::before.
 * backdrop-filter must stay on the pseudo — never on the column itself —
 * or position:fixed settings (a sidebar descendant) lock to the rail width.
 */
body[${BODY_ATTR}] *:has(> [data-slot='sidebar']),
body[${BODY_ATTR}] *:has(> [data-slot='conversation']),
body[${BODY_ATTR}] *:has(> [data-slot='details']) {
  position: relative;
}
body[${BODY_ATTR}] *:has(> [data-slot='sidebar']) {
  border-right: none !important;
}
body[${BODY_ATTR}] *:has(> [data-slot='details']) {
  border-left: none !important;
}
body[${BODY_ATTR}] *:has(> [data-slot='sidebar'])::before,
body[${BODY_ATTR}] *:has(> [data-slot='conversation'])::before,
body[${BODY_ATTR}] *:has(> [data-slot='details'])::before {
  content: '';
  position: absolute;
  z-index: -1;
  pointer-events: none;
  background: var(--dsw-alias-bg-layer-1);
  -webkit-backdrop-filter: blur(var(--fw-blur)) saturate(var(--fw-saturate));
  backdrop-filter: blur(var(--fw-blur)) saturate(var(--fw-saturate));
}
body[${BODY_ATTR}] *:has(> [data-slot='sidebar'])::before {
  inset: 0 -2px 0 0;
  background: var(--dsw-specific-sidebar-fill);
}
body[${BODY_ATTR}] *:has(> [data-slot='conversation'])::before {
  inset: 0 0 0 -2px;
}
body[${BODY_ATTR}] *:has(> [data-slot='details'])::before {
  inset: 0 0 0 -2px;
}

@media (prefers-reduced-transparency: reduce) {
  body[${BODY_ATTR}] *:has(> [data-slot='sidebar'])::before,
  body[${BODY_ATTR}] *:has(> [data-slot='conversation'])::before,
  body[${BODY_ATTR}] *:has(> [data-slot='details'])::before {
    -webkit-backdrop-filter: none;
    backdrop-filter: none;
  }
}

/* Settings page */
.fw-section {
  display: flex;
  flex-direction: column;
  gap: 16px;
  padding: 4px 0 28px;
  min-width: 0;
  max-width: 100%;
  color: var(--dsw-alias-label-primary);
}
.fw-panel {
  display: flex;
  flex-direction: column;
  gap: 18px;
  padding: 18px;
  border-radius: 20px;
  border: 1px solid var(--dsw-alias-border-l2);
  background:
    linear-gradient(180deg, rgba(255,255,255,0.08), rgba(255,255,255,0.02)),
    var(--dsw-alias-bg-layer-1);
  box-shadow: inset 0 1px 0 rgba(255,255,255,0.28);
}
body[data-ds-dark-theme] .fw-panel {
  background:
    linear-gradient(180deg, rgba(255,255,255,0.06), rgba(255,255,255,0.01)),
    var(--dsw-alias-bg-layer-1);
  box-shadow: inset 0 1px 0 rgba(255,255,255,0.08);
}
.fw-head {
  display: flex;
  align-items: flex-start;
  justify-content: space-between;
  gap: 12px;
}
.fw-lead { display: flex; flex-direction: column; gap: 4px; min-width: 0; }
.fw-kicker {
  font-size: 11px;
  letter-spacing: 0.08em;
  text-transform: uppercase;
  color: var(--dsw-alias-label-tertiary);
}
.fw-title { font-size: 18px; line-height: 26px; font-weight: 600; }
.fw-desc { font-size: 13px; line-height: 20px; color: var(--dsw-alias-label-secondary); }
.fw-chip {
  flex: 0 0 auto;
  margin-top: 4px;
  padding: 3px 8px;
  border-radius: 999px;
  font-size: 11px;
  line-height: 16px;
  background: var(--dsw-alias-interactive-bg-hover);
  color: var(--dsw-alias-label-secondary);
}
.fw-chip[data-tone='warn'] {
  background: color-mix(in srgb, var(--dsw-alias-state-warn-primary) 18%, transparent);
  color: var(--dsw-alias-state-warn-label, var(--dsw-alias-state-warn-primary));
}
.fw-hero {
  position: relative;
  isolation: isolate;
  overflow: hidden;
  width: 100%;
  max-width: 100%;
  min-height: 196px;
  border: 0;
  border-radius: 16px;
  padding: 0;
  background:
    radial-gradient(circle at 20% 20%, rgba(255,255,255,0.18), transparent 42%),
    linear-gradient(135deg, #8aa4c8 0%, #3d4f6b 52%, #1b2330 100%);
  color: inherit;
  font: inherit;
  cursor: pointer;
}
.fw-hero[data-over='true'] { outline: 2px solid var(--dsw-alias-brand-primary); outline-offset: 2px; }
.fw-hero:disabled { cursor: default; }
.fw-hero img {
  position: absolute;
  inset: 0;
  width: 100%;
  height: 100%;
  object-fit: cover;
}
.fw-hero-glass {
  position: absolute;
  inset: auto 18px 18px auto;
  width: 42%;
  min-width: 120px;
  height: 46%;
  border-radius: 14px;
  border: 1px solid rgba(255,255,255,0.35);
  background: rgba(255,255,255, var(--fw-ui-glass, 0.46));
  -webkit-backdrop-filter: blur(var(--fw-ui-blur, 28px)) saturate(var(--fw-ui-sat, 155%));
  backdrop-filter: blur(var(--fw-ui-blur, 28px)) saturate(var(--fw-ui-sat, 155%));
  box-shadow: inset 0 1px 0 rgba(255,255,255,0.55);
  pointer-events: none;
}
.fw-hero-copy {
  position: relative;
  z-index: 1;
  display: flex;
  flex-direction: column;
  align-items: flex-start;
  gap: 6px;
  padding: 20px;
  text-align: left;
}
.fw-hero-copy strong { font-size: 14px; line-height: 20px; font-weight: 600; }
.fw-hero-copy span {
  font-size: 12px;
  line-height: 18px;
  color: var(--dsw-alias-label-secondary);
}
.fw-hero:not([data-has='true']) .fw-hero-copy strong,
.fw-hero:not([data-has='true']) .fw-hero-copy span { color: #f4f7fb; }
.fw-switch {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
  font-size: 14px;
  line-height: 22px;
}
.fw-switch input {
  appearance: none;
  -webkit-appearance: none;
  width: 44px;
  height: 26px;
  margin: 0;
  border: 0;
  border-radius: 999px;
  background: #6b7178;
  position: relative;
  cursor: pointer;
  transition: background 160ms ease;
}
.fw-switch input::after {
  content: '';
  position: absolute;
  top: 3px;
  left: 3px;
  width: 20px;
  height: 20px;
  border-radius: 50%;
  background: #fff;
  box-shadow: 0 1px 3px rgba(0,0,0,0.28);
  transition: transform 160ms ease;
}
.fw-switch input:checked {
  background: #34c759;
}
.fw-switch input:checked::after { transform: translateX(18px); }
.fw-grid {
  display: grid;
  grid-template-columns: 1fr 1fr;
  gap: 14px 16px;
}
@media (max-width: 640px) { .fw-grid { grid-template-columns: 1fr; } }
.fw-row { display: flex; flex-direction: column; gap: 8px; }
.fw-row-head {
  display: flex;
  justify-content: space-between;
  font-size: 12px;
  line-height: 18px;
}
.fw-row-head span:last-child { color: var(--dsw-alias-label-secondary); font-variant-numeric: tabular-nums; }
.fw-row input[type='range'] {
  width: 100%;
  height: 4px;
  accent-color: var(--dsw-alias-brand-primary);
  cursor: pointer;
}
.fw-bar {
  display: flex;
  justify-content: flex-end;
  gap: 8px;
  padding-top: 4px;
}
.fw-btn {
  appearance: none;
  border: 1px solid var(--dsw-alias-border-l2);
  border-radius: 12px;
  background: transparent;
  color: var(--dsw-alias-label-primary);
  font: inherit;
  font-size: 13px;
  line-height: 20px;
  padding: 8px 14px;
  cursor: pointer;
}
.fw-btn:hover { background: var(--dsw-alias-interactive-bg-hover); }
.fw-btn[data-kind='primary'] {
  background: #34c759;
  color: #fff;
  border-color: transparent;
}
.fw-btn[data-kind='primary']:hover { background: #2fb350; }
.fw-btn[data-kind='primary']:disabled {
  background: transparent;
  color: var(--dsw-alias-label-primary);
  border-color: var(--dsw-alias-border-l2);
  opacity: 0.5;
}
.fw-btn[data-kind='danger'] { color: var(--dsw-alias-state-error-primary); }
.fw-btn:disabled { opacity: 0.5; cursor: default; }
.fw-error {
  font-size: 13px;
  line-height: 20px;
  color: var(--dsw-alias-state-error-primary);
}
.fw-hidden { position: absolute; width: 1px; height: 1px; overflow: hidden; clip: rect(0 0 0 0); }
`.trim();
		//#endregion
		//#region src/client/wallpaper.ts
		/**
		* Owns the wallpaper plate, dim veil, scoped stylesheet, and body attribute.
		* Retracts only what it wrote (ThemePresenter contract).
		*/
		var FrostedPresenter = class {
			styleEl;
			wallpaperEl;
			dimEl;
			objectUrl;
			/** Project one surface onto the document. Passing a disabled/empty surface retracts. */
			apply(surface) {
				if (!(surface.knobs.enabled && surface.objectUrl !== null)) {
					this.retractChrome();
					return;
				}
				this.ensureChrome();
				const wallpaper = this.wallpaperEl;
				const dim = this.dimEl;
				if (wallpaper === void 0 || dim === void 0) return;
				wallpaper.style.backgroundImage = `url(${JSON.stringify(surface.objectUrl)})`;
				document.body.setAttribute(BODY_ATTR, surface.scheme);
				document.body.style.setProperty("--fw-blur", `${surface.knobs.blurPx}px`);
				document.body.style.setProperty("--fw-saturate", `${Math.round(surface.knobs.saturate * 100)}%`);
				document.body.style.setProperty("--fw-dim", String(surface.knobs.dim));
			}
			/** Remember a blob URL so dispose can revoke it. Callers revoke the previous URL after React paints. */
			adoptObjectUrl(url) {
				this.objectUrl = url;
			}
			/** Current adopted object URL, if any. */
			currentObjectUrl() {
				return this.objectUrl;
			}
			/** Retract every node, attribute, custom property, and object URL. */
			dispose() {
				this.retractChrome();
				if (this.objectUrl !== void 0) {
					URL.revokeObjectURL(this.objectUrl);
					this.objectUrl = void 0;
				}
			}
			ensureChrome() {
				if (this.styleEl === void 0 || !this.styleEl.isConnected) {
					const style = document.createElement("style");
					style.dataset.plugin = PACKAGE_ID;
					style.textContent = GLASS_CSS;
					document.head.append(style);
					this.styleEl = style;
				}
				if (this.wallpaperEl === void 0 || !this.wallpaperEl.isConnected) {
					const plate = document.createElement("div");
					plate.setAttribute(`${BODY_ATTR}-wallpaper`, "");
					document.body.prepend(plate);
					this.wallpaperEl = plate;
				}
				if (this.dimEl === void 0 || !this.dimEl.isConnected) {
					const veil = document.createElement("div");
					veil.setAttribute(`${BODY_ATTR}-dim`, "");
					this.wallpaperEl.after(veil);
					this.dimEl = veil;
				}
			}
			retractChrome() {
				this.styleEl?.remove();
				this.styleEl = void 0;
				this.wallpaperEl?.remove();
				this.wallpaperEl = void 0;
				this.dimEl?.remove();
				this.dimEl = void 0;
				document.body.removeAttribute(BODY_ATTR);
				document.body.style.removeProperty("--fw-blur");
				document.body.style.removeProperty("--fw-saturate");
				document.body.style.removeProperty("--fw-dim");
			}
		};
		//#endregion
		//#region src/client/index.ts
		const name = PACKAGE_ID;
		const inject = [
			"slots",
			"locale",
			"theme"
		];
		/** Client plugin body. */
		function apply(ctx) {
			const images = openImageStore();
			const presenter = new FrostedPresenter();
			const store = createFrostedStore({
				...loadKnobs(),
				hasImage: false,
				previewUrl: null,
				fileName: null,
				width: 0,
				height: 0,
				dirty: false,
				busy: false,
				error: null,
				revision: 0
			});
			let knobs = loadKnobs();
			let draft;
			let disposeTokens;
			let mutation = 0;
			let disposed = false;
			let projecting = false;
			const t = (key) => {
				try {
					return ctx.locale.bind(LOCALE_NS)(key);
				} catch {
					return zh[key];
				}
			};
			const publish = (patch) => {
				const current = store.get();
				store.set({
					...current,
					...patch,
					revision: current.revision + 1
				});
			};
			const schemeOf = () => {
				try {
					return ctx.theme.getTheme?.()?.active?.colorScheme === "dark" ? "dark" : "light";
				} catch {
					return "light";
				}
			};
			const stackTokens = (live) => {
				if (typeof disposeTokens === "function") disposeTokens();
				disposeTokens = void 0;
				if (!live || typeof ctx.theme.overrideTokens !== "function") return;
				const retract = ctx.theme.overrideTokens(PACKAGE_ID, glassTokenOverrides(knobs));
				disposeTokens = typeof retract === "function" ? retract : void 0;
			};
			const projectChrome = () => {
				if (disposed || projecting) return;
				projecting = true;
				try {
					const preview = store.get().previewUrl;
					presenter.apply({
						knobs,
						objectUrl: preview,
						scheme: schemeOf()
					});
				} finally {
					projecting = false;
				}
			};
			const project = (restack) => {
				if (disposed) return;
				const live = knobs.enabled && store.get().previewUrl !== null;
				projectChrome();
				if (restack) stackTokens(live);
			};
			const persistKnobs = (next) => {
				knobs = normalizeKnobs(next);
				publish({
					...knobs,
					dirty: true
				});
				project(true);
			};
			const adoptRecord = (record, dirty) => {
				if (disposed) return;
				const previous = presenter.currentObjectUrl();
				draft = record;
				if (record === void 0) {
					publish({
						hasImage: false,
						previewUrl: null,
						fileName: null,
						width: 0,
						height: 0,
						dirty,
						error: null
					});
					presenter.adoptObjectUrl(void 0);
					if (previous !== void 0) requestAnimationFrame(() => {
						URL.revokeObjectURL(previous);
					});
					project(true);
					return;
				}
				const url = URL.createObjectURL(wallpaperBlob(record));
				publish({
					hasImage: true,
					previewUrl: url,
					fileName: record.name,
					width: record.width,
					height: record.height,
					dirty,
					error: null
				});
				presenter.adoptObjectUrl(url);
				if (previous !== void 0 && previous !== url) requestAnimationFrame(() => {
					URL.revokeObjectURL(previous);
				});
				project(true);
			};
			const upload = async (file) => {
				const generation = ++mutation;
				publish({
					busy: true,
					error: null
				});
				try {
					const record = await prepareWallpaper(file);
					if (generation !== mutation || disposed) return;
					adoptRecord(record, true);
				} catch (error) {
					if (generation !== mutation || disposed) return;
					publish({ error: messageFor(error, t) });
				} finally {
					if (generation === mutation && !disposed) publish({ busy: false });
				}
			};
			const save = async () => {
				const generation = ++mutation;
				publish({
					busy: true,
					error: null
				});
				try {
					saveKnobs(knobs);
					if (draft === void 0) await images.clear();
					else await images.put(draft);
					if (generation !== mutation || disposed) return;
					publish({ dirty: false });
				} catch (error) {
					if (generation !== mutation || disposed) return;
					publish({ error: messageFor(error, t) });
				} finally {
					if (generation === mutation && !disposed) publish({ busy: false });
				}
			};
			const remove = async () => {
				const generation = ++mutation;
				publish({
					busy: true,
					error: null
				});
				try {
					await images.clear();
					saveKnobs(knobs);
					if (generation !== mutation || disposed) return;
					adoptRecord(void 0, false);
				} catch (error) {
					if (generation !== mutation || disposed) return;
					publish({ error: messageFor(error, t) });
				} finally {
					if (generation === mutation && !disposed) publish({ busy: false });
				}
			};
			ctx.effect(() => ctx.locale.register(LOCALE_NS, {
				zh,
				en
			}), `${PACKAGE_ID}: locale`);
			const injected = () => ({
				store,
				t,
				setEnabled: (enabled) => {
					persistKnobs({
						...knobs,
						enabled
					});
				},
				setKnob: (key, value) => {
					persistKnobs({
						...knobs,
						[key]: value
					});
				},
				upload,
				save,
				remove
			});
			ctx.effect(() => ctx.slots.inject("settings.general.item", () => ctx.slots.register({
				name: "settings.general.item",
				id: "frosted-window",
				order: 12,
				locale: LOCALE_NS,
				inject: injected
			}, FrostedSection)), `${PACKAGE_ID}: general row`);
			ctx.effect(() => ctx.slots.inject("settings.section", () => ctx.slots.register({
				name: "settings.section",
				id: PACKAGE_ID,
				order: 36,
				label: () => t("nav"),
				locale: LOCALE_NS,
				inject: injected
			}, FrostedSection)), `${PACKAGE_ID}: settings`);
			ctx.effect(() => {
				const boot = mutation;
				const off = ctx.on("theme/change", () => {
					project(false);
				});
				images.get().then((record) => {
					if (disposed || mutation !== boot) return;
					if (record !== void 0) adoptRecord(record, false);
					else project(true);
				}).catch((error) => {
					if (!disposed && mutation === boot) publish({ error: messageFor(error, t) });
				});
				return () => {
					disposed = true;
					mutation += 1;
					off();
					if (typeof disposeTokens === "function") disposeTokens();
					disposeTokens = void 0;
					presenter.dispose();
				};
			}, `${PACKAGE_ID}: surface`);
		}
		function messageFor(error, t) {
			if (error instanceof ImageValidationError && error.message.includes("unsupported")) return t("errorType");
			return t("errorGeneric");
		}
		//#endregion
		exports.apply = apply;
		exports.inject = inject;
		exports.name = name;
		return module.exports;
	}
});

//# sourceMappingURL=client.js.map