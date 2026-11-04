//! O que o talk decide sem tocar no mundo.
//!
//! Microfone, GPU, teclado e área de transferência ficam no binário; aqui só
//! entra o que dá para testar com texto: o config, o vocabulário, a linha do
//! histórico e o fio entre o serviço e o trabalhador que transcreve.

pub mod clean;
pub mod config;
pub mod history;
pub mod paths;
pub mod shortcuts;
pub mod vocab;
pub mod wire;
