//! Informacje o komputerze: poziom baterii i zajętość dysków (dysk systemowy + woluminy w /Volumes).
//! Korzystamy z narzędzi macOS, które są zawsze dostępne: `pmset` i `df`.

use serde::Serialize;
use std::process::Command;

#[derive(Debug, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Battery {
    pub percent: i64,
    /// np. „charged”, „charging”, „discharging”
    pub state: String,
    pub on_ac_power: bool,
}

#[derive(Debug, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Disk {
    pub name: String,
    pub mount: String,
    /// zajętość w procentach
    pub used_percent: i64,
    /// wolne miejsce, bajty
    pub free_bytes: i64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SystemInfo {
    /// brak = komputer bez baterii (np. Mac mini)
    pub battery: Option<Battery>,
    pub disks: Vec<Disk>,
}

pub fn info() -> Result<SystemInfo, String> {
    let batt = run("pmset", &["-g", "batt"])?;
    let df = run("df", &["-k", "-P"])?;
    Ok(SystemInfo { battery: parse_battery(&batt), disks: parse_disks(&df) })
}

fn run(program: &str, args: &[&str]) -> Result<String, String> {
    let out = Command::new(program).args(args).output().map_err(|e| format!("{program}: {e}"))?;
    if !out.status.success() {
        return Err(format!("{program}: błąd {}", out.status));
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// Wyjście `pmset -g batt`: „Now drawing from 'AC Power'” i linia „-InternalBattery-0 … 100%; charged; …”
pub fn parse_battery(text: &str) -> Option<Battery> {
    let line = text.lines().find(|l| l.contains('%'))?;
    let percent_at = line.find('%')?;
    let digits: String = line[..percent_at].chars().rev().take_while(|c| c.is_ascii_digit()).collect::<Vec<_>>().into_iter().rev().collect();
    let percent: i64 = digits.parse().ok()?;
    let after = &line[percent_at + 1..];
    let state = after.split(';').nth(1).map_or(String::new(), |s| s.trim().to_string());
    let on_ac_power = text.contains("AC Power");
    Some(Battery { percent, state, on_ac_power })
}

/// Wyjście `df -k -P`: dysk systemowy „/” i woluminy w „/Volumes/”. Pozostałe punkty montowania pomijamy.
pub fn parse_disks(text: &str) -> Vec<Disk> {
    let mut disks = Vec::new();
    for line in text.lines().skip(1) {
        let cols: Vec<&str> = line.split_whitespace().collect();
        if cols.len() < 6 {
            continue;
        }
        // punkt montowania może zawierać spacje: wszystko od szóstej kolumny w górę
        let mount = cols[5..].join(" ");
        let is_system = mount == "/";
        let is_external = mount.starts_with("/Volumes/");
        if !is_system && !is_external {
            continue;
        }
        let Ok(available_kb) = cols[3].parse::<i64>() else { continue };
        let Some(used_percent) = cols[4].trim_end_matches('%').parse::<i64>().ok() else { continue };
        let name = if is_system { "Dysk systemowy".to_string() } else { mount.rsplit('/').next().unwrap_or(&mount).to_string() };
        disks.push(Disk { name, mount, used_percent, free_bytes: available_kb * 1024 });
    }
    disks
}

#[cfg(test)]
mod tests {
    use super::*;

    const BATT: &str = "Now drawing from 'AC Power'\n -InternalBattery-0 (id=22216803)\t100%; charged; 0:00 remaining present: true\n";
    const BATT_DISCHARGING: &str = "Now drawing from 'Battery Power'\n -InternalBattery-0 (id=1)\t64%; discharging; 3:12 remaining present: true\n";
    const DF: &str = "Filesystem 1024-blocks Used Available Capacity Mounted on
/dev/disk3s1s1 482797652 12342352 211956056 6% /
devfs 205 205 0 100% /dev
/dev/disk3s5 482797652 244050788 211956056 54% /System/Volumes/Data
/dev/disk4s1 976728320 82825472 893902848 9% /Volumes/DyskTestowy
/dev/disk5s1 1000 500 500 50% /Volumes/Moje Dane
";

    #[test]
    fn parses_charged_battery_on_ac() {
        assert_eq!(parse_battery(BATT), Some(Battery { percent: 100, state: "charged".into(), on_ac_power: true }));
    }

    #[test]
    fn parses_discharging_battery() {
        assert_eq!(parse_battery(BATT_DISCHARGING), Some(Battery { percent: 64, state: "discharging".into(), on_ac_power: false }));
    }

    #[test]
    fn no_battery_gives_none() {
        assert_eq!(parse_battery("Now drawing from 'AC Power'\n"), None);
    }

    #[test]
    fn keeps_system_and_external_volumes_only() {
        let disks = parse_disks(DF);
        let names: Vec<&str> = disks.iter().map(|d| d.name.as_str()).collect();
        assert_eq!(names, vec!["Dysk systemowy", "DyskTestowy", "Moje Dane"]);
        assert_eq!(disks[0].used_percent, 6);
        assert_eq!(disks[1].free_bytes, 893902848 * 1024);
    }
}
