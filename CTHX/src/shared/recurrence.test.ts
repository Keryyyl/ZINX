import { describe, expect, it } from 'vitest'
import { addDays, monthGrid, occursOn } from './recurrence'

const item = (date: string, recurrence: 'none' | 'daily' | 'weekly' | 'monthly' | 'yearly') => ({ date, recurrence })

describe('occursOn', () => {
  it('jednorazowe tylko w dniu startu', () => {
    expect(occursOn(item('2026-11-07', 'none'), '2026-11-07')).toBe(true)
    expect(occursOn(item('2026-11-07', 'none'), '2026-11-08')).toBe(false)
  })
  it('codziennie od daty startu, nie wcześniej', () => {
    expect(occursOn(item('2026-11-07', 'daily'), '2026-11-06')).toBe(false)
    expect(occursOn(item('2026-11-07', 'daily'), '2026-12-31')).toBe(true)
  })
  it('co tydzień w ten sam dzień tygodnia', () => {
    expect(occursOn(item('2026-11-07', 'weekly'), '2026-11-14')).toBe(true)
    expect(occursOn(item('2026-11-07', 'weekly'), '2026-11-08')).toBe(false)
  })
  it('co rok w tę samą datę, nie wcześniej niż data startu', () => {
    expect(occursOn(item('1990-03-14', 'yearly'), '2026-03-14')).toBe(true)
    expect(occursOn(item('1990-03-14', 'yearly'), '2026-03-15')).toBe(false)
    expect(occursOn(item('2030-03-14', 'yearly'), '2026-03-14')).toBe(false)
  })
  it('co miesiąc w ten sam dzień miesiąca', () => {
    expect(occursOn(item('2026-11-07', 'monthly'), '2026-12-07')).toBe(true)
    expect(occursOn(item('2026-11-07', 'monthly'), '2026-12-08')).toBe(false)
  })
})

describe('kalendarz', () => {
  it('dodawanie dni przechodzi przez granice miesięcy i lat', () => {
    expect(addDays('2026-12-31', 1)).toBe('2027-01-01')
    expect(addDays('2026-03-01', -1)).toBe('2026-02-28')
  })
  it('siatka miesiąca zaczyna się w poniedziałek i ma 42 dni', () => {
    const grid = monthGrid(2026, 10) // listopad 2026, 1 listopada to niedziela
    expect(grid).toHaveLength(42)
    expect(grid[0]).toBe('2026-10-26')
    expect(grid).toContain('2026-11-01')
  })
})
