//! Coque du terrain lue dans les meshes de collision RocketSim (`soccar/mesh_0..15.cmf`) : les mêmes
//! triangles que la physique, pour que la voiture touche exactement la surface dessinée. Les fichiers
//! restent sur le disque du joueur (téléchargés par `scripts/fetch-meshes.sh`) : rien n'est embarqué.
//!
//! Format : `i32 n_tris`, `i32 n_verts`, `n_tris × 3 × i32` indices, `n_verts × 3 × f32` sommets,
//! en petit-boutiste et en unités Bullet (1 unité = 50 uu).

use std::path::Path;

use crate::arena_geo::{HEIGHT, V3};

/// Facteur Bullet → unreal units.
const BT_TO_UU: f32 = 50.0;
/// Angle (cos) en dessous duquel l'arête entre deux triangles voisins est tracée comme un pli.
const CREASE_COS: f32 = 0.9397; // 20°

/// Triangles tournés vers l'intérieur de l'arène (sens de l'enroulement) et plis marqués.
#[derive(Debug, Default)]
pub struct Shell {
    pub tris: Vec<[V3; 3]>,
    pub creases: Vec<(V3, V3)>,
}

fn sub(a: V3, b: V3) -> V3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn cross(a: V3, b: V3) -> V3 {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

fn unit(a: V3) -> Option<V3> {
    let l = (a[0] * a[0] + a[1] * a[1] + a[2] * a[2]).sqrt();
    (l > 1e-6).then(|| [a[0] / l, a[1] / l, a[2] / l])
}

fn dot(a: V3, b: V3) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn rd_i32(b: &[u8], at: usize) -> Option<i32> {
    Some(i32::from_le_bytes(b.get(at..at + 4)?.try_into().ok()?))
}

fn rd_f32(b: &[u8], at: usize) -> Option<f32> {
    Some(f32::from_le_bytes(b.get(at..at + 4)?.try_into().ok()?))
}

/// Ajoute au résultat les triangles et plis d'un fichier `.cmf`. Erreur si le fichier est mal formé.
pub fn parse_cmf(bytes: &[u8], out: &mut Shell) -> Result<(), String> {
    let bad = || "mesh .cmf mal formé".to_string();
    let nt = usize::try_from(rd_i32(bytes, 0).ok_or_else(bad)?).map_err(|_| bad())?;
    let nv = usize::try_from(rd_i32(bytes, 4).ok_or_else(bad)?).map_err(|_| bad())?;
    if bytes.len() != 8 + 12 * nt + 12 * nv {
        return Err(bad());
    }
    let vbase = 8 + 12 * nt;
    let vert = |i: usize| -> Option<V3> {
        (i < nv).then_some(())?;
        let at = vbase + 12 * i;
        Some([
            rd_f32(bytes, at)? * BT_TO_UU,
            rd_f32(bytes, at + 4)? * BT_TO_UU,
            rd_f32(bytes, at + 8)? * BT_TO_UU,
        ])
    };
    // Sommet « au centre » de l'arène : un triangle est gardé s'il lui fait face.
    let center = [0.0, 0.0, HEIGHT * 0.45];
    let mut edges: std::collections::HashMap<(usize, usize), Vec<V3>> = Default::default();
    for t in 0..nt {
        let mut idx = [0usize; 3];
        for (k, slot) in idx.iter_mut().enumerate() {
            *slot = usize::try_from(rd_i32(bytes, 8 + 12 * t + 4 * k).ok_or_else(bad)?)
                .map_err(|_| bad())?;
        }
        let v = [
            vert(idx[0]).ok_or_else(bad)?,
            vert(idx[1]).ok_or_else(bad)?,
            vert(idx[2]).ok_or_else(bad)?,
        ];
        let Some(n) = unit(cross(sub(v[1], v[0]), sub(v[2], v[0]))) else {
            continue;
        };
        let c = [
            (v[0][0] + v[1][0] + v[2][0]) / 3.0,
            (v[0][1] + v[1][1] + v[2][1]) / 3.0,
            (v[0][2] + v[1][2] + v[2][2]) / 3.0,
        ];
        if dot(n, sub(center, c)) <= 0.0 {
            continue; // face tournée vers l'extérieur
        }
        out.tris.push(v);
        for (a, b) in [(0, 1), (1, 2), (2, 0)] {
            let key = (idx[a].min(idx[b]), idx[a].max(idx[b]));
            edges.entry(key).or_default().push(n);
        }
    }
    for ((a, b), normals) in edges {
        if normals.len() == 2 && dot(normals[0], normals[1]) < CREASE_COS {
            if let (Some(pa), Some(pb)) = (vert(a), vert(b)) {
                out.creases.push((pa, pb));
            }
        }
    }
    Ok(())
}

/// Lit les 16 meshes soccar du dossier `dir` (celui qui contient `soccar/`).
pub fn load(dir: &Path) -> Result<Shell, String> {
    let mut out = Shell::default();
    for i in 0..sim_core::SOCCAR_MESH_COUNT {
        let p = dir.join("soccar").join(format!("mesh_{i}.cmf"));
        let bytes = std::fs::read(&p).map_err(|e| format!("{} : {e}", p.display()))?;
        parse_cmf(&bytes, &mut out).map_err(|e| format!("{} : {e}", p.display()))?;
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cmf(tris: &[[usize; 3]], verts: &[V3]) -> Vec<u8> {
        let mut b = Vec::new();
        b.extend((tris.len() as i32).to_le_bytes());
        b.extend((verts.len() as i32).to_le_bytes());
        for t in tris {
            for i in t {
                b.extend((*i as i32).to_le_bytes());
            }
        }
        for v in verts {
            for c in v {
                b.extend((c / BT_TO_UU).to_le_bytes());
            }
        }
        b
    }

    #[test]
    fn keeps_inward_faces_and_scales_to_uu() {
        // Sol z = 0 : normale +z (vers le centre) gardée ; même triangle retourné : écarté.
        let verts = [[0.0, 0.0, 0.0], [1000.0, 0.0, 0.0], [0.0, 1000.0, 0.0]];
        let mut s = Shell::default();
        parse_cmf(&cmf(&[[0, 1, 2], [0, 2, 1]], &verts), &mut s).unwrap();
        assert_eq!(s.tris.len(), 1);
        assert_eq!(s.tris[0][1], [1000.0, 0.0, 0.0]);
    }

    #[test]
    fn marks_creases_between_distinct_faces() {
        // Un sol et un mur à angle droit partageant l'arête 0-1.
        let verts = [
            [-500.0, 500.0, 0.0],
            [500.0, 500.0, 0.0],
            [0.0, -500.0, 0.0],
            [0.0, 500.0, 1000.0],
        ];
        let mut s = Shell::default();
        // sol (normale +z), mur en y = 500, tourné vers -y (vers le centre)
        parse_cmf(&cmf(&[[0, 2, 1], [0, 1, 3]], &verts), &mut s).unwrap();
        assert_eq!(s.tris.len(), 2);
        assert_eq!(s.creases.len(), 1);
    }

    #[test]
    fn rejects_truncated_files() {
        let mut s = Shell::default();
        assert!(parse_cmf(&[1, 0, 0, 0], &mut s).is_err());
        let ok = cmf(
            &[[0, 1, 2]],
            &[[0.0; 3], [50.0, 0.0, 0.0], [0.0, 50.0, 0.0]],
        );
        assert!(parse_cmf(&ok[..ok.len() - 4], &mut s).is_err());
    }

    /// Avec les vrais meshes (si présents) : la coque couvre rampes et coins, tournée vers l'intérieur.
    #[test]
    fn real_meshes_load_when_available() {
        let Ok(dir) = sim_core::meshes_dir_from_env() else {
            return;
        };
        let s = load(&dir).unwrap();
        assert!(s.tris.len() > 5000, "{} triangles", s.tris.len());
        for t in &s.tris {
            for p in t {
                assert!(
                    p[0].abs() <= 4200.0
                        && p[1].abs() <= 6100.0
                        && (-10.0..=2200.0).contains(&p[2])
                );
            }
        }
    }
}
