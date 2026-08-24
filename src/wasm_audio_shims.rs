//! Shims audio **no-op** pour la cible WebAssembly (`wasm32-unknown-unknown`).
//!
//! Le backend audio de `quad-snd` (crate `macroquad` feature `audio`) déclare
//! des fonctions `extern "C"` (`audio_init`, `audio_play_buffer`…) qui sont
//! fournies par un backend C/JS absent sur wasm - sans elles, le lien échoue
//! (`rust-lld`: undefined symbol). Ces définitions `#[no_mangle]` satisfont le
//! lien : l'audio devient **silencieux** sur le web (comportement accepté,
//! voir README). Rien n'est nécessité côté natif (les vraies
//! implémentations y existent).
#![cfg(target_arch = "wasm32")]

#[unsafe(no_mangle)]
pub extern "C" fn audio_init() {}

#[unsafe(no_mangle)]
pub extern "C" fn audio_add_buffer(_content: *const u8, _content_len: u32) -> u32 {
    0
}

#[unsafe(no_mangle)]
pub extern "C" fn audio_play_buffer(_buffer: u32, _volume: f32, _repeat: bool) -> u32 {
    0
}

#[unsafe(no_mangle)]
pub extern "C" fn audio_source_is_loaded(_buffer: u32) -> bool {
    true
}

#[unsafe(no_mangle)]
pub extern "C" fn audio_source_set_volume(_buffer: u32, _volume: f32) {}

#[unsafe(no_mangle)]
pub extern "C" fn audio_source_stop(_buffer: u32) {}

#[unsafe(no_mangle)]
pub extern "C" fn audio_source_delete(_buffer: u32) {}

#[unsafe(no_mangle)]
pub extern "C" fn audio_playback_stop(_playback: u32) {}

#[unsafe(no_mangle)]
pub extern "C" fn audio_playback_set_volume(_playback: u32, _volume: f32) {}