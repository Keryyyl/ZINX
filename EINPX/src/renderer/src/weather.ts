import { WEATHER_PLACE } from './config'

export interface Weather {
  temperature: number
  /** najniższa i najwyższa przewidywana temperatura dnia */
  tempMin: number
  tempMax: number
  description: string
  place: string
}

/** Kody pogody WMO (Open-Meteo) na opis po polsku */
export function describe(code: number): string {
  if (code === 0) return 'Bezchmurnie'
  if (code <= 2) return 'Częściowo pochmurno'
  if (code === 3) return 'Pochmurno'
  if (code === 45 || code === 48) return 'Mgła'
  if (code >= 51 && code <= 57) return 'Mżawka'
  if (code >= 61 && code <= 67) return 'Deszcz'
  if (code >= 71 && code <= 77) return 'Śnieg'
  if (code >= 80 && code <= 82) return 'Przelotne opady'
  if (code >= 85 && code <= 86) return 'Przelotny śnieg'
  if (code >= 95) return 'Burza'
  return 'Brak danych'
}

/** Aktualna pogoda z Open-Meteo (bez klucza API). Rzuca błędem, gdy się nie uda. */
export async function fetchWeather(): Promise<Weather> {
  const url = new URL('https://api.open-meteo.com/v1/forecast')
  url.searchParams.set('latitude', String(WEATHER_PLACE.latitude))
  url.searchParams.set('longitude', String(WEATHER_PLACE.longitude))
  url.searchParams.set('current', 'temperature_2m,weather_code')
  url.searchParams.set('daily', 'temperature_2m_max,temperature_2m_min')
  url.searchParams.set('forecast_days', '1')
  url.searchParams.set('timezone', 'auto')
  const res = await fetch(url)
  if (!res.ok) throw new Error(`Pogoda: błąd ${res.status}`)
  const data = (await res.json()) as {
    current: { temperature_2m: number; weather_code: number }
    daily: { temperature_2m_max: number[]; temperature_2m_min: number[] }
  }
  return {
    temperature: Math.round(data.current.temperature_2m),
    tempMin: Math.round(data.daily.temperature_2m_min[0]),
    tempMax: Math.round(data.daily.temperature_2m_max[0]),
    description: describe(data.current.weather_code),
    place: WEATHER_PLACE.name
  }
}
