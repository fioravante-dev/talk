//! A bandeja no Windows ainda não existe (vem na fase 2, com o `tray-icon`).
//! Este módulo tem a mesma cara do `tray.rs`, e o `spawn` falha: o serviço já
//! segue sem bandeja quando ela não sobe. Por isso nada aqui é lido ainda.
#![allow(dead_code)]

use std::marker::PhantomData;
use std::sync::mpsc::Sender;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Look {
    Idle,
    Recording { bright: bool },
    Transcribing,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Toggle,
    Open,
    CopyLast,
    Quit,
}

#[derive(Debug)]
pub struct Tray {
    pub look: Look,
    pub tooltip: String,
    pub actions: Sender<Action>,
}

pub struct Handle<T>(PhantomData<T>);

impl<T> Handle<T> {
    pub fn update(&self, _f: impl FnOnce(&mut T)) {}
}

pub trait TrayMethods: Sized {
    fn spawn(self) -> Result<Handle<Self>, &'static str> {
        Err("a bandeja do Windows ainda não foi feita")
    }
}

impl TrayMethods for Tray {}
