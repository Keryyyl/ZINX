import { describe, expect, it } from 'vitest'
import type { Item } from './types'
import { layoutDay } from './timeline'

const base = { description: '', date: '2026-11-07', recurrence: 'none' as const }
const event = (id: number, startTime: string, endTime: string): Item => ({ ...base, id, kind: 'event', title: `e${id}`, startTime, endTime })
const task = (id: number, startTime: string | null): Item => ({ ...base, id, kind: 'task', title: `t${id}`, startTime, endTime: null })

describe('layoutDay', () => {
  it('wpisy bez godziny idą osobno', () => {
    const r = layoutDay([task(1, null), task(2, '09:00')])
    expect(r.untimed.map((i) => i.id)).toEqual([1])
    expect(r.timed).toHaveLength(1)
    expect(r.timed[0].duration).toBe(30)
  })

  it('wydarzenia, które nie nakładają się, zajmują całą szerokość', () => {
    const r = layoutDay([event(1, '09:00', '10:00'), event(2, '10:00', '11:00')])
    expect(r.timed.map((p) => [p.columns, p.column])).toEqual([
      [1, 0],
      [1, 0]
    ])
  })

  it('nakładające się wydarzenia dzielą szerokość na kolumny', () => {
    const r = layoutDay([event(1, '09:00', '11:00'), event(2, '10:00', '12:00'), event(3, '10:30', '10:45')])
    expect(r.timed.every((p) => p.columns === 3)).toBe(true)
    const cols = Object.fromEntries(r.timed.map((p) => [p.item.id, p.column]))
    expect(cols[1]).toBe(0)
    expect(cols[2]).toBe(1)
    expect(cols[3]).toBe(2)
  })

  it('kolumna jest ponownie używana, gdy wcześniejszy wpis się skończył', () => {
    const r = layoutDay([event(1, '09:00', '10:00'), event(2, '09:30', '10:30'), event(3, '10:15', '11:00')])
    const cols = Object.fromEntries(r.timed.map((p) => [p.item.id, p.column]))
    expect(cols[3]).toBe(0)
    expect(r.timed.every((p) => p.columns === 2)).toBe(true)
  })
})
