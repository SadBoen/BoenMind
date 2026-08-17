/**
 * #60 durable state: state.json grows from the theme-only `disabledSkins`
 * into the generic `disabled` list plus custom groups. These specs exercise
 * the REAL hot.ts state functions and the pure groups.ts CRUD — the route
 * wiring and live toggles live in flows.spec.ts.
 */

import { describe, expect, it } from 'vitest'
import { mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import {
  readDisabled, readDisabledThemes, readMarketState, writeDisabled, writeDisabledThemes, writeMarketState,
} from '../src/hot.ts'
import {
  createGroup, deleteGroup, removeFromGroups, renameGroup, setGroupMembers,
} from '../src/groups.ts'

function stateDir(): string {
  const dir = mkdtempSync(join(tmpdir(), 'dshm-state-'))
  mkdirSync(join(dir, '.dsh-market'), { recursive: true })
  return dir
}

function readRaw(dir: string): Record<string, unknown> {
  return JSON.parse(readFileSync(join(dir, '.dsh-market', 'state.json'), 'utf8')) as Record<string, unknown>
}

describe('market state.json (#60)', () => {
  it('loads legacy disabledSkins; new writes use the unified disabled key', () => {
    const dir = stateDir()
    try {
      writeFileSync(join(dir, '.dsh-market', 'state.json'), JSON.stringify({ disabledSkins: ['theme-a'] }))
      expect([...readMarketState(dir).disabled]).toEqual(['theme-a'])
      expect([...readDisabled(dir)]).toEqual(['theme-a'])
      expect([...readDisabledThemes(dir)]).toEqual(['theme-a'])

      writeDisabledThemes(dir, new Set(['theme-b']))
      const raw = readRaw(dir)
      expect(raw.disabled).toEqual(['theme-b'])
      expect(raw.disabledSkins).toBeUndefined()
      expect([...readMarketState(dir).disabled]).toEqual(['theme-b'])
    } finally {
      rmSync(dir, { recursive: true, force: true })
    }
  })

  it('writeDisabled preserves groups and groupOrder; writeMarketState persists all', () => {
    const dir = stateDir()
    try {
      writeMarketState(dir, {
        disabled: new Set(['dsh-loop']),
        groups: { work: ['dsh-loop', 'dsh-notify'] },
        groupOrder: ['work'],
      })
      // A theme switch only rewrites the disable list — groups must survive.
      writeDisabled(dir, new Set(['theme-a']))
      const state = readMarketState(dir)
      expect([...state.disabled]).toEqual(['theme-a'])
      expect(state.groups).toEqual({ work: ['dsh-loop', 'dsh-notify'] })
      expect(state.groupOrder).toEqual(['work'])
    } finally {
      rmSync(dir, { recursive: true, force: true })
    }
  })

  it('readMarketState normalizes malformed payloads to empty state', () => {
    const dir = stateDir()
    try {
      writeFileSync(join(dir, '.dsh-market', 'state.json'), 'not json')
      expect(readMarketState(dir)).toEqual({ disabled: new Set(), groups: {}, groupOrder: [] })
      writeFileSync(join(dir, '.dsh-market', 'state.json'), JSON.stringify({
        disabled: ['a', 'a', '', 7],
        groups: { work: ['x', 'x', 3] },
        groupOrder: ['work', 'work', null],
      }))
      const state = readMarketState(dir)
      expect([...state.disabled]).toEqual(['a'])
      expect(state.groups).toEqual({ work: ['x'] })
      expect(state.groupOrder).toEqual(['work'])
    } finally {
      rmSync(dir, { recursive: true, force: true })
    }
  })
})

describe('group CRUD (groups.ts)', () => {
  it('create/rename/delete keep groups and order consistent', () => {
    const state = { groups: {}, groupOrder: [] }
    expect(createGroup(state, 'work').ok).toBe(true)
    expect(createGroup(state, 'work').ok).toBe(false)
    expect(createGroup(state, 'bad/name').ok).toBe(false)
    expect(createGroup(state, '').ok).toBe(false)

    expect(renameGroup(state, 'work', 'daily').ok).toBe(true)
    expect(state.groups).toEqual({ daily: [] })
    expect(state.groupOrder).toEqual(['daily'])
    expect(renameGroup(state, 'missing', 'x').ok).toBe(false)
    expect(renameGroup(state, 'daily', 'work').ok).toBe(true)
    expect(renameGroup(state, 'work', 'work').ok).toBe(true)

    expect(deleteGroup(state, 'work').ok).toBe(true)
    expect(state).toEqual({ groups: {}, groupOrder: [] })
    expect(deleteGroup(state, 'ghost').ok).toBe(false)
  })

  it('set-members keeps only installed unique names and drops the market itself', () => {
    const state = { groups: { work: [] }, groupOrder: ['work'] }
    const installed = new Set(['dsh-loop', 'dsh-notify', 'dshmarket'])
    const themes = new Set(['theme-a'])
    expect(setGroupMembers(state, 'work', ['dsh-loop', 'dsh-loop', 'ghost', 'dshmarket'], installed, themes).ok).toBe(true)
    expect(state.groups.work).toEqual(['dsh-loop'])
    expect(setGroupMembers(state, 'ghost', [], installed, themes).ok).toBe(false)
    expect(setGroupMembers(state, 'work', 'nope', installed, themes).ok).toBe(false)
  })

  it('set-members rejects a second theme in one group', () => {
    const state = { groups: { work: [] }, groupOrder: ['work'] }
    const installed = new Set(['theme-a', 'theme-b'])
    const themes = new Set(['theme-a', 'theme-b'])
    const result = setGroupMembers(state, 'work', ['theme-a', 'theme-b'], installed, themes)
    expect(result.ok).toBe(false)
    expect(result.error).toMatch(/at most one theme/)
    expect(state.groups.work).toEqual([])
    // A single theme is fine.
    expect(setGroupMembers(state, 'work', ['theme-a'], installed, themes).ok).toBe(true)
    expect(state.groups.work).toEqual(['theme-a'])
  })

  it('removeFromGroups prunes a name everywhere', () => {
    const state = { groups: { a: ['x', 'y'], b: ['x'] }, groupOrder: ['a', 'b'] }
    removeFromGroups(state, 'x')
    expect(state.groups).toEqual({ a: ['y'], b: [] })
  })
})
