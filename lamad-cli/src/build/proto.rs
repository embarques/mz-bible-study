//! Youth slide prototype indices.

pub struct Proto {
    pub title: u32,
    pub lectura: u32,
    pub propositos: u32,
    pub idea: u32,
    pub comentario: u32,
    pub intro_header: u32,
    pub intro: u32,
    pub section: [u32; 3],
    pub texto: u32,
    pub ab: u32,
    pub conclusion: u32,
    pub proximo: u32,
}

pub const PROTO: Proto = Proto {
    title: 1,
    lectura: 2,
    propositos: 3,
    idea: 4,
    comentario: 5,
    intro_header: 6,
    intro: 7,
    section: [8, 12, 16],
    texto: 9,
    ab: 10,
    conclusion: 20,
    proximo: 21,
};
