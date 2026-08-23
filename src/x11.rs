//! Accès minimal à X11 (EWMH) pour le plein écran.
//!
//! miniquad 0.4.11 (dernière version publiée) ne sait pas sortir du plein
//! écran sur X11 : son `set_fullscreen(false)` envoie un ClientMessage
//! `_NET_WM_STATE` avec un **atome vide** (action ADD) au lieu d'un REMOVE
//! (TODO dans `linux_x11.rs`). Ce module envoie le ClientMessage EWMH
//! **correct** (ADD/REMOVE de `_NET_WM_STATE_FULLSCREEN`) directement via
//! libX11, sans dépendre d'un outil externe (`wmctrl`).
//!
//! La fenêtre cible est retrouvée par `XGetInputFocus` (le jeu a le focus
//! quand on presse F), avec un repli sur la recherche par titre
//! (`XQueryTree` + `XFetchName`) si le focus est sur la racine.

#![allow(non_snake_case, non_camel_case_types, dead_code)]

use std::ffi::{c_char, c_int, c_long, c_uint, c_ulong, c_void, CStr};

use crate::config::WINDOW_TITLE;

// ─── Types X11 (opaques) ─────────────────────────────────────────────────────

type Display = c_void;
type Window = c_ulong;
type Atom = c_ulong;

/// Valeurs `Bool` X11.
const FALSE: c_int = 0;
const TRUE: c_int = 1;

/// `ClientMessage` (événement X11 type 33) : porté à la root window, c'est le
/// mécanisme standard EWMH pour demander un changement d'état au WM.
const CLIENT_MESSAGE: c_int = 33;

/// Masques d'événement pour `XSendEvent` vers la root (EWMH).
const SUBSTRUCTURE_REDIRECT_MASK: c_long = 1 << 20;
const SUBSTRUCTURE_NOTIFY_MASK: c_long = 1 << 19;

/// Actions `_NET_WM_STATE`.
const NET_WM_STATE_REMOVE: c_long = 0;
const NET_WM_STATE_ADD: c_long = 1;

/// `XClientMessageEvent` (ABI libX11, `xlib.h`) - ordre des champs identique
/// à la déclaration de miniquad (`libx11.rs`).
#[repr(C)]
struct XClientMessageEvent {
    type_: c_int,
    serial: c_ulong,
    send_event: c_int,
    display: *mut Display,
    window: Window,
    message_type: Atom,
    format: c_int,
    data: [c_long; 5],
}

/// `XEvent` (union) : on ne lit que la partie `xclient` ; la struct ci-dessus
/// est plus petite que l'union, ce qui est sans danger (Xlib lit selon le
/// champ `type`).
#[repr(C)]
struct XEvent {
    xclient: XClientMessageEvent,
}

#[cfg(target_os = "linux")]
#[link(name = "X11")]
extern "C" {
    fn XOpenDisplay(name: *const c_char) -> *mut Display;
    fn XCloseDisplay(display: *mut Display) -> c_int;
    fn XDefaultRootWindow(display: *mut Display) -> Window;
    fn XInternAtom(display: *mut Display, name: *const c_char, only_if_exists: c_int) -> Atom;
    fn XGetInputFocus(display: *mut Display, focus_return: *mut Window, revert_to: *mut c_int) -> c_int;
    fn XQueryTree(
        display: *mut Display,
        w: Window,
        root_return: *mut Window,
        parent_return: *mut Window,
        children_return: *mut *mut Window,
        nchildren_return: *mut c_uint,
    ) -> c_int;
    fn XFetchName(display: *mut Display, w: Window, name_return: *mut *mut c_char) -> c_int;
    fn XFree(data: *mut c_void) -> c_int;
    fn XSendEvent(display: *mut Display, w: Window, propagate: c_int, event_mask: c_long, event: *mut XEvent) -> c_int;
    fn XFlush(display: *mut Display) -> c_int;
    fn XTranslateCoordinates(
        display: *mut Display,
        src_w: Window,
        dest_w: Window,
        src_x: c_int,
        src_y: c_int,
        dest_x_return: *mut c_int,
        dest_y_return: *mut c_int,
        child_return: *mut Window,
    ) -> c_int;
    fn XMoveWindow(display: *mut Display, w: Window, x: c_int, y: c_int) -> c_int;
}

/// Ouvre le display par défaut et exécute `f` dessus, puis le ferme.
///
/// Retourne `None` si le display n'est pas ouvrable (pas de serveur X11).
fn with_display<T>(f: impl FnOnce(*mut Display) -> T) -> Option<T> {
    #[cfg(not(target_os = "linux"))]
    {
        let _ = f;
        None
    }

    #[cfg(target_os = "linux")]
    unsafe {
        let display = XOpenDisplay(std::ptr::null());
        if display.is_null() {
            return None;
        }
        let result = f(display);
        XCloseDisplay(display);
        Some(result)
    }
}

/// Passe la fenêtre du jeu en plein écran (`add = true`) ou l'en fait sortir
/// (`add = false`) via le ClientMessage `_NET_WM_STATE` (EWMH).
///
/// Retourne `true` si le message a été envoyé au WM (le WM fait le reste).
pub fn set_fullscreen(add: bool) -> bool {
    with_display(|display| unsafe { send_fullscreen_message(display, add) }).unwrap_or(false)
}

/// Position de la fenêtre du jeu (coin supérieur gauche du client, relatif à
/// la racine X) via `XTranslateCoordinates` - utilisée pour persister et
/// restaurer la position de la fenêtre fenêtrée. `None` si indisponible (pas
/// de serveur X11 ou fenêtre introuvable).
pub fn window_position() -> Option<(i32, i32)> {
    with_display(|display| unsafe {
        let window = find_game_window(display)?;
        let root = XDefaultRootWindow(display);
        let mut x: c_int = 0;
        let mut y: c_int = 0;
        let mut child: Window = 0;
        if XTranslateCoordinates(display, window, root, 0, 0, &mut x, &mut y, &mut child) != 0 {
            Some((x, y))
        } else {
            None
        }
    })
    .flatten()
}

/// Déplace la fenêtre du jeu à la position donnée (coin supérieur gauche,
/// relatif à la racine X). Retourne `false` si indisponible (pas de serveur
/// X11 ou fenêtre introuvable).
pub fn move_window(x: i32, y: i32) -> bool {
    with_display(|display| unsafe {
        let Some(window) = find_game_window(display) else {
            return false;
        };
        XMoveWindow(display, window, x, y) != 0
    })
    .unwrap_or(false)
}

/// Envoie le ClientMessage `_NET_WM_STATE` avec l'action ADD/REMOVE de
/// `_NET_WM_STATE_FULLSCREEN` à la root window, pour la fenêtre du jeu.
#[cfg(target_os = "linux")]
unsafe fn send_fullscreen_message(display: *mut Display, add: bool) -> bool {
    let wm_state = XInternAtom(display, c"_NET_WM_STATE".as_ptr(), FALSE);
    let wm_fullscreen = XInternAtom(display, c"_NET_WM_STATE_FULLSCREEN".as_ptr(), FALSE);
    if wm_state == 0 || wm_fullscreen == 0 {
        return false;
    }

    let Some(window) = find_game_window(display) else {
        return false;
    };

    let mut event = XEvent {
        xclient: XClientMessageEvent {
            type_: CLIENT_MESSAGE,
            serial: 0,
            send_event: TRUE,
            display,
            window,
            message_type: wm_state,
            format: 32,
            data: [
                if add { NET_WM_STATE_ADD } else { NET_WM_STATE_REMOVE },
                wm_fullscreen as c_long,
                0,
                0,
                0,
            ],
        },
    };

    let root = XDefaultRootWindow(display);
    let sent = XSendEvent(
        display,
        root,
        FALSE,
        SUBSTRUCTURE_REDIRECT_MASK | SUBSTRUCTURE_NOTIFY_MASK,
        &mut event,
    );
    XFlush(display);
    sent != 0
}

/// Retrouve la fenêtre du jeu : le focus X (le jeu a le focus quand on presse
/// F), sinon recherche par titre dans l'arbre des fenêtres.
#[cfg(target_os = "linux")]
unsafe fn find_game_window(display: *mut Display) -> Option<Window> {
    let mut focus: Window = 0;
    let mut revert_to: c_int = 0;
    // `XGetInputFocus` renvoie 1 (PointerRoot) ou 0 (None) si rien n'a le
    // focus : dans ce cas on cherche par titre.
    if XGetInputFocus(display, &mut focus, &mut revert_to) != 0
        && focus != 0
        && focus != 1
        && window_has_title(display, focus)
    {
        return Some(focus);
    }
    let root = XDefaultRootWindow(display);
    find_window_by_title(display, root)
}

/// Vrai si la fenêtre porte le titre du jeu (`WM_NAME`).
#[cfg(target_os = "linux")]
unsafe fn window_has_title(display: *mut Display, window: Window) -> bool {
    let mut name: *mut c_char = std::ptr::null_mut();
    if XFetchName(display, window, &mut name) == 0 || name.is_null() {
        return false;
    }
    let matches = CStr::from_ptr(name).to_bytes() == WINDOW_TITLE.as_bytes();
    XFree(name as *mut c_void);
    matches
}

/// Recherche récursive de la fenêtre portant le titre du jeu.
#[cfg(target_os = "linux")]
unsafe fn find_window_by_title(display: *mut Display, window: Window) -> Option<Window> {
    if window_has_title(display, window) {
        return Some(window);
    }
    let mut root: Window = 0;
    let mut parent: Window = 0;
    let mut children: *mut Window = std::ptr::null_mut();
    let mut nchildren: c_uint = 0;
    if XQueryTree(
        display,
        window,
        &mut root,
        &mut parent,
        &mut children,
        &mut nchildren,
    ) == 0
        || children.is_null()
    {
        return None;
    }
    let mut found = None;
    for i in 0..nchildren as usize {
        if let Some(w) = find_window_by_title(display, *children.add(i)) {
            found = Some(w);
            break;
        }
    }
    XFree(children as *mut c_void);
    found
}
