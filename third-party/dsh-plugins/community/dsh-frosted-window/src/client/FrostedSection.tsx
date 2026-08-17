import { useRef, useState, useSyncExternalStore, type ChangeEvent, type CSSProperties, type DragEvent } from 'react'
import type { FrostedKey } from './locales.ts'
import type { FrostedKnobs } from './knobs.ts'
import type { FrostedStore } from './store.ts'

export interface FrostedSectionInjected {
  store: FrostedStore
  t: (key: FrostedKey) => string
  setEnabled: (enabled: boolean) => void
  setKnob: <K extends keyof Omit<FrostedKnobs, 'enabled'>>(key: K, value: number) => void
  upload: (file: File) => Promise<void>
  save: () => Promise<void>
  remove: () => Promise<void>
}

/**
 * Settings page: preview, sliders, explicit save / delete.
 * @param props - inject face from apply().
 */
export function FrostedSection({
  store, t, setEnabled, setKnob, upload, save, remove,
}: FrostedSectionInjected) {
  const state = useSyncExternalStore(store.subscribe, store.get, store.get)
  const inputRef = useRef<HTMLInputElement>(null)
  const [over, setOver] = useState(false)

  const onFiles = (files: FileList | null): void => {
    if (state.busy) return
    const file = files?.[0]
    if (file === undefined) return
    void upload(file)
  }

  const pick = (): void => {
    if (!state.busy) inputRef.current?.click()
  }

  const previewStyle = {
    '--fw-ui-glass': String(state.glassOpacity),
    '--fw-ui-blur': `${state.blurPx}px`,
    '--fw-ui-sat': `${Math.round(state.saturate * 100)}%`,
  } as CSSProperties

  const meta = [
    state.fileName,
    state.width > 0 && state.height > 0 ? `${state.width}×${state.height}` : null,
  ].filter(Boolean).join(' · ')

  return (
    <div className="fw-section">
      <div className="fw-panel">
        <div className="fw-head">
          <div className="fw-lead">
            <div className="fw-kicker">Theme</div>
            <div className="fw-title">{t('title')}</div>
            <div className="fw-desc">{t('description')}</div>
          </div>
          <span className="fw-chip" data-tone={state.dirty ? 'warn' : undefined}>
            {state.dirty ? t('unsaved') : t('saved')}
          </span>
        </div>

        <label className="fw-switch">
          <span>{t('enable')}</span>
          <input
            type="checkbox"
            checked={state.enabled}
            onChange={(event: ChangeEvent<HTMLInputElement>) => { setEnabled(event.target.checked) }}
          />
        </label>

        <button
          type="button"
          className="fw-hero"
          style={previewStyle}
          data-over={over ? 'true' : 'false'}
          data-has={state.hasImage ? 'true' : 'false'}
          disabled={state.busy}
          onClick={pick}
          onDragOver={(event) => { event.preventDefault(); setOver(true) }}
          onDragLeave={() => { setOver(false) }}
          onDrop={(event: DragEvent<HTMLButtonElement>) => {
            event.preventDefault()
            setOver(false)
            onFiles(event.dataTransfer.files)
          }}
        >
          {state.previewUrl !== null ? <img src={state.previewUrl} alt="" /> : null}
          {state.hasImage ? <span className="fw-hero-glass" /> : null}
          <span className="fw-hero-copy">
            <strong>{state.busy ? t('busy') : state.hasImage ? t('dropReplace') : t('drop')}</strong>
            <span>{state.hasImage ? (meta || t('dropReplace')) : t('empty')}</span>
          </span>
        </button>
        <input
          ref={inputRef}
          className="fw-hidden"
          type="file"
          accept="image/jpeg,image/jpg,image/png,image/webp,image/gif,image/*"
          onChange={(event) => {
            onFiles(event.target.files)
            event.target.value = ''
          }}
        />

        {state.error !== null ? <div className="fw-error" role="alert">{state.error}</div> : null}

        <div className="fw-grid">
          <Slider label={t('glass')} value={state.glassOpacity} min={0.18} max={0.82} step={0.01}
            display={`${Math.round(state.glassOpacity * 100)}%`}
            onChange={value => { setKnob('glassOpacity', value) }} />
          <Slider label={t('blur')} value={state.blurPx} min={8} max={64} step={1}
            display={`${Math.round(state.blurPx)}px`}
            onChange={value => { setKnob('blurPx', value) }} />
          <Slider label={t('saturate')} value={state.saturate} min={1} max={2} step={0.01}
            display={`${Math.round(state.saturate * 100)}%`}
            onChange={value => { setKnob('saturate', value) }} />
          <Slider label={t('dim')} value={state.dim} min={0} max={0.65} step={0.01}
            display={`${Math.round(state.dim * 100)}%`}
            onChange={value => { setKnob('dim', value) }} />
        </div>

        <div className="fw-bar">
          <button type="button" className="fw-btn" data-kind="danger" disabled={!state.hasImage || state.busy} onClick={() => { void remove() }}>
            {t('remove')}
          </button>
          <button type="button" className="fw-btn" onClick={pick} disabled={state.busy}>
            {t('choose')}
          </button>
          <button type="button" className="fw-btn" data-kind="primary" disabled={!state.dirty || state.busy} onClick={() => { void save() }}>
            {t('save')}
          </button>
        </div>
      </div>
    </div>
  )
}

function Slider(props: {
  label: string
  value: number
  min: number
  max: number
  step: number
  display: string
  onChange: (value: number) => void
}) {
  return (
    <label className="fw-row">
      <span className="fw-row-head">
        <span>{props.label}</span>
        <span>{props.display}</span>
      </span>
      <input
        type="range"
        min={props.min}
        max={props.max}
        step={props.step}
        value={props.value}
        onChange={(event) => { props.onChange(Number(event.target.value)) }}
      />
    </label>
  )
}
