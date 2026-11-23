//! Clima actual y de las próximas horas desde MET Norway (api.met.no), gratis y sin cuenta.
//! La ubicación sale del servicio de ubicación de Windows (precisión baja: alcanza para el clima).

use crate::http;
use nanoserde::DeJson;

#[derive(Clone, Debug)]
pub struct Weather {
    pub temp: f64,
    /// Código de MET Norway, ej. "partlycloudy_day", "rain".
    pub now: String,
    /// Lo que viene en las próximas 6 horas.
    pub next: String,
    pub min6: f64,
    pub max6: f64,
}

#[derive(DeJson)]
struct Met {
    properties: Props,
}
#[derive(DeJson)]
struct Props {
    timeseries: Vec<Ts>,
}
#[derive(DeJson)]
struct Ts {
    data: TsData,
}
#[derive(DeJson)]
struct TsData {
    instant: InstantData,
    #[nserde(default)]
    next_1_hours: Option<Next>,
    #[nserde(default)]
    next_6_hours: Option<Next>,
}
#[derive(DeJson)]
struct InstantData {
    details: Details,
}
#[derive(DeJson)]
struct Details {
    #[nserde(default)]
    air_temperature: f64,
}
#[derive(DeJson)]
struct Next {
    summary: Summary,
}
#[derive(DeJson)]
struct Summary {
    symbol_code: String,
}

/// Ubicación aproximada desde Windows. None si el usuario no dio permiso.
pub fn locate() -> Option<(f64, f64)> {
    use windows::Devices::Geolocation::{GeolocationAccessStatus, Geolocator, PositionAccuracy};
    let status = Geolocator::RequestAccessAsync().ok()?.get().ok()?;
    if status != GeolocationAccessStatus::Allowed {
        return None;
    }
    let g = Geolocator::new().ok()?;
    let _ = g.SetDesiredAccuracy(PositionAccuracy::Default);
    let pos = g.GetGeopositionAsync().ok()?.get().ok()?;
    let p = pos.Coordinate().ok()?.Point().ok()?.Position().ok()?;
    Some((p.Latitude, p.Longitude))
}

/// Pide el pronóstico (bloqueante: llamar desde un hilo de fondo).
pub fn fetch(lat: f64, lon: f64) -> Option<Weather> {
    // MET Norway pide coordenadas con 4 decimales como máximo.
    let path = format!("/weatherapi/locationforecast/2.0/compact?lat={lat:.4}&lon={lon:.4}");
    let r = http::get("api.met.no", &path)?;
    if r.status != 200 {
        return None;
    }
    let met = Met::deserialize_json(&r.body).ok()?;
    let ts = &met.properties.timeseries;
    let first = ts.first()?;
    let temps: Vec<f64> = ts.iter().take(7).map(|t| t.data.instant.details.air_temperature).collect();
    let sym = |n: &Option<Next>| n.as_ref().map(|n| n.summary.symbol_code.clone()).unwrap_or_default();
    Some(Weather {
        temp: first.data.instant.details.air_temperature,
        now: sym(&first.data.next_1_hours),
        next: sym(&first.data.next_6_hours),
        min6: temps.iter().cloned().fold(f64::MAX, f64::min),
        max6: temps.iter().cloned().fold(f64::MIN, f64::max),
    })
}

/// (símbolo monocromo, descripción en español) para un código de MET Norway.
pub fn describe(code: &str) -> (&'static str, &'static str) {
    let base = code.split('_').next().unwrap_or("");
    match base {
        "clearsky" => ("☀", "despejado"),
        "fair" => ("🌤", "poco nuboso"),
        "partlycloudy" => ("⛅", "parcialmente nublado"),
        "cloudy" => ("☁", "nublado"),
        "fog" => ("🌫", "niebla"),
        b if b.contains("thunder") => ("⚡", "tormenta"),
        b if b.contains("snow") => ("❄", "nieve"),
        b if b.contains("sleet") => ("❄", "aguanieve"),
        b if b.contains("rain") && b.starts_with("light") => ("☂", "llovizna"),
        b if b.contains("rain") && b.starts_with("heavy") => ("☂", "lluvia fuerte"),
        b if b.contains("rain") => ("☂", "lluvia"),
        _ => ("☁", ""),
    }
}
