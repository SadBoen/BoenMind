import { describe, expect, it } from 'vitest'
import { openImageStore } from '../src/client/image-store.ts'

describe('image store', () => {
  it('puts, reads, and clears a wallpaper record', async () => {
    const store = openImageStore()
    const bytes = [1, 2, 3]
    await store.put({
      bytes,
      mime: 'image/jpeg',
      name: 'wall.jpg',
      width: 10,
      height: 8,
      updatedAt: 1,
    })
    const got = await store.get()
    expect(got?.name).toBe('wall.jpg')
    expect(got?.width).toBe(10)
    expect(got?.mime).toBe('image/jpeg')
    expect(got?.bytes).toEqual([1, 2, 3])
    await store.clear()
    expect(await store.get()).toBeUndefined()
  })
})
