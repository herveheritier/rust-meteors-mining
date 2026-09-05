//! Audio - portage des sons de `meteorsMining.bas` (Phase 4).
//!
//! Correspondance avec l'original :
//! - `sh1&`  = mis4.ogg     - tir de balle (`_sndplay` à chaque tir)
//! - `sh5&`  = gem1.ogg     - ramassage d.un minerai (volume 0.05)
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
    mineral: Sound,
    explosions: Vec<Sound>,
    engine: Sound,
    reverse: Sound,
    ambient: Sound,
    music: Sound,
    /// Bip d'approche de l'accostage - **synthétisé en mémoire** au
    /// chargement (aucun fichier : courte impulsion sinusoïdale, voir
    /// `synth_approach_beep`). Émis d'autant plus souvent que le vaisseau est
    /// près du centre de la station (messages clignotants au-dessus du
    /// vaisseau lors du retour à la base).
    approach_beep: Sound,
    /// Son « accostage réussi » - **synthétisé en mémoire** (deux notes
    /// ascendantes, distinctes du bip d'approche, voir `synth_dock_ok`). Émis
    /// une seule fois au moment où le vaisseau est capturé (début de
    /// l'animation d'accostage) ; après l'accostage, plus aucun son.
    dock_ok: Sound,
    /// Musique en lecture (touche M, ex `_sndpaused(sh7&)`).
    pub music_on: bool,
    /// Volume maître (0.0..=1.0), réglable dans l'écran de paramétrage
    /// (touche O) et persisté dans le fichier de config.
    pub volume: f32,
    /// Sous-volume de la musique (0.0..=1.0), réglable dans l'écran de
    /// paramétrage et persisté (clé `music_volume`). Multiplié au maître.
    pub music_volume: f32,
    /// Sous-volume des effets (tirs, explosions, minerais, moteurs)
    /// (0.0..=1.0), réglable dans l'écran de paramétrage et persisté (clé
    /// `effects_volume`). Multiplié au maître.
    pub effects_volume: f32,
    /// Sous-volume de l'ambiance (0.0..=1.0), réglable dans l'écran de
    /// paramétrage et persisté (clé `ambient_volume`). Multiplié au maître.
    pub ambient_volume: f32,
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
        // Modding : un fichier `user_assets/<nom>` remplace le son embarqué
        // (voir `modding.rs`).
        async fn load(bytes: &'static [u8], name: &str) -> Sound {
            let owned = crate::modding::asset_bytes(name, bytes);
            audio::load_sound_from_bytes(&owned)
                .await
                .unwrap_or_else(|e| panic!("{name} illisible - {e}"))
        }

        let mut explosions = Vec::with_capacity(10);
        for i in 0..10 {
            let name = format!("exp{}.ogg", i + 11);
            let bytes: &'static [u8] = match i {
                0 => &include_bytes!("../assets/exp11.ogg")[..],
                1 => &include_bytes!("../assets/exp12.ogg")[..],
                2 => &include_bytes!("../assets/exp13.ogg")[..],
                3 => &include_bytes!("../assets/exp14.ogg")[..],
                4 => &include_bytes!("../assets/exp15.ogg")[..],
                5 => &include_bytes!("../assets/exp16.ogg")[..],
                6 => &include_bytes!("../assets/exp17.ogg")[..],
                7 => &include_bytes!("../assets/exp18.ogg")[..],
                8 => &include_bytes!("../assets/exp19.ogg")[..],
                _ => &include_bytes!("../assets/exp20.ogg")[..],
            };
            explosions.push(load(bytes, &name).await);
        }

        Sounds {
            bullet: load(include_bytes!("../assets/mis4.ogg"), "mis4.ogg").await,
            mineral: load(include_bytes!("../assets/gem1.ogg"), "gem1.ogg").await,
            explosions,
            engine: load(include_bytes!("../assets/fffff.ogg"), "fffff.ogg").await,
            reverse: load(include_bytes!("../assets/fffff.ogg"), "fffff.ogg").await,
            ambient: load(include_bytes!("../assets/bruitDeFond.ogg"), "bruitDeFond.ogg").await,
            music: load(include_bytes!("../assets/music1.ogg"), "music1.ogg").await,
            // sons synthétisés en code (aucun fichier `assets/`) : le backend
            // quad-snd/miniaudio décode le WAV PCM comme l'Ogg Vorbis
            approach_beep: audio::load_sound_from_bytes(&synth_approach_beep())
                .await
                .expect("bip d'approche synthétisé illisible"),
            dock_ok: audio::load_sound_from_bytes(&synth_dock_ok())
                .await
                .expect("son d'accostage synthétisé illisible"),
            music_on: false,
            volume: 1.0,
            music_volume: 1.0,
            effects_volume: 1.0,
            ambient_volume: 1.0,
            engine_on: false,
            reverse_on: false,
            ambient_on: false,
        }
    }

    /// Volume effectif d'une boucle : maître × sous-volume du canal, puis
    /// atténuation propre à la source (la musique joue à 0.1 comme l'original).
    fn loop_gain(&self, channel: f32, source_gain: f32) -> f32 {
        (self.volume * channel * source_gain).clamp(0.0, 1.0)
    }

    /// Applique le volume maître (0.0..=1.0) à tous les sons. Les boucles en
    /// cours (ambiance, musique, moteurs) sont relancées au nouveau volume.
    pub fn set_volume(&mut self, volume: f32) {
        self.volume = volume.clamp(0.0, 1.0);
        self.apply_gains();
    }

    /// Rejoue les boucles en cours avec les volumes actuels (maître × sous-
    /// volumes). Appelé après un changement de sous-volume (écran de
    /// paramétrage) pour appliquer la nouvelle valeur sans toucher au maître.
    pub fn apply_gains(&mut self) {
        if self.ambient_on {
            audio::stop_sound(&self.ambient);
            audio::play_sound(
                &self.ambient,
                PlaySoundParams {
                    looped: true,
                    volume: self.ambient_gain(),
                },
            );
        }
        if self.music_on {
            audio::stop_sound(&self.music);
            audio::play_sound(
                &self.music,
                PlaySoundParams {
                    looped: true,
                    volume: self.music_gain(),
                },
            );
        }
        if self.engine_on {
            audio::stop_sound(&self.engine);
            audio::play_sound(
                &self.engine,
                PlaySoundParams {
                    looped: true,
                    volume: self.effects_gain(),
                },
            );
        }
        if self.reverse_on {
            audio::stop_sound(&self.reverse);
            audio::play_sound(
                &self.reverse,
                PlaySoundParams {
                    looped: true,
                    volume: self.effects_gain(),
                },
            );
        }
    }

    /// Gain de l'ambiance (`maître × ambient_volume`).
    pub fn ambient_gain(&self) -> f32 {
        self.loop_gain(self.ambient_volume, 1.0)
    }

    /// Gain de la musique (`0.1 × maître × music_volume`, le 0.1 vient de
    /// l'original).
    pub fn music_gain(&self) -> f32 {
        self.loop_gain(self.music_volume, 0.1)
    }

    /// Gain des effets (moteurs, tirs, explosions, minerais) : `maître ×
    /// effects_volume`.
    pub fn effects_gain(&self) -> f32 {
        self.loop_gain(self.effects_volume, 1.0)
    }

    // ─── Effets ponctuels ───────────────────────────────────────────────────

    /// Tir de balle (ex `_sndplay sh1&`), au volume maître × effets.
    pub fn play_bullet(&self) {
        audio::play_sound(
            &self.bullet,
            PlaySoundParams {
                looped: false,
                volume: self.effects_gain(),
            },
        );
    }

    /// Ramassage d'un minerai (ex `_sndplay sh5&`, volume 0.05 × maître ×
    /// effets).
    pub fn play_mineral(&self) {
        audio::play_sound(
            &self.mineral,
            PlaySoundParams {
                looped: false,
                volume: 0.05 * self.effects_gain(),
            },
        );
    }

    /// Explosion d'un triangle (ex `shexp(s%)` aléatoire) au volume donné
    /// (déjà calculé selon la distance au vaisseau) × volume maître × effets.
    pub fn play_explosion(&self, rng: &mut impl Rng, volume: f32) {
        let idx = rng.gen_range(0..self.explosions.len());
        audio::play_sound(
            &self.explosions[idx],
            PlaySoundParams {
                looped: false,
                volume: (volume * self.effects_gain()).max(0.0),
            },
        );
    }

    /// Bip de proximité de l'accostage (messages clignotants au-dessus du
    /// vaisseau lors du retour à la base) : son court et discret (× 0.6), au
    /// volume maître × effets, **modulé par la trajectoire** (`traj_gain` de
    /// `docking::approach_beep_traj_gain` : plus le vaisseau est aligné sur le
    /// centre de la zone d'accostage, plus le bip est fort) - émis d'autant
    /// plus souvent que le vaisseau approche du centre
    /// (`docking::update_dock_approach`).
    pub fn play_approach_beep(&self, traj_gain: f32) {
        audio::play_sound(
            &self.approach_beep,
            PlaySoundParams {
                looped: false,
                volume: 0.6 * self.effects_gain() * traj_gain,
            },
        );
    }

    /// Son « accostage réussi » : distinct du bip d'approche, émis une seule
    /// fois au moment où le vaisseau est **capturé** (l'animation d'accostage
    /// démarre) - après l'accostage, plus aucun son (`docking` coupe le
    /// guide).
    pub fn play_dock_ok(&self) {
        audio::play_sound(
            &self.dock_ok,
            PlaySoundParams {
                looped: false,
                volume: 0.8 * self.effects_gain(),
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
                volume: self.ambient_gain(),
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
                    volume: self.effects_gain(),
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
                    volume: self.effects_gain(),
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
                    volume: self.music_gain(),
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

// ─── Synthèse en code (aucun fichier `assets/`) ─────────────────────────────

/// Construit un `.wav` PCM 16 bits mono en mémoire (44 octets d'en-tête + les
/// échantillons) : le backend quad-snd/miniaudio décode le WAV comme l'Ogg
/// Vorbis des autres sons. `gen` reçoit le temps `t` (s) et renvoie
/// l'échantillon dans [-1, 1]. Les deux nouveaux sons de l'accostage (bip de
/// proximité + « accostage réussi ») sont générés ainsi au chargement - pas
/// d'asset à ajouter au dépôt.
fn synth_wav(duration: f32, sample_rate: u32, mut sample: impl FnMut(f32) -> f32) -> Vec<u8> {
    let n = (duration * sample_rate as f32) as usize;
    let data_len = n * 2; // mono 16 bits
    let mut wav = Vec::with_capacity(44 + data_len);
    wav.extend_from_slice(b"RIFF");
    wav.extend_from_slice(&(36 + data_len as u32).to_le_bytes());
    wav.extend_from_slice(b"WAVE");
    wav.extend_from_slice(b"fmt ");
    wav.extend_from_slice(&16u32.to_le_bytes()); // taille du bloc fmt
    wav.extend_from_slice(&1u16.to_le_bytes()); // PCM
    wav.extend_from_slice(&1u16.to_le_bytes()); // mono
    wav.extend_from_slice(&sample_rate.to_le_bytes());
    wav.extend_from_slice(&(sample_rate * 2).to_le_bytes()); // débit (octets/s)
    wav.extend_from_slice(&2u16.to_le_bytes()); // alignement bloc
    wav.extend_from_slice(&16u16.to_le_bytes()); // bits par échantillon
    wav.extend_from_slice(b"data");
    wav.extend_from_slice(&(data_len as u32).to_le_bytes());
    for i in 0..n {
        let t = i as f32 / sample_rate as f32;
        let s = (sample(t) * 32767.0).clamp(-32768.0, 32767.0) as i16;
        wav.extend_from_slice(&s.to_le_bytes());
    }
    wav
}

/// Enveloppe d'amplitude en trapèze (attaque rapide, relâchement progressif)
/// pour éviter les clics en début/fin d'échantillon.
fn trapezoid_env(t: f32, attack: f32, release: f32) -> f32 {
    let a = (t / attack).min(1.0);
    let r = ((1.0 - t) / release).clamp(0.0, 1.0);
    a * r
}

/// Bip d'approche de l'accostage : courte impulsion sinusoïdale aiguë
/// (~880 Hz, 70 ms) avec une fondamentale et une petite octave au-dessus
/// (timbre métallique léger).
fn synth_approach_beep() -> Vec<u8> {
    const DURATION: f32 = 0.07;
    const FREQ: f32 = 880.0;
    synth_wav(DURATION, 22050, |t| {
        let env = trapezoid_env(t / DURATION, 0.08, 0.15);
        let s = (std::f32::consts::TAU * FREQ * t).sin()
            + 0.5 * (std::f32::consts::TAU * 2.0 * FREQ * t).sin();
        0.45 * env * s / 1.5
    })
}

/// Son « accostage réussi » : deux notes **ascendantes** (660 → 990 Hz,
/// ~0,4 s) bien distinctes du bip d'approche - « c'est bon », l'accostage
/// est engagé. Chaque note a sa propre enveloppe (pas de clic entre les deux).
fn synth_dock_ok() -> Vec<u8> {
    const DURATION: f32 = 0.42;
    const NOTE_1: f32 = 660.0; // mi5
    const NOTE_2: f32 = 990.0; // si5
    synth_wav(DURATION, 22050, |t| {
        // deux segments de hauteur distincte, enchaînés sans silence
        let (freq, start) = if t < 0.16 { (NOTE_1, 0.0) } else { (NOTE_2, 0.16) };
        let lt = t - start;
        let seg_len = if t < 0.16 { 0.16 } else { DURATION - 0.16 };
        let env = trapezoid_env(lt / seg_len, 0.08, 0.2);
        let s = (std::f32::consts::TAU * freq * lt).sin()
            + 0.4 * (std::f32::consts::TAU * 2.0 * freq * lt).sin();
        0.5 * env * s / 1.4
    })
}
