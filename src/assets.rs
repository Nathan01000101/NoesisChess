use macroquad::{
    audio::{Sound, load_sound_from_bytes},
    prelude::*,
};

pub struct Assets {
    pub font: Font,
    pub move_check: Sound,
    pub move_normal: Sound,
    pub move_capture: Sound,
    pub move_castle: Sound,
    pub game_finished: Sound,
}
pub async fn load() -> Assets {
    Assets {
        font: load_ttf_font_from_bytes(include_bytes!("../assets/FreeSerif.ttf")).unwrap(),
        move_check: load_sound_from_bytes(include_bytes!("../assets/move-check.wav"))
            .await
            .unwrap(),
        move_normal: load_sound_from_bytes(include_bytes!("../assets/move-self.wav"))
            .await
            .unwrap(),
        move_capture: load_sound_from_bytes(include_bytes!("../assets/capture.wav"))
            .await
            .unwrap(),
        move_castle: load_sound_from_bytes(include_bytes!("../assets/castle.wav"))
            .await
            .unwrap(),
        game_finished: load_sound_from_bytes(include_bytes!("../assets/game-end.wav"))
            .await
            .unwrap(),
    }
}
