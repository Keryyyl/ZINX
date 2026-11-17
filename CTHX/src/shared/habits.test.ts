import { describe, expect, it } from 'vitest'
import { dayState, habitStats, monthDays } from './habits'

const habit = { id: 1, name: 'Bieg', description: '', goal: 5, startDate: '2026-11-01' }

describe('nawyki', () => {
  it('miesiąc ma tyle dni, ile wynosi kalendarz', () => {
    expect(monthDays(2026, 10)).toHaveLength(30) // listopad
    expect(monthDays(2028, 1)).toHaveLength(29) // luty przestępny
    expect(monthDays(2026, 10)[0]).toBe('2026-11-01')
  })

  it('dzień zrobiony, pominięty, otwarty i sprzed startu', () => {
    const done = new Set(['2026-11-02'])
    expect(dayState('2026-11-02', done, habit, '2026-11-08')).toBe('done')
    expect(dayState('2026-11-03', done, habit, '2026-11-08')).toBe('missed')
    expect(dayState('2026-11-08', done, habit, '2026-11-08')).toBe('pending')
    expect(dayState('2026-11-09', done, habit, '2026-11-08')).toBe('pending')
    expect(dayState('2026-10-31', done, { startDate: '2026-11-05' }, '2026-11-08')).toBe('untracked')
  })

  it('statystyki: zrobione, pominięte, cel i tempo', () => {
    const done = new Set(['2026-11-01', '2026-11-03', '2026-11-05'])
    const s = habitStats(habit, done, '2026-11-08', 2026, 10)
    expect(s.done).toBe(3)
    expect(s.missed).toBe(4) // 2, 4, 6, 7 listopada
    expect(s.open).toBe(30 - 3 - 4)
    expect(s.goalLeft).toBe(2)
    expect(s.tracked).toBe(7)
    expect(s.rate).toBe(43)
  })

  it('dni przed startem nie liczą się jako pominięte', () => {
    const s = habitStats({ ...habit, startDate: '2026-11-06' }, new Set(), '2026-11-08', 2026, 10)
    expect(s.missed).toBe(2) // 6 i 7 listopada
    expect(s.tracked).toBe(2)
  })

  it('cel osiągnięty daje zero brakujących dni', () => {
    const done = new Set(['2026-11-01', '2026-11-02', '2026-11-03', '2026-11-04', '2026-11-05', '2026-11-06'])
    expect(habitStats(habit, done, '2026-11-08', 2026, 10).goalLeft).toBe(0)
  })
})
