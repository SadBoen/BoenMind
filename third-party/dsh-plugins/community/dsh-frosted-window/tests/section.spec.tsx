// @vitest-environment jsdom
import { createRoot } from 'react-dom/client'
import { act } from 'react'
import { describe, expect, it } from 'vitest'
import { FrostedSection } from '../src/client/FrostedSection.tsx'
import { zh } from '../src/client/locales.ts'
import { createFrostedStore, INITIAL_STATE } from '../src/client/store.ts'

describe('FrostedSection', () => {
  it('renders the upload surface and reports slider writes', async () => {
    const store = createFrostedStore({ ...INITIAL_STATE, revision: 0 })
    const host = document.createElement('div')
    document.body.append(host)
    const root = createRoot(host)
    await act(async () => {
      root.render(
        <FrostedSection
          store={store}
          t={key => zh[key]}
          setEnabled={() => {}}
          setKnob={() => {}}
          upload={async () => {}}
          save={async () => {}}
          remove={async () => {}}
        />,
      )
    })
    expect(host.textContent).toContain('磨砂玻璃窗口')
    expect(host.textContent).toContain('启用主题')
    expect(host.textContent).toContain('保存')
    expect(host.textContent).toContain('删除')
    expect(host.querySelectorAll('input[type="range"]')).toHaveLength(4)
    expect(host.querySelector('input[type="file"]')?.getAttribute('accept')).toContain('image/png')
    await act(async () => { root.unmount() })
    host.remove()
  })
})
