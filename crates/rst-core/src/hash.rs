//! SHA256 : calcul, lecture d'un fichier `SHA256SUMS`, vérification.

use std::io::Read;
use std::path::Path;

use sha2::{Digest, Sha256};

pub fn sha256_hex(data: &[u8]) -> String {
    hex(&Sha256::digest(data))
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

pub fn sha256_file(path: &Path) -> std::io::Result<String> {
    let mut f = std::fs::File::open(path)?;
    let mut h = Sha256::new();
    let mut buf = [0u8; 64 * 1024];
    loop {
        let n = f.read(&mut buf)?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
    }
    Ok(hex(&h.finalize()))
}

/// Empreinte attendue pour `name` dans un contenu au format `sha256sum` (`<hex>  <nom>` ou `<hex> *<nom>`).
pub fn find_sum(sums: &str, name: &str) -> Option<String> {
    sums.lines().find_map(|l| {
        let (h, rest) = l.trim().split_once(char::is_whitespace)?;
        let file = rest.trim_start().trim_start_matches('*');
        (file == name && h.len() == 64 && h.bytes().all(|c| c.is_ascii_hexdigit()))
            .then(|| h.to_ascii_lowercase())
    })
}

/// Vérifie que `path` a l'empreinte `expected`.
pub fn verify_file(path: &Path, expected: &str) -> Result<(), String> {
    let got = sha256_file(path).map_err(|e| format!("lecture de {} : {e}", path.display()))?;
    if got.eq_ignore_ascii_case(expected) {
        Ok(())
    } else {
        Err(format!(
            "SHA256 différent (attendu {expected}, obtenu {got})"
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ABC: &str = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";

    #[test]
    fn known_vector() {
        assert_eq!(sha256_hex(b"abc"), ABC);
    }

    #[test]
    fn sums_lookup() {
        let s = format!("{ABC}  rst-linux-x86_64\n{}  *autre\n", "0".repeat(64));
        assert_eq!(find_sum(&s, "rst-linux-x86_64").as_deref(), Some(ABC));
        assert!(find_sum(&s, "absent").is_none());
        assert_eq!(find_sum(&s, "autre"), Some("0".repeat(64)));
        assert!(find_sum("zz  x", "x").is_none());
    }

    #[test]
    fn verify() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("f");
        std::fs::write(&p, b"abc").unwrap();
        assert!(verify_file(&p, ABC).is_ok());
        assert!(
            verify_file(&p, &"0".repeat(64))
                .unwrap_err()
                .contains("différent")
        );
    }
}
