//! Audio - portage des sons de `meteorsMining.bas` (Phase 4).
//!
//! Correspondance avec l'original :
//! - `sh1&`  = mis4.ogg     - tir de balle (`_sndplay` à chaque tir)
//! - `sh5&`  = gem1.ogg     - ramassage d'une gemme (volume 0.05)
//! - `shexp` = exp11..20.ogg - explosion d'un triangle, volume selon la
//!   distance au vaisseau (`v! = (1 − dist/diag)^3`, un son au hasard)
//! - `sh6&`  = bruitDeFond.ogg - ambiance en boucle (`_sndloop`)
//! - `sh7&`  = music1.ogg   - musique en boucle (volume 0.1), bascule M
//! - `sh8&`/`sh9&` = fffff.ogg (2 exemplaires) - boucle moteur avant / recul
//!   (`_sndloop` tant que `thrusted`/`revertThrusted`, sinon `_sndpause`)
//!
//! Les boucles gardent leur état (`engine_on`…) pour ne pas relancer un son
//! déjà en lecture à chaque frame (ce qui le ferait repartir de zéro).

use macroquad::audio::{self, PlaySoundParams, Sound};
use ::rand::Rng;

/// Sons chargés au démarrage (ex les `_sndopen` de `meteorsMining.bas`).
pub struct Sounds {
    bullet: Sound,
    gem: Sound,
    explosions: Vec<Sound>,
    engine: Sound,
    reverse: Sound,
    ambient: Sound,
    music: Sound,
    /// Musique en lecture (touche M, ex `_sndpaused(sh7&)`).
    pub music_on: bool,
    /// Volume maître (0.0..=1.0), réglable dans l'écran de paramétrage
    /// (touche O) et persisté dans le fichier de config.
    pub volume: f32,
    engine_on: bool,
    reverse_on: bool,
    ambient_on: bool,
}

impl Sounds {
    /// Charge tous les sons depuis `assets/` (copie des `.ogg` de référence -
    /// le backend quad-snd/miniaudio décode l'Ogg Vorbis).
    pub async fn load() -> Sounds {
        // Sons intégrés dans le binaire (`include_bytes!`) : l'exécutable est
        // autonome, le dossier `assets/` n'est plus nécessaire au runtime.
        async fn load(bytes: &'static [u8], name: &str) -> Sound {
            audio::load_sound_from_bytes(bytes)
                .await
                .unwrap_or_else(|e| panic!("{name} illisible - {e}"))
        }

        let mut explosions = Vec::with_capacity(10);
        for (i, bytes) in [
            &include_bytes!("../assets/exp11.ogg")[..],
            &include_bytes!("../assets/exp12.ogg")[..],
            &include_bytes!("../assets/exp13.ogg")[..],
            &include_bytes!("../assets/exp14.ogg")[..],
            &include_bytes!("../assets/exp15.ogg")[..],
            &include_bytes!("../assets/exp16.ogg")[..],
            &include_bytes!("../assets/exp17.ogg")[..],
            &include_bytes!("../assets/exp18.ogg")[..],
            &include_bytes!("../assets/exp19.ogg")[..],
            &include_bytes!("../assets/exp20.ogg")[..],
        ]
        .iter()
        .enumerate()
        {
            explosions.push(load(bytes, &format!("assets/exp{}.ogg", i + 11)).await);
        }

        Sounds {
            bullet: load(include_bytes!("../assets/mis4.ogg"), "assets/mis4.ogg").await,
            gem: load(include_bytes!("../assets/gem1.ogg"), "assets/gem1.ogg").await,
            explosions,
            engine: load(include_bytes!("../assets/fffff.ogg"), "assets/fffff.ogg").await,
            reverse: load(include_bytes!("../assets/fffff.ogg"), "assets/fffff.ogg").await,
            ambient: load(include_bytes!("../assets/bruitDeFond.ogg"), "assets/bruitDeFond.ogg").await,
            music: load(include_bytes!("../assets/music1.ogg"), "assets/music1.ogg").await,
            music_on: false,
            volume: 1.0,
            engine_on: false,
            reverse_on: false,
            ambient_on: false,
        }
    }

    /// Applique le volume maître (0.0..=1.0) à tous les sons. Les boucles en
    /// cours (ambiance, musique, moteurs) sont relancées au nouveau volume.
    pub fn set_volume(&mut self, volume: f32) {
        self.volume = volume.clamp(0.0, 1.0);
        if self.ambient_on {
            audio::stop_sound(&self.ambient);
            audio::play_sound(
                &self.ambient,
                PlaySoundParams {
                    looped: true,
                    volume: self.volume,
                },
            );
        }
        if self.music_on {
            audio::stop_sound(&self.music);
            audio::play_sound(
                &self.music,
                PlaySoundParams {
                    looped: true,
                    volume: 0.1 * self.volume,
                },
            );
        }
        if self.engine_on {
            audio::stop_sound(&self.engine);
            audio::play_sound(
                &self.engine,
                PlaySoundParams {
                    looped: true,
                    volume: self.volume,
                },
            );
        }
        if self.reverse_on {
            audio::stop_sound(&self.reverse);
            audio::play_sound(
                &self.reverse,
                PlaySoundParams {
                    looped: true,
                    volume: self.volume,
                },
            );
        }
    }

    // ─── Effets ponctuels ───────────────────────────────────────────────────

    /// Tir de balle (ex `_sndplay sh1&`), au volume maître.
    pub fn play_bullet(&self) {
        audio::play_sound(
            &self.bullet,
            PlaySoundParams {
                looped: false,
                volume: self.volume,
            },
        );
    }

    /// Ramassage d'une gemme (ex `_sndplay sh5&`, volume 0.05 × maître).
    pub fn play_gem(&self) {
        audio::play_sound(
            &self.gem,
            PlaySoundParams {
                looped: false,
                volume: 0.05 * self.volume,
            },
        );
    }

    /// Explosion d'un triangle (ex `shexp(s%)` aléatoire) au volume donné
    /// (déjà calculé selon la distance au vaisseau) × volume maître.
    pub fn play_explosion(&self, rng: &mut impl Rng, volume: f32) {
        let idx = rng.gen_range(0..self.explosions.len());
        audio::play_sound(
            &self.explosions[idx],
            PlaySoundParams {
                looped: false,
                volume: (volume * self.volume).max(0.0),
            },
        );
    }

    // ─── Boucles ────────────────────────────────────────────────────────────

    /// Ambiance de fond (ex `_sndloop sh6&` au démarrage).
    pub fn start_ambient(&mut self) {
        if self.ambient_on {
            return;
        }
        self.ambient_on = true;
        audio::play_sound(
            &self.ambient,
            PlaySoundParams {
                looped: true,
                volume: self.volume,
            },
        );
    }

    /// Moteur avant : boucle `fffff.ogg` tant que le vaisseau pousse
    /// (ex `_sndloop/_sndpause sh8&`).
    pub fn engine(&mut self, on: bool) {
        if on == self.engine_on {
            return;
        }
        self.engine_on = on;
        if on {
            audio::play_sound(
                &self.engine,
                PlaySoundParams {
                    looped: true,
                    volume: self.volume,
                },
            );
        } else {
            audio::stop_sound(&self.engine);
        }
    }

    /// Moteur arrière : idem sur le second exemplaire de `fffff.ogg`
    /// (ex `_sndloop/_sndpause sh9&`).
    pub fn reverse_engine(&mut self, on: bool) {
        if on == self.reverse_on {
            return;
        }
        self.reverse_on = on;
        if on {
            audio::play_sound(
                &self.reverse,
                PlaySoundParams {
                    looped: true,
                    volume: self.volume,
                },
            );
        } else {
            audio::stop_sound(&self.reverse);
        }
    }

    /// Bascule la musique (touche M, ex `if _sndpaused(sh7&) then _sndloop
    /// sh7& else _sndpause sh7&`), volume 0.1 comme l'original.
    pub fn toggle_music(&mut self) {
        if self.music_on {
            audio::stop_sound(&self.music);
            self.music_on = false;
        } else {
            audio::play_sound(
                &self.music,
                PlaySoundParams {
                    looped: true,
                    volume: 0.1 * self.volume,
                },
            );
            self.music_on = true;
        }
    }

    /// Démarre la musique (au lancement de la partie, comme l'original qui la
    /// boucle dès `mainLoop`).
    pub fn start_music(&mut self) {
        if !self.music_on {
            self.toggle_music();
        }
    }
}
