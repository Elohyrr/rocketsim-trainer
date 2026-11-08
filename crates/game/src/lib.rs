//! Client jouable (M1) : monde de jeu à 120 Hz fixe (`world`), entrées (`input`, `config`),
//! géométrie du terrain en primitives originales (`arena_geo`). Le rendu macroquad est dans le binaire.

pub mod arena_geo;
pub mod config;
pub mod input;
pub mod meshes;
pub mod pad;
pub mod rig;
pub mod shell;
pub mod world;
