//! Pont audio vers la Web Audio API pour la cible WebAssembly
//! (`wasm32-unknown-unknown`).
//!
//! Le backend wasm de `quad-snd` (src/web_snd.rs) déclare 9 fonctions
//! `extern "C"` (`audio_init`, `audio_add_buffer`…) **sans** `#[link(
//! wasm_import_module = "env")]` : sans définition locale, le lien échoue
//! (`rust-lld`: undefined symbol). Ces définitions `#[no_mangle]` satisfont le
//! lien et ne font que relayer vers les implémentations réelles en JavaScript
//! (Web Audio API), branchées comme imports `env` (`mm_audio_*`) dans
//! `web/index.html` - même mécanisme que la persistance (`persist.rs`,
//! `mmcfg_read`/`mmcfg_write`). Sur natif, rien n'est nécessaire (les vraies
//! implémentations de `quad-snd` y existent).
#![cfg(target_arch = "wasm32")]

#[link(wasm_import_module = "env")]
unsafe extern "C" {
    fn mm_audio_init();
    fn mm_audio_add_buffer(content: *const u8, content_len: u32) -> u32;
    fn mm_audio_play_buffer(buffer: u32, volume: f32, repeat: bool) -> u32;
    fn mm_audio_source_is_loaded(buffer: u32) -> bool;
    fn mm_audio_source_set_volume(buffer: u32, volume: f32);
    fn mm_audio_source_stop(buffer: u32);
    fn mm_audio_source_delete(buffer: u32);
    fn mm_audio_playback_stop(playback: u32);
    fn mm_audio_playback_set_volume(playback: u32, volume: f32);
}

#[unsafe(no_mangle)]
pub extern "C" fn audio_init() {
    unsafe { mm_audio_init() }
}

#[unsafe(no_mangle)]
pub extern "C" fn audio_add_buffer(content: *const u8, content_len: u32) -> u32 {
    unsafe { mm_audio_add_buffer(content, content_len) }
}

#[unsafe(no_mangle)]
pub extern "C" fn audio_play_buffer(buffer: u32, volume: f32, repeat: bool) -> u32 {
    unsafe { mm_audio_play_buffer(buffer, volume, repeat) }
}

#[unsafe(no_mangle)]
pub extern "C" fn audio_source_is_loaded(buffer: u32) -> bool {
    unsafe { mm_audio_source_is_loaded(buffer) }
}

#[unsafe(no_mangle)]
pub extern "C" fn audio_source_set_volume(buffer: u32, volume: f32) {
    unsafe { mm_audio_source_set_volume(buffer, volume) }
}

#[unsafe(no_mangle)]
pub extern "C" fn audio_source_stop(buffer: u32) {
    unsafe { mm_audio_source_stop(buffer) }
}

#[unsafe(no_mangle)]
pub extern "C" fn audio_source_delete(buffer: u32) {
    unsafe { mm_audio_source_delete(buffer) }
}

#[unsafe(no_mangle)]
pub extern "C" fn audio_playback_stop(playback: u32) {
    unsafe { mm_audio_playback_stop(playback) }
}

#[unsafe(no_mangle)]
pub extern "C" fn audio_playback_set_volume(playback: u32, volume: f32) {
    unsafe { mm_audio_playback_set_volume(playback, volume) }
}