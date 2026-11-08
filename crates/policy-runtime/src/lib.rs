//! Inférence légère d'une politique exportée par le learner : MLP `tanh` lue depuis un fichier
//! [safetensors](https://github.com/huggingface/safetensors) (format écrit et lu ici à la main : 8 octets de
//! longueur d'en-tête, JSON, puis tenseurs `f32` little-endian). Aucune dépendance à burn.
//!
//! Convention : une couche `<nom>.weight` de forme `[entrées, sorties]` et `<nom>.bias` `[sorties]`,
//! `y = x·W + b`. La politique est la suite `policy.0`, `policy.1`, … ; `tanh` entre les couches, pas sur la
//! dernière (logits).

use std::collections::BTreeMap;
use std::path::Path;

#[derive(Debug, thiserror::Error)]
pub enum PolicyError {
    #[error("entrée/sortie : {0}")]
    Io(#[from] std::io::Error),
    #[error("fichier safetensors invalide : {0}")]
    Format(String),
}

/// Tenseur `f32` nommé (forme + données row-major).
#[derive(Clone, Debug, PartialEq)]
pub struct Tensor {
    pub shape: Vec<usize>,
    pub data: Vec<f32>,
}

/// Écrit un fichier safetensors (tenseurs `F32`, triés par nom).
pub fn write_safetensors(
    path: &Path,
    tensors: &BTreeMap<String, Tensor>,
) -> Result<(), PolicyError> {
    let mut header = serde_json::Map::new();
    let mut offset = 0usize;
    for (name, t) in tensors {
        let n = t.data.len() * 4;
        header.insert(
            name.clone(),
            serde_json::json!({"dtype": "F32", "shape": t.shape, "data_offsets": [offset, offset + n]}),
        );
        offset += n;
    }
    let mut h = serde_json::to_vec(&header).map_err(|e| PolicyError::Format(e.to_string()))?;
    while h.len() % 8 != 0 {
        h.push(b' ');
    }
    let mut out = Vec::with_capacity(8 + h.len() + offset);
    out.extend_from_slice(&(h.len() as u64).to_le_bytes());
    out.extend_from_slice(&h);
    for t in tensors.values() {
        for x in &t.data {
            out.extend_from_slice(&x.to_le_bytes());
        }
    }
    std::fs::write(path, out)?;
    Ok(())
}

/// Lit un fichier safetensors `F32`.
pub fn read_safetensors(path: &Path) -> Result<BTreeMap<String, Tensor>, PolicyError> {
    let b = std::fs::read(path)?;
    let bad = |m: &str| PolicyError::Format(m.to_string());
    let len = u64::from_le_bytes(
        b.get(..8)
            .ok_or_else(|| bad("trop court"))?
            .try_into()
            .unwrap(),
    ) as usize;
    let hdr = b.get(8..8 + len).ok_or_else(|| bad("en-tête tronqué"))?;
    let body = &b[8 + len..];
    let json: serde_json::Value =
        serde_json::from_slice(hdr).map_err(|e| PolicyError::Format(e.to_string()))?;
    let mut out = BTreeMap::new();
    for (name, v) in json.as_object().ok_or_else(|| bad("en-tête non objet"))? {
        if name == "__metadata__" {
            continue;
        }
        if v["dtype"] != "F32" {
            return Err(bad("seul F32 est géré"));
        }
        let shape: Vec<usize> = v["shape"]
            .as_array()
            .ok_or_else(|| bad("forme"))?
            .iter()
            .map(|x| x.as_u64().unwrap_or(0) as usize)
            .collect();
        let o = v["data_offsets"].as_array().ok_or_else(|| bad("offsets"))?;
        let (s, e) = (
            o[0].as_u64().unwrap_or(0) as usize,
            o[1].as_u64().unwrap_or(0) as usize,
        );
        let raw = body.get(s..e).ok_or_else(|| bad("données tronquées"))?;
        let data: Vec<f32> = raw
            .chunks_exact(4)
            .map(|c| f32::from_le_bytes(c.try_into().unwrap()))
            .collect();
        if data.len() != shape.iter().product::<usize>() {
            return Err(bad("taille incohérente avec la forme"));
        }
        out.insert(name.clone(), Tensor { shape, data });
    }
    Ok(out)
}

struct Layer {
    n_in: usize,
    n_out: usize,
    w: Vec<f32>,
    b: Vec<f32>,
}

/// MLP de politique prête à l'emploi.
pub struct Policy {
    layers: Vec<Layer>,
}

impl Policy {
    pub fn from_tensors(t: &BTreeMap<String, Tensor>) -> Result<Self, PolicyError> {
        let mut layers = Vec::new();
        for i in 0.. {
            let (Some(w), Some(b)) = (
                t.get(&format!("policy.{i}.weight")),
                t.get(&format!("policy.{i}.bias")),
            ) else {
                break;
            };
            if w.shape.len() != 2 || b.shape != [w.shape[1]] {
                return Err(PolicyError::Format(format!(
                    "couche policy.{i} : formes incohérentes"
                )));
            }
            layers.push(Layer {
                n_in: w.shape[0],
                n_out: w.shape[1],
                w: w.data.clone(),
                b: b.data.clone(),
            });
        }
        if layers.is_empty() {
            return Err(PolicyError::Format("aucune couche policy.*".into()));
        }
        Ok(Self { layers })
    }

    pub fn load(path: &Path) -> Result<Self, PolicyError> {
        Self::from_tensors(&read_safetensors(path)?)
    }

    pub fn obs_dim(&self) -> usize {
        self.layers[0].n_in
    }
    pub fn action_count(&self) -> usize {
        self.layers.last().map_or(0, |l| l.n_out)
    }

    /// Logits pour une observation.
    pub fn logits(&self, obs: &[f32]) -> Vec<f32> {
        assert_eq!(obs.len(), self.obs_dim(), "taille d'observation");
        let mut x = obs.to_vec();
        for (k, l) in self.layers.iter().enumerate() {
            let mut y = l.b.clone();
            for (i, xi) in x.iter().enumerate() {
                let row = &l.w[i * l.n_out..(i + 1) * l.n_out];
                for (yo, w) in y.iter_mut().zip(row) {
                    *yo += xi * w;
                }
            }
            if k + 1 < self.layers.len() {
                y.iter_mut().for_each(|v| *v = v.tanh());
            }
            x = y;
        }
        x
    }

    /// Action gloutonne (indice de la table d'actions).
    pub fn act(&self, obs: &[f32]) -> usize {
        self.logits(obs)
            .iter()
            .enumerate()
            .max_by(|a, b| a.1.total_cmp(b.1))
            .map_or(0, |(i, _)| i)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tiny() -> BTreeMap<String, Tensor> {
        let mut m = BTreeMap::new();
        m.insert(
            "policy.0.weight".into(),
            Tensor {
                shape: vec![2, 2],
                data: vec![1., 0., 0., 1.],
            },
        );
        m.insert(
            "policy.0.bias".into(),
            Tensor {
                shape: vec![2],
                data: vec![0., 0.],
            },
        );
        m.insert(
            "policy.1.weight".into(),
            Tensor {
                shape: vec![2, 3],
                data: vec![1., 0., -1., 0., 2., 0.],
            },
        );
        m.insert(
            "policy.1.bias".into(),
            Tensor {
                shape: vec![3],
                data: vec![0., 0., 0.5],
            },
        );
        m
    }

    #[test]
    fn roundtrip_and_inference() {
        let p = std::env::temp_dir().join(format!("pr-{}.safetensors", std::process::id()));
        write_safetensors(&p, &tiny()).unwrap();
        let back = read_safetensors(&p).unwrap();
        std::fs::remove_file(&p).ok();
        assert_eq!(back, tiny());
        let pol = Policy::from_tensors(&back).unwrap();
        assert_eq!((pol.obs_dim(), pol.action_count()), (2, 3));
        let l = pol.logits(&[0.0, 1.0]);
        let t = 1.0f32.tanh();
        assert!((l[1] - 2.0 * t).abs() < 1e-6 && (l[2] - 0.5).abs() < 1e-6);
        assert_eq!(pol.act(&[0.0, 1.0]), 1);
    }

    #[test]
    fn rejects_garbage() {
        let p = std::env::temp_dir().join(format!("pr-bad-{}.safetensors", std::process::id()));
        std::fs::write(&p, b"abc").unwrap();
        assert!(read_safetensors(&p).is_err());
        std::fs::remove_file(&p).ok();
    }
}
