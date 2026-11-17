//! Pogoda z Open-Meteo (bez klucza API), tak jak w EINPX.

use serde::Serialize;

const PLACE: &str = "Warszawa";
const LATITUDE: &str = "52.2297";
const LONGITUDE: &str = "21.0122";

#[derive(Debug, Serialize)]
pub struct Weather {
    pub miejsce: String,
    pub teraz_stopni: i64,
    pub min_stopni: i64,
    pub max_stopni: i64,
    pub opis: String,
}

/// Kody pogody WMO na opis po polsku (jak w EINPX)
pub fn describe(code: i64) -> &'static str {
    match code {
        0 => "Bezchmurnie",
        1..=2 => "Częściowo pochmurno",
        3 => "Pochmurno",
        45 | 48 => "Mgła",
        51..=57 => "Mżawka",
        61..=67 => "Deszcz",
        71..=77 => "Śnieg",
        80..=82 => "Przelotne opady",
        85..=86 => "Przelotny śnieg",
        95.. => "Burza",
        _ => "Brak danych",
    }
}

pub fn fetch() -> Result<Weather, String> {
    let json: serde_json::Value = ureq::get("https://api.open-meteo.com/v1/forecast")
        .query("latitude", LATITUDE)
        .query("longitude", LONGITUDE)
        .query("current", "temperature_2m,weather_code")
        .query("daily", "temperature_2m_max,temperature_2m_min")
        .query("forecast_days", "1")
        .query("timezone", "auto")
        .timeout(std::time::Duration::from_secs(8))
        .call()
        .map_err(|e| format!("Pogoda: {e}"))?
        .into_json()
        .map_err(|e| format!("Pogoda: {e}"))?;

    let num = |v: &serde_json::Value| v.as_f64().map(|x| x.round() as i64);
    let teraz = num(&json["current"]["temperature_2m"]).ok_or("Pogoda: brak temperatury")?;
    let code = json["current"]["weather_code"].as_i64().unwrap_or(-1);
    Ok(Weather {
        miejsce: PLACE.into(),
        teraz_stopni: teraz,
        min_stopni: num(&json["daily"]["temperature_2m_min"][0]).unwrap_or(teraz),
        max_stopni: num(&json["daily"]["temperature_2m_max"][0]).unwrap_or(teraz),
        opis: describe(code).into(),
    })
}
