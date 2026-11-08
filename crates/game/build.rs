//! Commit et date de build, exposés à `rst version` (RST_COMMIT, RST_BUILD_DATE).

use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

fn main() {
    let commit = std::env::var("GITHUB_SHA")
        .ok()
        .or_else(|| {
            let o = Command::new("git")
                .args(["rev-parse", "HEAD"])
                .output()
                .ok()?;
            o.status
                .success()
                .then(|| String::from_utf8_lossy(&o.stdout).trim().to_string())
        })
        .map_or_else(|| "inconnu".to_string(), |s| s.chars().take(9).collect());
    println!("cargo:rustc-env=RST_COMMIT={commit}");
    println!("cargo:rustc-env=RST_BUILD_DATE={}", today());
    println!("cargo:rerun-if-env-changed=GITHUB_SHA");
    println!("cargo:rerun-if-changed=../../.git/HEAD");
}

/// Date UTC `AAAA-MM-JJ` (jours depuis 1970 → calendrier grégorien).
fn today() -> String {
    let days = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() / 86_400) as i64;
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}-{m:02}-{d:02}")
}
